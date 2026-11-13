//! The rules of the environment: obligation lifecycle, authorization, wake-ups and scheduling.
//! None of this depends on a model — "the agent exists permanently, its compute context does not".

use rusqlite::OptionalExtension as _;
use crate::error::{Error, Result};
use crate::model::*;
use crate::store::Store;
use crate::{now_ms, workset};
use rusqlite::params;

/// Defaults used when no grant matches. The user can override any of them per agent / project.
pub fn default_mode(action: &str) -> GrantMode {
    use crate::model::action::*;
    match action {
        MESSAGE_POST | MEMORY_WRITE | OBLIGATION_UPDATE | DECISION_PROPOSE | ARTIFACT_WRITE | OBLIGATION_CREATE | COLLEAGUE_CALL => GrantMode::Auto,
        // Delegation, promotion to shared knowledge and decisions that change the project are the
        // user's call until they say otherwise.
        OBLIGATION_DELEGATE => GrantMode::Auto,
        MEMORY_PROMOTE | DECISION_ACCEPT | EXTERNAL_SEND | PROFILE_CHANGE | PROJECT_CREATE | MCP_INSTALL | API_CONNECT | OBLIGATION_MANAGE => GrantMode::Approve,
        _ => GrantMode::Deny,
    }
}

#[derive(Debug, Clone, Default)]
pub struct TransitionArgs {
    pub wait: Option<WaitCondition>,
    pub blocker: Option<String>,
    pub outcome: Option<String>,
    pub next_step: Option<String>,
    pub note: String,
}

fn allowed(from: ObligationState, to: ObligationState) -> bool {
    use ObligationState::*;
    matches!(
        (from, to),
        (Accepted, Running | Waiting | Blocked | Cancelled | Done)
            | (Running, Accepted | Waiting | Blocked | Review | Done | Cancelled)
            | (Waiting, Accepted | Running | Blocked | Cancelled)
            | (Blocked, Accepted | Waiting | Cancelled)
            | (Review, Done | Accepted | Cancelled)
    )
}

#[derive(Debug, Default)]
pub struct ScheduleOutcome {
    pub woken: Vec<String>,
    pub sessions: Vec<Session>,
    pub blocked_by_budget: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ChatSend {
    pub thread_id: String,
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<serde_json::Value>,
    #[serde(default)]
    pub context: Vec<serde_json::Value>,
    pub reply_to: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Dispatched {
    pub user_message: Message,
    pub agent_messages: Vec<Message>,
    pub sessions: Vec<Session>,
}

impl Store {
    /// Change the state of an obligation, enforcing the invariants from Senate.md §9:
    /// "waiting" needs a resume condition, "blocked" needs the blocker, "done" needs a delivered
    /// result and no open delegated work, "cancelled" needs a reason.
    pub fn transition(&self, id: &str, to: ObligationState, args: TransitionArgs, actor: &str) -> Result<Obligation> {
        let cur = self.get_obligation(id)?;
        if cur.state == to {
            return Ok(cur);
        }
        if !allowed(cur.state, to) {
            return Err(Error::Transition(format!("{} → {}", cur.state.as_str(), to.as_str())));
        }
        let mut wait = None;
        let mut blocker = String::new();
        let mut outcome = args.outcome.clone();
        match to {
            ObligationState::Waiting => {
                let w = args.wait.clone().ok_or_else(|| Error::invalid("ожидание требует условия продолжения"))?;
                if let WaitCondition::Obligation { id: dep } = &w {
                    if dep == id || self.wait_would_cycle(id, dep)? {
                        return Err(Error::invalid("круговая зависимость между задачами"));
                    }
                }
                wait = Some(w);
            }
            ObligationState::Blocked => {
                blocker = args.blocker.clone().unwrap_or_default();
                if blocker.trim().is_empty() {
                    return Err(Error::invalid("укажите препятствие и какое вмешательство требуется"));
                }
            }
            ObligationState::Done => {
                let o = outcome.clone().unwrap_or_else(|| cur.outcome.clone());
                if o.trim().is_empty() {
                    return Err(Error::invalid("«готово» без результата не принимается — опишите, что получено"));
                }
                let open: i64 = self.lock().query_row(
                    "SELECT COUNT(*) FROM obligations WHERE parent_id=? AND state NOT IN ('done','cancelled')",
                    [id],
                    |r| r.get(0),
                )?;
                if open > 0 {
                    return Err(Error::invalid(format!("есть {open} незавершённых делегированных задач")));
                }
                outcome = Some(o);
            }
            ObligationState::Cancelled => {
                let o = outcome.clone().unwrap_or_default();
                if o.trim().is_empty() {
                    return Err(Error::invalid("укажите причину отмены"));
                }
                // Delegated work no longer has a purpose.
                let children: Vec<String> = {
                    let c = self.lock();
                    let mut st = c.prepare("SELECT id FROM obligations WHERE parent_id=? AND state NOT IN ('done','cancelled')")?;
                    let v = st.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
                    v
                };
                for ch in children {
                    let _ = self.transition(&ch, ObligationState::Cancelled, TransitionArgs { outcome: Some(format!("отменено вместе с «{}»", cur.title)), ..Default::default() }, actor);
                }
            }
            _ => {}
        }
        self.write_obligation_state(id, to, wait.as_ref(), &blocker, outcome.as_deref(), args.next_step.as_deref(), actor, &args.note)
    }

    fn wait_would_cycle(&self, start: &str, dep: &str) -> Result<bool> {
        let mut cur = dep.to_string();
        for _ in 0..64 {
            let o = match self.get_obligation(&cur) {
                Ok(o) => o,
                Err(_) => return Ok(false),
            };
            match o.wait_condition {
                Some(WaitCondition::Obligation { id: next }) if o.state == ObligationState::Waiting => {
                    if next == start {
                        return Ok(true);
                    }
                    cur = next;
                }
                _ => return Ok(false),
            }
        }
        Ok(true)
    }

    /// The user answered something an obligation was waiting for.
    pub fn answer_obligation(&self, id: &str, answer: &str) -> Result<Obligation> {
        let o = self.get_obligation(id)?;
        self.emit("user.answered", "obligation", id, "user", serde_json::json!({"answer": answer}))?;
        match o.state {
            ObligationState::Waiting | ObligationState::Blocked => self.transition(
                id,
                ObligationState::Accepted,
                TransitionArgs { note: format!("ответ пользователя: {answer}"), ..Default::default() },
                "user",
            ),
            _ => Ok(o),
        }
    }

    // ---------- authorization ----------

    /// Decide what to do with an action requested by an agent. Rights live here, not in prompts.
    pub fn authorize(&self, agent_id: &str, project_id: Option<&str>, action: &str) -> Result<GrantMode> {
        self.resolve_mode(agent_id, project_id, action)
    }

    pub fn decide_approval(&self, id: &str, approve: bool) -> Result<Approval> {
        let a = self.get_approval(id)?;
        self.set_approval_status(id, if approve { ApprovalStatus::Approved } else { ApprovalStatus::Denied })?;
        if approve {
            crate::tools::execute_approved(self, &a)?;
        }
        if let Some(obl) = &a.obligation_id {
            self.emit("user.answered", "obligation", obl, "user", serde_json::json!({"approval": id, "approved": approve}))?;
        }
        self.get_approval(id)
    }

    // ---------- wake-ups ----------

    /// Move waiting obligations whose condition has been satisfied back to the queue.
    pub fn wake_due(&self) -> Result<Vec<String>> {
        let now = now_ms();
        let waiting = self.list_waiting()?;
        let mut woken = vec![];
        for o in waiting {
            let Some(cond) = &o.wait_condition else { continue };
            let met = {
                let c = self.lock();
                match cond {
                    WaitCondition::Time { at } => *at <= now,
                    WaitCondition::Event { kind, subject } => {
                        let n: i64 = c.query_row(
                            "SELECT COUNT(*) FROM events WHERE kind=?1 AND ts>=?2 AND (?3 IS NULL OR subject_id=?3)",
                            params![kind, o.updated_at, subject],
                            |r| r.get(0),
                        )?;
                        n > 0
                    }
                    WaitCondition::Obligation { id } => {
                        let st: Option<String> = c
                            .query_row("SELECT state FROM obligations WHERE id=?", [id], |r| r.get(0))
                            .ok();
                        matches!(st.as_deref(), Some("done") | Some("cancelled") | None)
                    }
                    WaitCondition::User => {
                        let n: i64 = c.query_row(
                            "SELECT COUNT(*) FROM events WHERE kind='user.answered' AND subject_id=? AND ts>=?",
                            params![o.id, o.updated_at],
                            |r| r.get(0),
                        )?;
                        n > 0
                    }
                }
            };
            if met {
                self.transition(&o.id, ObligationState::Accepted, TransitionArgs { note: "условие продолжения выполнено".into(), ..Default::default() }, "system")?;
                woken.push(o.id);
            }
        }
        Ok(woken)
    }

    fn list_waiting(&self) -> Result<Vec<Obligation>> {
        Ok(self.list_obligations(None, None, true)?.into_iter().filter(|o| o.state == ObligationState::Waiting).collect())
    }

    /// Obligations that missed their deadline and are not finished — made visible, never hidden.
    pub fn overdue(&self) -> Result<Vec<Obligation>> {
        let now = now_ms();
        Ok(self.list_obligations(None, None, true)?.into_iter().filter(|o| o.due_at.map(|d| d < now).unwrap_or(false)).collect())
    }

    // ---------- scheduling ----------

    /// One pass of the environment loop: wake waiting work, turn user messages into sessions, turn
    /// ready obligations into sessions. Idempotent — an obligation never gets two live sessions
    /// (enforced by a unique index), so repeated or duplicate events cannot multiply work.
    pub fn schedule(&self, default_backend: &str) -> Result<ScheduleOutcome> {
        let mut out = ScheduleOutcome { woken: self.wake_due()?, ..Default::default() };

        // ready obligations → sessions
        let ready: Vec<Obligation> = self
            .list_obligations(None, None, true)?
            .into_iter()
            .filter(|o| o.state == ObligationState::Accepted && o.owner_id.is_some())
            .collect();
        for o in ready {
            let owner = self.get_agent(o.owner_id.as_deref().unwrap())?;
            if owner.status != "active" {
                continue;
            }
            if let Some(pid) = &o.project_id {
                if self.get_project(pid)?.status != "active" {
                    continue;
                }
            }
            if o.budget_tokens > 0 && o.spent_tokens >= o.budget_tokens {
                self.transition(
                    &o.id,
                    ObligationState::Blocked,
                    TransitionArgs { blocker: Some(format!("исчерпан бюджет ({} из {} токенов): продлить или остановить?", o.spent_tokens, o.budget_tokens)), ..Default::default() },
                    "system",
                )?;
                out.blocked_by_budget.push(o.id.clone());
                continue;
            }
            let has_live: i64 = self.lock().query_row(
                "SELECT COUNT(*) FROM sessions WHERE obligation_id=? AND status IN ('queued','running')",
                [&o.id],
                |r| r.get(0),
            )?;
            if has_live > 0 {
                continue;
            }
            let backend = if owner.model_pref.is_empty() { default_backend.to_string() } else { owner.model_pref.clone() };
            let reason = match self.events(Some("obligation"), Some(&o.id), 1)?.first() {
                Some(e) if e.kind == "obligation.created" => "принята новая задача".to_string(),
                Some(e) => format!("продолжение работы: {}", e.kind),
                None => "продолжение работы".to_string(),
            };
            let prompt = workset::build_obligation_prompt(self, &owner, &o, &reason)?;
            // The run streams into a chat of its own so the user can open it and watch the work.
            let thread = self.obligation_thread(&o, &owner)?;
            let msg = self.post_message(
                &thread.id,
                "agent",
                Some(&owner.id),
                "",
                serde_json::json!({"backend": backend, "reason": reason, "phase": "ждёт исполнителя"}),
                MessageStatus::Queued,
                None,
            )?;
            match self.create_session(&owner.id, Some(&o.id), Some(&thread.id), Some(&msg.id), &reason, &backend, &prompt) {
                Ok(s) => out.sessions.push(s),
                Err(Error::Invalid(_)) => {
                    // Someone queued it concurrently: drop the orphan message.
                    let _ = self.update_message(&msg.id, None, None, Some(MessageStatus::Cancelled));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    /// Chat that carries the runs of one obligation (created on first use).
    pub fn obligation_thread(&self, o: &Obligation, owner: &Agent) -> Result<Thread> {
        let key = format!("obl-thread:{}", o.id);
        if let Some(id) = self.setting(&key)? {
            if let Ok(t) = self.get_thread(&id) {
                return Ok(t);
            }
        }
        let company_id = match &o.project_id { Some(p) => self.company_of_project(p)?, None => None };
        let t = self.create_thread(NewThread { project_id: o.project_id.clone(), company_id, agent_id: Some(owner.id.clone()), title: o.title.clone() })?;
        self.set_setting(&key, &t.id)?;
        Ok(t)
    }

    /// The person disagrees with the specialist's plan and stopped the work. The plan is kept in the chat's
    /// state and goes to the specialist with the person's next message as an objection (see `workset`).
    /// The list comes from the UI, so it is trimmed to what a plan can be: few short items with a known status.
    pub fn hold_plan(&self, thread_id: &str, todos: &serde_json::Value) -> Result<serde_json::Value> {
        let items: Vec<serde_json::Value> = todos
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or_default()
            .iter()
            .filter_map(|t| {
                let content: String = t["content"].as_str()?.chars().filter(|c| !c.is_control()).take(300).collect();
                if content.trim().is_empty() {
                    return None;
                }
                let status = match t["status"].as_str() {
                    Some("completed") => "completed",
                    Some("in_progress") => "in_progress",
                    _ => "pending",
                };
                Some(serde_json::json!({"content": content, "status": status}))
            })
            .take(30)
            .collect();
        let hold = serde_json::json!({"todos": items, "at": crate::now_ms()});
        self.set_forum_meta(thread_id, &serde_json::json!({"plan_hold": hold.clone()}))?;
        Ok(hold)
    }

    /// The person changed their mind: the plan goes on as it was.
    pub fn release_plan(&self, thread_id: &str) -> Result<()> {
        self.set_forum_meta(thread_id, &serde_json::json!({"plan_hold": null}))?;
        Ok(())
    }

    /// The main entrance: the user writes in a chat. Posts the message, picks the addressees,
    /// queues one session per addressee and returns everything the UI needs to show immediately.
    pub fn send_chat(&self, req: ChatSend, default_backend: &str) -> Result<Dispatched> {
        let thread = self.get_thread(&req.thread_id)?;
        let mut props = serde_json::json!({});
        if !req.attachments.is_empty() {
            props["attachments"] = serde_json::json!(req.attachments);
        }
        if !req.context.is_empty() {
            props["context"] = serde_json::json!(req.context);
        }
        if let Some(r) = &req.reply_to {
            if let Ok(m) = self.get_message(r) {
                props["replyTo"] = serde_json::json!({"id": m.id, "actor": m.author_id.clone().unwrap_or_else(|| m.author_kind.clone()), "text": m.body.chars().take(160).collect::<String>()});
            }
        }
        let user_msg = self.post_message(&thread.id, "user", None, &req.text, props, MessageStatus::Done, None)?;
        self.auto_title(&thread.id, &req.text)?;

        // The person named a company or a project: the specialist should not have to ask where to look.
        // An unattached chat is attached to it (only when it is unambiguous).
        let mentions = self.mentions(&req.text)?;
        if thread.kind == ThreadKind::Chat && thread.project_id.is_none() && thread.company_id.is_none() {
            let projects: Vec<&Mention> = mentions.iter().filter(|m| m.kind == "project").collect();
            let companies: Vec<&Mention> = mentions.iter().filter(|m| m.kind == "company").collect();
            let target = if projects.len() == 1 && companies.iter().all(|c| projects[0].company_id.as_deref() == Some(&c.id)) {
                Some((Some(projects[0].id.clone()), projects[0].company_id.clone()))
            } else if projects.is_empty() && companies.len() == 1 {
                Some((None, Some(companies[0].id.clone())))
            } else {
                None
            };
            if let Some((p, c)) = target {
                self.move_thread(&thread.id, p.as_deref(), c.as_deref())?;
                self.post_message(&thread.id, "system", None, &format!("Чат привязан: {}", match (&p, &c) {
                    (Some(pid), _) => format!("проект «{}»", self.get_project(pid)?.name),
                    (None, Some(cid)) => format!("компания «{}»", self.get_company(cid)?.name),
                    _ => String::new(),
                }), serde_json::json!({}), MessageStatus::Done, None)?;
            }
        }
        let thread = self.get_thread(&thread.id)?;

        // Per-send overrides become the chat's settings, so the next message keeps them.
        // The model is not a chat's setting: it is the specialist's own, set in Specialists. (A model left on a chat
        // by older versions, or sent by an old client, is ignored.)
        let patch = crate::store::ThreadPatch { effort: req.effort.clone(), mode: req.mode.clone(), ..Default::default() };
        let thread = if patch.effort.is_some() || patch.mode.is_some() { self.patch_thread(&thread.id, patch)? } else { thread };

        // A forum is run by the forum orchestrator: a human message joins the discussion, it does not
        // start sessions by itself.
        if thread.kind == ThreadKind::Forum {
            return Ok(Dispatched { user_message: user_msg, agent_messages: vec![], sessions: vec![] });
        }
        let pool = match &thread.project_id {
            Some(p) => {
                let m = self.project_members(p)?;
                if m.is_empty() { self.list_agents(false)? } else { m }
            }
            None => self.list_agents(false)?,
        };
        // Naming someone beats the chat's own specialist: with a chat pinned to the designer, "@разработчик …"
        // is for the developer and does not wait in the designer's line.
        let named = workset::mentioned(&req.text, &self.list_agents(false)?);
        let targets: Vec<Agent> = if !named.is_empty() {
            named.clone()
        } else {
            match &thread.agent_id {
                Some(a) => vec![self.get_agent(a)?],
                None => workset::route(&req.text, &pool),
            }
        };
        // `/compact`: whoever worked in this chat writes a summary of it (see `workset::compact_prompt`); the node then
        // starts their next turn in a fresh session that is given the summary.
        // `/compact-all`: everyone who answered here and whose context is over a fifth full compacts at once.
        let all = compact_all_arg(&req.text).map(String::from);
        let compact = compact_arg(&req.text).map(String::from).or(all.clone());
        let mut skipped: Vec<String> = vec![];
        let targets = if all.is_some() {
            let mut seen = std::collections::HashSet::new();
            let mut who = vec![];
            for m in self.list_messages(&thread.id, None, 300)?.iter().rev() {
                if m.author_kind != "agent" || m.props.get("call").is_some() || m.status != MessageStatus::Done {
                    continue;
                }
                let Some(id) = m.author_id.clone() else { continue };
                if !seen.insert(id.clone()) {
                    continue; // only each one's latest answer tells how full they are now
                }
                let Ok(ag) = self.get_agent(&id) else { continue };
                let (used, window) = (m.props["work"]["context_tokens"].as_f64().unwrap_or(0.0), m.props["work"]["context_window"].as_f64().unwrap_or(0.0));
                let share = if window > 0.0 { used / window } else { 0.0 };
                if share > 0.2 {
                    who.push(ag);
                } else {
                    skipped.push(format!("{} — {}", ag.name, if window > 0.0 { format!("{}%", (share * 100.0).round()) } else { "неизвестно".into() }));
                }
            }
            if who.is_empty() {
                let text = if skipped.is_empty() { "Сжимать нечего: в этом чате ещё никто не отвечал.".to_string() } else { format!("Сжимать некого: контекст у всех не больше 20% ({}).", skipped.join(", ")) };
                let m = self.post_message(&thread.id, "system", None, &text, serde_json::json!({}), MessageStatus::Done, None)?;
                return Ok(Dispatched { user_message: user_msg, agent_messages: vec![m], sessions: vec![] });
            }
            who
        } else if compact.is_some() && named.is_empty() && thread.agent_id.is_none() {
            let mut seen = std::collections::HashSet::new();
            let who: Vec<Agent> = self
                .list_messages(&thread.id, None, 300)?
                .iter()
                .rev()
                .filter(|m| m.author_kind == "agent" && m.props.get("call").is_none())
                .filter_map(|m| m.author_id.clone())
                .filter(|id| seen.insert(id.clone()))
                .filter_map(|id| self.get_agent(&id).ok())
                .collect();
            who
        } else {
            targets
        };
        let mut d = Dispatched { user_message: user_msg, agent_messages: vec![], sessions: vec![] };
        if all.is_some() && !skipped.is_empty() {
            // the person sees who was left as is, and why
            d.agent_messages.push(self.post_message(&thread.id, "system", None, &format!("Не сжимаются, контекст не больше 20%: {}.", skipped.join(", ")), serde_json::json!({}), MessageStatus::Done, None)?);
        }
        if targets.is_empty() && compact.is_some() {
            let m = self.post_message(&thread.id, "system", None, "Сжимать нечего: в этом чате ещё никто не отвечал.", serde_json::json!({}), MessageStatus::Done, None)?;
            d.agent_messages.push(m);
            return Ok(d);
        }
        if targets.is_empty() {
            let m = self.post_message(&thread.id, "system", None, "В Senate пока нет специалистов — создайте хотя бы одного.", serde_json::json!({}), MessageStatus::Done, None)?;
            d.agent_messages.push(m);
            return Ok(d);
        }
        for agent in targets {
            // Everyone works on their own model, whichever chat they are in and whoever called them.
            let backend = if !agent.model_pref.is_empty() { agent.model_pref.clone() } else { default_backend.to_string() };
            let mut prompt = match &compact {
                Some(extra) => workset::compact_prompt(extra),
                None => workset::chat_turn_prompt(self, &agent, &thread, &req.text)?,
            };
            if compact.is_none() {
                if req.text.trim().is_empty() {
                    prompt += "(сообщение без текста — смотри вложения)";
                }
                prompt += &workset::attachments_block(&req.attachments);
            }
            let msg = self.post_message(
                &thread.id,
                "agent",
                Some(&agent.id),
                "",
                serde_json::json!({"backend": backend, "effort": thread.effort, "phase": "в очереди", "steps": [], "compact": compact.is_some()}),
                MessageStatus::Queued,
                None,
            )?;
            let s = self.create_session(&agent.id, None, Some(&thread.id), Some(&msg.id), "сообщение пользователя", &backend, &prompt)?;
            d.agent_messages.push(msg);
            d.sessions.push(s);
        }
        // An objection to the plan is delivered once, with this message: it does not stick to later ones.
        if thread.meta.get("plan_hold").is_some() {
            self.set_forum_meta(&thread.id, &serde_json::json!({"plan_hold": null}))?;
        }
        Ok(d)
    }

    /// A specialist calls a colleague into this chat: their request is posted, the colleague gets a turn of their own,
    /// and when they have answered the answer is handed back to the caller (see `return_call`). A chain of calls is
    /// short on purpose: nobody keeps two specialists busy talking to each other forever.
    pub fn call_colleague(&self, caller: &Agent, thread_id: &str, to: &str, request: &str, depth: i64, default_backend: &str) -> Result<serde_json::Value> {
        const MAX_DEPTH: i64 = 3;
        let thread = self.get_thread(thread_id)?;
        if thread.kind == ThreadKind::Forum {
            return Err(Error::invalid("в форуме коллег не зовут: там обсуждают все участники"));
        }
        let target = self.get_agent(to)?;
        if target.id == caller.id {
            return Err(Error::invalid("позвать можно только коллегу, не себя"));
        }
        if target.status != "active" {
            return Err(Error::invalid(format!("{} сейчас не работает", target.name)));
        }
        if request.trim().is_empty() {
            return Err(Error::invalid("опиши, что нужно от коллеги: он не видит твоих мыслей"));
        }
        if depth >= MAX_DEPTH {
            return Err(Error::invalid(format!(
                "НЕ ДОСТАВЛЕНО: цепочка вызовов уже длинная ({MAX_DEPTH}), {} этот запрос не получил и нового хода не будет. Важное предупреждение отправь записку senate_message_colleague — она доставляется всегда; иначе доведи работу сам или спроси человека. Не говори, что передал.",
                target.name
            )));
        }
        // the same request to the same colleague, still waiting or in work: one turn, not a pile of identical ones
        let pending: Option<(String, Option<String>)> = self
            .lock()
            .query_row(
                "SELECT id, session_id FROM messages WHERE thread_id=? AND author_id=? AND status IN ('queued','working')
                 AND json_extract(props,'$.asked_by')=? AND json_extract(props,'$.request')=? ORDER BY created_at DESC LIMIT 1",
                rusqlite::params![thread.id, target.id, caller.id, request.trim()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((mid, sid)) = pending {
            return Ok(serde_json::json!({"status": "already_queued", "to": target.slug, "message_id": mid, "session_id": sid, "note": "этот запрос коллеге уже в работе — новый не отправлен; ответ придёт как обычно"}));
        }
        let call_msg = self.post_message(&thread.id, "agent", Some(&caller.id), &format!("→ @{}: {}", target.slug, request.trim()), serde_json::json!({"call": {"to": target.id}}), MessageStatus::Done, None)?;
        let backend = if !target.model_pref.is_empty() { target.model_pref.clone() } else { default_backend.to_string() };
        let asked = format!(
            "<senate-call>\nТебя позвал в этот разговор коллега {} (@{}). Он продолжает свою работу и ждёт твой ответ. Ответь ему по делу и коротко; если нужно что-то сделать в твоей зоне — сделай и сообщи результат.\n</senate-call>\n\n{}",
            caller.name, caller.slug, request.trim()
        );
        let mut prompt = workset::chat_turn_prompt(self, &target, &thread, &asked)?;
        prompt += &workset::attachments_block(&[]);
        let msg = self.post_message(
            &thread.id,
            "agent",
            Some(&target.id),
            "",
            serde_json::json!({"backend": backend, "effort": thread.effort, "phase": "в очереди", "steps": [], "asked_by": caller.id, "depth": depth + 1, "request": request.trim()}),
            MessageStatus::Queued,
            None,
        )?;
        let s = self.create_session(&target.id, None, Some(&thread.id), Some(&msg.id), &format!("позвал {}", caller.name), &backend, &prompt)?;
        // the call knows its answer: the chat shows the answer as a comment under the call
        self.update_message(&call_msg.id, None, Some(&serde_json::json!({"call": {"to": target.id, "answer": msg.id}})), None)?;
        Ok(serde_json::json!({"status": "queued", "to": target.slug, "message_id": msg.id, "session_id": s.id}))
    }

    /// The colleague has answered: the caller gets the answer as a turn of their own and goes on with their work.
    pub fn return_call(&self, thread_id: &str, asker_id: &str, colleague: &Agent, reply: &str, depth: i64, default_backend: &str) -> Result<Option<Session>> {
        let thread = self.get_thread(thread_id)?;
        let asker = self.get_agent(asker_id)?;
        if asker.status != "active" {
            return Ok(None);
        }
        let reply: String = reply.trim().chars().take(8000).collect();
        let text = format!(
            "<senate-call>\nКоллега {} (@{}) ответил на твой запрос. Продолжи работу над исходной задачей с учётом ответа; если вопрос закрыт и дела больше нет — коротко скажи человеку итог.\n</senate-call>\n\n{}",
            colleague.name, colleague.slug, if reply.is_empty() { "(ответа нет)".to_string() } else { reply }
        );
        let backend = if !asker.model_pref.is_empty() { asker.model_pref.clone() } else { default_backend.to_string() };
        let prompt = workset::chat_turn_prompt(self, &asker, &thread, &text)?;
        let msg = self.post_message(
            &thread.id,
            "agent",
            Some(&asker.id),
            "",
            serde_json::json!({"backend": backend, "effort": thread.effort, "phase": "в очереди", "steps": [], "reply_from": colleague.id, "depth": depth}),
            MessageStatus::Queued,
            None,
        )?;
        Ok(Some(self.create_session(&asker.id, None, Some(&thread.id), Some(&msg.id), &format!("ответил {}", colleague.name), &backend, &prompt)?))
    }

    /// Called when a node claims a session.
    pub fn on_session_started(&self, s: &Session) -> Result<()> {
        if let Some(oid) = &s.obligation_id {
            let o = self.get_obligation(oid)?;
            if o.state == ObligationState::Accepted {
                self.transition(oid, ObligationState::Running, TransitionArgs { note: format!("сессия {}", &s.id[..8]), ..Default::default() }, &s.agent_id)?;
            }
        }
        Ok(())
    }

    /// Snapshot for the home screen: what the user must decide, what is going on, what is stuck.
    pub fn attention(&self) -> Result<serde_json::Value> {
        let approvals = self.list_approvals(true)?;
        let obligations = self.list_obligations(None, None, true)?;
        let needs_user: Vec<&Obligation> = obligations
            .iter()
            .filter(|o| {
                o.state == ObligationState::Blocked
                    || o.state == ObligationState::Review
                    || (o.state == ObligationState::Waiting && matches!(o.wait_condition, Some(WaitCondition::User)))
            })
            .collect();
        let running: Vec<&Obligation> = obligations.iter().filter(|o| o.state == ObligationState::Running).collect();
        let overdue = self.overdue()?;
        let queued = self.list_sessions(None, None, Some(SessionStatus::Queued), 50)?;
        let proposed_decisions: Vec<Decision> = {
            let mut v = vec![];
            for p in self.list_projects()? {
                v.extend(self.list_decisions(&p.id)?.into_iter().filter(|d| d.status == DecisionStatus::Proposed));
            }
            v
        };
        Ok(serde_json::json!({
            "approvals": approvals,
            "needs_user": needs_user,
            "running": running,
            "overdue": overdue,
            "queued_sessions": queued.len(),
            "proposed_decisions": proposed_decisions,
        }))
    }
}

// ---------- first run ----------

impl Store {
    /// A small starting collective so the first screen is not empty. Called once.
    pub fn seed_defaults(&self) -> Result<()> {
        self.seed_presets()?;
        if self.setting("seeded")?.is_some() {
            return Ok(());
        }
        if self.list_agents(true)?.is_empty() {
            let team = [
                ("Архитектор", "architect", "#5e5ce6",
                 "Работаю с системами, зависимостями и последствиями решений. Прежде чем предлагать, выясняю ограничения и цену отката. Не пишу код сам, если это не нужно для проверки идеи. Критерий качества: решение объяснимо, обратимо там, где это дёшево, и записано."),
                ("Разработчик", "developer", "#0a84ff",
                 "Работаю с реализацией, корректностью и поддерживаемостью. Меняю минимум нужного, проверяю запуском, сообщаю о том, что не проверил. Критерий качества: работает, читается, не ломает соседнее."),
                ("Дизайнер", "designer", "#ff9f0a",
                 "Работаю с восприятием и взаимодействием. Начинаю с задачи человека, а не с экрана. Критерий качества: понятно с первого взгляда, состояния продуманы, ничего не держится только на цвете."),
                ("Ассистент", "assistant", "#64d2ff",
                 "Помогаю с повседневным: коротко отвечаю, собираю справки, напоминаю, раскладываю дела по полочкам. Не берусь за сложное — передаю профильному специалисту и объясняю, кому и почему. Критерий качества: быстро, по делу, без выдумок."),
                ("Аналитик", "analyst", "#30d158",
                 "Работаю с данными, гипотезами и достоверностью. Отделяю наблюдение от вывода, называю источник и ограничения выборки. Критерий качества: вывод можно перепроверить."),
            ];
            for (name, role, accent, profile) in team {
                self.create_agent(NewAgent { name: name.into(), role: role.into(), profile: profile.into(), accent: accent.into(), model_pref: String::new() })?;
            }
        }
        self.set_setting("seeded", "1")?;
        self.seed_presets()
    }

    #[allow(dead_code)]
    fn _seed_tail(&self) -> Result<()> {
        Ok(())
    }
}

impl Store {
    /// Presets that exist from the first start of every database, once.
    fn seed_presets(&self) -> Result<()> {
        // The starting collective may not exist yet on a brand-new database: presets are re-checked
        // after it is created (see end of `seed_defaults`).
        if self.list_agents(false)?.is_empty() {
            return Ok(());
        }

        // Apple Developer documentation MCP: wanted by the user from day one; read-only, so trusted.
        // Given to those who build and design for Apple platforms.
        if self.setting("seed:apple-developer")?.is_none() {
            let agents = self.list_agents(false)?;
            let holders: Vec<String> = agents.iter().filter(|a| a.role == "developer" || a.role == "designer").map(|a| a.id.clone()).collect();
            if self.get_mcp("apple-developer").is_err() {
                self.save_mcp(
                    None,
                    McpInput {
                        name: Some("apple-developer".into()),
                        kind: Some("stdio".into()),
                        command: Some("npx".into()),
                        args: Some(vec!["-y".into(), "@kimsungwhee/apple-docs-mcp".into()]),
                        note: Some("Документация Apple Developer: API, Human Interface Guidelines, WWDC, примеры кода".into()),
                        trusted: Some(true),
                        agents: Some(holders),
                        ..Default::default()
                    },
                )?;
            }
            self.set_setting("seed:apple-developer", "1")?;
        }
        // The assistant is the person's hands for task housekeeping ("close everything that is done"): it may act on
        // any task of the chat's project without asking each time. Once; the person can change it in the grants.
        if self.setting("seed:assistant-manage")?.is_none() {
            for a in self.list_agents(true)?.into_iter().filter(|a| a.role == "assistant") {
                self.set_grant(Some(&a.id), None, action::OBLIGATION_MANAGE, GrantMode::Auto)?;
            }
            self.set_setting("seed:assistant-manage", "1")?;
        }
        // An everyday assistant on a light model (the owner's example: Haiku for the assistant), once per database.
        if self.setting("seed:assistant")?.is_none() {
            let existing = self.list_agents(true)?.into_iter().find(|a| a.role == "assistant");
            match existing {
                Some(a) if a.model_pref.is_empty() => {
                    self.update_agent_basics(&a.id, None, None, Some("claude:haiku"))?;
                }
                Some(_) => {}
                None => {
                    self.create_agent(NewAgent {
                        name: "Ассистент".into(),
                        role: "assistant".into(),
                        profile: "Помогаю с повседневным: коротко отвечаю, собираю справки, раскладываю дела по полочкам. Не берусь за сложное — передаю профильному специалисту и объясняю, кому и почему. Критерий качества: быстро, по делу, без выдумок.".into(),
                        accent: "#64d2ff".into(),
                        model_pref: "claude:haiku".into(),
                    })?;
                }
            }
            self.set_setting("seed:assistant", "1")?;
        }
        Ok(())
    }
}

/// `/compact-all` or `/compact-all <what to keep in mind>`: the argument, if the text is that command.
pub fn compact_all_arg(text: &str) -> Option<&str> {
    let rest = text.trim().strip_prefix("/compact-all")?;
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        Some(rest.trim())
    } else {
        None
    }
}

/// `/compact` or `/compact <what to keep in mind>`: the argument, if the text is that command.
pub fn compact_arg(text: &str) -> Option<&str> {
    let rest = text.trim().strip_prefix("/compact")?;
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        Some(rest.trim())
    } else {
        None
    }
}
