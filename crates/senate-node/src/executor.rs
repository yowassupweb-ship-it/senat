//! Executes sessions on this node: claims queued work, runs the backend, streams progress into the
//! chat message, applies the end state to the environment. State lives in the store, never only in
//! this process — a restart requeues what was running.

use crate::{App, Pending};
use senate_core::engine::TransitionArgs;
use senate_core::model::*;
use senate_core::tools::ToolCtx;
use senate_runtime::{Approver, Decision, Event, ExtraMcp, McpSpec, PermissionRequest, Progress, RunSpec};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch, Semaphore};

/// After a restart: whatever was `running` has no executor. Requeue it and say so in the chat.
pub(crate) fn recover(app: &Arc<App>) {
    let n = app.store.requeue_orphaned_sessions().unwrap_or(0);
    if n == 0 {
        return;
    }
    if let Ok(queued) = app.store.list_sessions(None, None, Some(SessionStatus::Queued), 200) {
        for s in queued {
            if let Some(m) = &s.message_id {
                let _ = app.store.update_message(m, None, Some(&json!({"phase": "перезапуск — продолжу"})), Some(MessageStatus::Queued));
            }
        }
    }
}

pub(crate) async fn run_loop(app: Arc<App>, mut stop: watch::Receiver<bool>) {
    let sem = Arc::new(Semaphore::new(app.cfg.parallel.max(1)));
    crate::skills::sync_routines(&app);
    let mut ticks: u32 = 0;
    loop {
        tokio::select! {
            _ = app.wake.notified() => {}
            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            _ = stop.changed() => { if *stop.borrow() { break } }
        }
        if *stop.borrow() {
            break;
        }
        // skills may appear while the app runs: look for their routines about once a minute
        ticks = ticks.wrapping_add(1);
        if ticks % 30 == 0 {
            crate::skills::sync_routines(&app);
        }
        // work on a schedule: whatever is due becomes a task, and the scheduler below picks it up
        match app.store.fire_due_routines(senate_core::now_ms(), crate::skills::offset_secs()) {
            Ok(fired) if !fired.is_empty() => {
                app.publish("routines", json!({}));
                app.publish("attention", json!({}));
            }
            Err(e) => tracing::warn!("регулярные задачи: {e}"),
            _ => {}
        }
        match app.store.schedule(&app.backend_default()) {
            Ok(out) => {
                for s in &out.sessions {
                    if let Some(m) = s.message_id.as_ref().and_then(|m| app.store.get_message(m).ok()) {
                        app.publish("message", serde_json::to_value(&m).unwrap_or_default());
                    }
                }
                if !out.sessions.is_empty() || !out.woken.is_empty() || !out.blocked_by_budget.is_empty() {
                    app.publish("attention", json!({}));
                }
            }
            Err(e) => tracing::warn!("планировщик: {e}"),
        }
        while let Ok(permit) = sem.clone().try_acquire_owned() {
            match app.store.claim_session(&app.node_id, &|_| true) {
                Ok(Some(s)) => {
                    let a = app.clone();
                    tokio::spawn(async move {
                        execute(a, s).await;
                        drop(permit);
                    });
                }
                _ => break,
            }
        }
    }
}

struct NodeApprover {
    app: Arc<App>,
    message_id: String,
    /// The run's folder: relative paths in a question are relative to it.
    cwd: std::path::PathBuf,
}

impl Approver for NodeApprover {
    fn ask(&self, req: PermissionRequest) -> Pin<Box<dyn Future<Output = Decision> + Send + '_>> {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            let rule = crate::permissions::candidate(&req, &self.cwd);
            let dir = crate::permissions::dir_candidate(&req, &self.cwd);
            self.app.pending.lock().unwrap().insert(self.message_id.clone(), Pending { tx, tool_use_id: req.tool_use_id, rule, dir });
            rx.await.unwrap_or(Decision::Deny("Ожидание разрешения прервано.".into()))
        })
    }

    fn preapproved(&self, req: &PermissionRequest) -> bool {
        let rules = self.app.store.list_permission_rules().unwrap_or_default();
        crate::permissions::allowed(&rules, req, &self.cwd)
    }
}

fn parse_mode(thread: Option<&Thread>, project: Option<&Project>) -> PermissionMode {
    let pick = thread.map(|t| t.mode.as_str()).filter(|m| !m.is_empty()).or_else(|| project.map(|p| p.mode.as_str()));
    pick.and_then(PermissionMode::parse).unwrap_or(PermissionMode::Edit)
}

fn progress_props(p: &Progress) -> Value {
    let mut m = serde_json::Map::new();
    if let Some(v) = &p.phase {
        m.insert("phase".into(), json!(v));
    }
    if let Some(v) = &p.steps {
        m.insert("steps".into(), serde_json::to_value(v).unwrap_or_default());
    }
    if let Some(v) = &p.model {
        m.insert("model".into(), json!(v));
    }
    if let Some(v) = &p.rate_limit {
        m.insert("rateLimit".into(), v.clone());
    }
    if let Some(v) = &p.reasoning {
        m.insert("reasoning".into(), json!([v]));
    }
    Value::Object(m)
}

fn merge(dst: &mut Progress, src: Progress) {
    if src.phase.is_some() {
        dst.phase = src.phase;
    }
    if src.body.is_some() {
        dst.body = src.body;
    }
    if src.steps.is_some() {
        dst.steps = src.steps;
    }
    if src.model.is_some() {
        dst.model = src.model;
    }
    if src.session_id.is_some() {
        dst.session_id = src.session_id;
    }
    if src.rate_limit.is_some() {
        dst.rate_limit = src.rate_limit;
    }
    if src.reasoning.is_some() {
        dst.reasoning = src.reasoning;
    }
    if src.limits.is_some() {
        dst.limits = src.limits;
    }
}

fn flush(app: &App, msg_id: &str, p: &mut Progress) {
    let props = progress_props(p);
    let body = p.body.take();
    p.phase = None;
    p.steps = None;
    p.model = None;
    p.rate_limit = None;
    p.reasoning = None;
    let has_props = props.as_object().map(|o| !o.is_empty()).unwrap_or(false);
    if body.is_none() && !has_props {
        return;
    }
    if let Ok(m) = app.store.update_message(msg_id, body.as_deref(), if has_props { Some(&props) } else { None }, None) {
        app.publish("message", serde_json::to_value(&m).unwrap_or_default());
    }
}

pub(crate) async fn execute(app: Arc<App>, s: Session) {
    let store = &app.store;
    let Ok(agent) = store.get_agent(&s.agent_id) else {
        let _ = store.finish_session(&s.id, SessionStatus::Failed, "", "", "специалист не найден", 0, 0);
        return;
    };
    let thread = s.thread_id.as_deref().and_then(|t| store.get_thread(t).ok());
    let project = thread.as_ref().and_then(|t| t.project_id.as_deref()).and_then(|p| store.get_project(p).ok());
    let msg_id = s.message_id.clone().unwrap_or_default();
    let _ = store.on_session_started(&s);
    if let Ok(m) = store.update_message(&msg_id, None, Some(&json!({"phase": "запускается", "agent": agent.id, "sessionId": s.id})), Some(MessageStatus::Working)) {
        app.publish("message", serde_json::to_value(&m).unwrap_or_default());
    }
    app.publish("attention", json!({}));

    let (cancel_tx, cancel_rx) = watch::channel(false);
    app.cancels.lock().unwrap().insert(s.id.clone(), cancel_tx);
    let (backend_now, fallback_note) = app.resolve_backend(&s.backend);
    if let Some(note) = &fallback_note {
        let _ = store.update_message(&msg_id, None, Some(&json!({"backend": backend_now, "fallback": note})), None);
    }
    let s = Session { backend: backend_now, ..s };

    // Working directory: the project's folder, or a private workspace of the specialist.
    let cwd = match project.as_ref().map(|p| p.root_path.as_str()).filter(|r| !r.is_empty()) {
        Some(r) => std::path::PathBuf::from(r),
        None => {
            let d = app.cfg.data_dir.join("workspaces").join(&agent.slug);
            let _ = std::fs::create_dir_all(&d);
            d
        }
    };

    let is_chat = s.obligation_id.is_none();
    let provider = s.backend.split(':').next().unwrap_or("").to_string();
    // Where the turn runs. A project whose folder is on one of the person's computers is worked on there, by whoever
    // works on it: the folder, the dev server and the tools are there. Otherwise a specialist set to "local" works
    // on the computer connected last, and everyone else on the node.
    let project_machine = project.as_ref().filter(|p| !p.root_path.is_empty()).and_then(|p| p.node_id.clone()).filter(|m| !m.is_empty() && *m != app.machine_id);
    let local = project_machine.is_some() || store.setting(&format!("exec:{}", agent.id)).ok().flatten().as_deref() == Some("local");
    // The computer the turn goes to is fixed now, so the session continued is that computer's own.
    let project_machine = if local && project_machine.is_none() { app.runner.latest_machine() } else { project_machine };
    // CLI sessions live on the computer that ran them: a session from one computer means nothing on another.
    let session_key = match (&project_machine, local) {
        (Some(m), _) => format!("{provider}@{m}"),
        (None, true) => format!("{provider}@local"),
        _ => provider.clone(),
    };
    let is_forum_thread = thread.as_ref().map(|t| t.kind == ThreadKind::Forum).unwrap_or(false);
    // One CLI session per specialist in a chat, not per chat: two specialists sharing one would resume the same
    // session (a CLI lets only one process write to it) and would wear each other's identity and history.
    // Chats from before keep their single session for the specialist the chat is pinned to.
    let resume = if is_chat && !is_forum_thread { thread.as_ref().and_then(|t| store.cli_session_for(t, &session_key, &agent.id)) } else { None };
    let company_id = thread.as_ref().and_then(|t| t.company_id.clone()).or_else(|| project.as_ref().and_then(|p| p.company_id.clone()));
    let mut system_prompt = senate_core::workset::system_prompt(store, &agent, project.as_ref(), company_id.as_deref()).unwrap_or_default();
    system_prompt += &crate::skills::block_for(&app.cfg.data_dir, &agent.slug);
    if !local {
        // on a computer its app adds this itself; on the node, the node does
        let env = tokio::task::spawn_blocking(crate::workspace::environment).await.unwrap_or_default();
        system_prompt += &crate::workspace::environment_block("сервер (нода Senate)", &cwd, &env);
        if app.cfg.bind.ip().is_unspecified() {
            system_prompt += "Это сервер, а не компьютер человека: его localhost человеку напрямую не открыть. Запустил страницу или dev-сервер — покажи через `senate_preview` (http://localhost:ПОРТ): кнопка откроет её у человека через Senate. Не проси человека открыть localhost и не говори, что он «на сервере недоступен». Папки коллег рядом с твоей: ../<username>.\n";
        }
    }

    let token = format!("sst_{}", uuid::Uuid::new_v4().simple());
    app.session_tokens.lock().unwrap().insert(
        token.clone(),
        ToolCtx {
            default_backend: app.backend_default(),
            agent_id: agent.id.clone(),
            session_id: Some(s.id.clone()),
            obligation_id: s.obligation_id.clone(),
            thread_id: s.thread_id.clone(),
            project_id: project.as_ref().map(|p| p.id.clone()),
            company_id: company_id.clone(),
            // a node that serves other machines (bound to all interfaces: the server) is not the person's computer
            on_node: !local && app.cfg.bind.ip().is_unspecified(),
        },
    );
    let mcp_bin = std::env::var_os("SENATE_MCP_BIN").map(std::path::PathBuf::from).or_else(|| std::env::current_exe().ok());
    let mcp = mcp_bin.map(|exe| McpSpec {
        command: exe.to_string_lossy().into_owned(),
        args: vec!["mcp".into()],
        env: vec![("SENATE_HUB".into(), app.url()), ("SENATE_SESSION_TOKEN".into(), token.clone())],
    });

    // After `/compact` a fresh session knows the chat only through the specialist's own summary of it.
    let compact_turn = store.get_message(&msg_id).map(|m| m.props["compact"].as_bool().unwrap_or(false)).unwrap_or(false);
    let mut turn_prompt = s.prompt.clone();
    // notes colleagues left while this specialist was away come with the turn
    if let Ok(notes) = store.take_notes(&agent.id) {
        if !notes.is_empty() {
            turn_prompt = crate::notes::block(&notes) + &turn_prompt;
        }
    }
    if resume.is_none() && !compact_turn {
        if let Some(sum) = thread.as_ref().and_then(|t| senate_core::store::Store::compact_summary(t, &agent.id)) {
            turn_prompt = senate_core::workset::compact_preamble(&sum) + &turn_prompt;
        }
    }

    // the chat's choice, else the specialist's own default (set on their sheet)
    let effort = thread.as_ref().map(|t| t.effort.clone()).filter(|e| !e.is_empty()).or_else(|| store.setting(&format!("effort:{}", agent.id)).ok().flatten().filter(|e| !e.is_empty()));
    let cwd_for_rules = cwd.clone();
    let spec = RunSpec {
        backend: s.backend.clone(),
        effort,
        mode: parse_mode(thread.as_ref(), project.as_ref()),
        cwd,
        system_prompt,
        prompt: turn_prompt,
        resume,
        mcp,
        // The specialist's own equipment: only the MCP servers assigned to them (or shared).
        // On the node, servers set up for the person's computer (a Windows path) are left out; on a computer, its
        // app leaves out the node's own (`runner::usable_here` either way).
        extra_mcp: store.mcp_for_agent(&agent.id).unwrap_or_default().iter().map(ExtraMcp::from).filter(|m| local || crate::runner::usable_here(m))
            .map(|mut m| {
                if !local {
                    m.args = m.args.iter().map(|a| a.replace("{slug}", &agent.slug)).collect();
                }
                m
            })
            .collect(),
        extra_dirs: crate::permissions::allowed_dirs(&store.list_permission_rules().unwrap_or_default()),
        env: vec![],
        bare: false,
        work_dir: app.work_dir(),
        run_id: s.id[..16.min(s.id.len())].to_string(),
    };

    let (tx, mut rx) = mpsc::unbounded_channel::<Event>();
    let approver = NodeApprover { app: app.clone(), message_id: msg_id.clone(), cwd: cwd_for_rules.clone() };

    // Consumer: coalesces fast progress into ~8 updates a second.
    let consumer = {
        let app = app.clone();
        let msg_id = msg_id.clone();
        let provider_c = provider.clone();
        let cwd_c = cwd_for_rules.clone();
        tokio::spawn(async move {
            let mut acc = Progress::default();
            let mut sid: Option<String> = None;
            let mut tick = tokio::time::interval(Duration::from_millis(120));
            loop {
                tokio::select! {
                    ev = rx.recv() => match ev {
                        None => break,
                        Some(Event::Progress(p)) => {
                            if p.session_id.is_some() { sid = p.session_id.clone(); }
                            if let Some(w) = &p.limits { crate::accounts::note_limits(&app, &provider_c, w); }
                            merge(&mut acc, *p);
                        }
                        Some(Event::Ask(req)) => {
                            flush(&app, &msg_id, &mut acc);
                            let always = crate::permissions::candidate(&req, &cwd_c).map(|c| c.label);
                            let dir = crate::permissions::dir_candidate(&req, &cwd_c).map(|c| json!({"label": c.label, "path": c.value}));
                            let ask = json!({"kind": "permission", "toolUseId": req.tool_use_id, "tool": req.tool, "input": req.input, "question": req.reason, "state": "pending", "always": always, "dir": dir});
                            if let Ok(m) = app.store.update_message(&msg_id, None, Some(&json!({"ask": ask})), None) {
                                app.publish("message", serde_json::to_value(&m).unwrap_or_default());
                            }
                            app.publish("attention", json!({}));
                        }
                        Some(Event::Answered { tool_use_id, allowed }) => {
                            let ask = json!({"kind": "permission", "toolUseId": tool_use_id, "state": if allowed { "allowed" } else { "denied" }});
                            if let Ok(m) = app.store.update_message(&msg_id, None, Some(&json!({"ask": ask})), None) {
                                app.publish("message", serde_json::to_value(&m).unwrap_or_default());
                            }
                            app.publish("attention", json!({}));
                        }
                    },
                    _ = tick.tick() => flush(&app, &msg_id, &mut acc),
                }
            }
            flush(&app, &msg_id, &mut acc);
            sid
        })
    };

    let outcome = if local {
        crate::runner::remote_run(&app, spec, &agent.slug, project_machine.as_deref(), &approver, tx, cancel_rx).await
    } else {
        senate_runtime::run(spec, &approver, tx, cancel_rx).await
    };
    let sid = consumer.await.ok().flatten();

    // ---- finalize ----
    app.cancels.lock().unwrap().remove(&s.id);
    app.session_tokens.lock().unwrap().remove(&token);
    app.pending.lock().unwrap().remove(&msg_id);

    let status = if outcome.stopped {
        MessageStatus::Cancelled
    } else if outcome.failure.is_some() {
        MessageStatus::Failed
    } else {
        MessageStatus::Done
    };
    let mut props = json!({
        "steps": outcome.steps, "work": outcome.work, "model": outcome.model, "phase": null,
        "ask": null, "backend": s.backend,
    });
    if let Some(f) = &outcome.failure {
        props["error"] = json!(f);
    }
    if let Ok(m) = store.update_message(&msg_id, Some(&outcome.text), Some(&props), Some(status)) {
        app.publish("message", serde_json::to_value(&m).unwrap_or_default());
    }
    // A colleague who was called has answered (or could not): the answer goes back to whoever called them.
    if status != MessageStatus::Cancelled {
        if let (Ok(m), Some(tid)) = (store.get_message(&msg_id), s.thread_id.as_deref()) {
            // asked with senate_ask_colleague and the asker is still waiting: the answer goes to them inline, not as a turn
            let waited_inline = m.props["inline_wait"] == true && m.props["inline_gave_up"] != true;
            if let (Some(asker), false) = (m.props["asked_by"].as_str(), waited_inline) {
                let reply = if status == MessageStatus::Done { outcome.text.clone() } else { format!("Не удалось ответить: {}", outcome.failure.clone().unwrap_or_default()) };
                if let Ok(Some(next)) = store.return_call(tid, asker, &agent, &reply, m.props["depth"].as_i64().unwrap_or(1), &app.backend_default()) {
                    if let Some(nm) = next.message_id.as_ref().and_then(|i| store.get_message(i).ok()) {
                        app.publish("message", serde_json::to_value(&nm).unwrap_or_default());
                    }
                    app.publish("thread", json!({"id": tid}));
                }
            }
        }
    }
    if let (Some(t), Some(id)) = (thread.as_ref(), outcome.session_id.as_ref().or(sid.as_ref())) {
        if is_chat && !is_forum_thread && !provider.is_empty() && provider != "mock" {
            let _ = store.set_cli_session_for(&t.id, &session_key, &s.agent_id, id);
        }
    }
    // `/compact` done: keep the summary, drop the long session; the next turn starts a fresh one with the summary.
    if compact_turn && status == MessageStatus::Done && !outcome.text.trim().is_empty() && is_chat && !is_forum_thread {
        if let Some(t) = thread.as_ref() {
            let n = store.list_messages(&t.id, None, 2000).map(|v| v.len()).unwrap_or(0);
            if store.set_compact_summary(&t.id, &agent.id, outcome.text.trim(), n).is_ok() {
                let _ = store.clear_cli_session_for(&t.id, &session_key, &agent.id);
                if let Ok(m) = store.post_message(&t.id, "system", None, &format!("Контекст сжат. {} продолжит со своей сводкой в новой сессии.", agent.name), json!({}), MessageStatus::Done, None) {
                    app.publish("message", serde_json::to_value(&m).unwrap_or_default());
                }
                app.publish("thread", json!({"id": t.id}));
            }
        }
    }
    let sess_status = match status {
        MessageStatus::Cancelled => SessionStatus::Cancelled,
        MessageStatus::Failed => SessionStatus::Failed,
        _ => SessionStatus::Done,
    };
    let summary: String = outcome.text.chars().take(600).collect();
    let _ = store.finish_session(
        &s.id,
        sess_status,
        &outcome.text,
        &summary,
        outcome.failure.as_deref().unwrap_or(""),
        outcome.work.input_tokens as i64,
        outcome.work.output_tokens as i64,
    );
    if let Some(oid) = &s.obligation_id {
        let spent = outcome.work.input_tokens.saturating_sub(outcome.work.cached_input_tokens) + outcome.work.output_tokens;
        let _ = store.add_spent_tokens(oid, spent as i64);
        settle_obligation(&app, oid, &s, status);
    }
    app.publish("session", json!({"id": s.id, "status": sess_status.as_str()}));
    app.publish("attention", json!({}));
    app.poke();
    // Learning is the environment's job: reflect on finished work in the background (forum turns are
    // discussion, not work, and are summarised by the forum itself).
    let is_forum = thread.as_ref().map(|t| t.kind == ThreadKind::Forum).unwrap_or(false);
    if sess_status == SessionStatus::Done && !is_forum {
        tokio::spawn(crate::reflect::reflect_and_publish(app.clone(), s.id.clone(), false));
    }
}

/// What happens to an obligation when its session ends. The environment never lets work vanish
/// silently, and never loops on a session that makes no progress.
fn settle_obligation(app: &App, oid: &str, s: &Session, status: MessageStatus) {
    let store = &app.store;
    let Ok(o) = store.get_obligation(oid) else { return };
    if o.state != ObligationState::Running {
        return; // the agent set the end state itself (done / review / waiting / blocked)
    }
    let sessions = store.list_sessions(None, Some(oid), None, 50).unwrap_or_default();
    let block = |why: String| {
        let _ = store.transition(oid, ObligationState::Blocked, TransitionArgs { blocker: Some(why), ..Default::default() }, "system");
    };
    match status {
        MessageStatus::Cancelled => block("Остановлено пользователем. Продолжить, изменить задачу или отменить?".into()),
        MessageStatus::Failed => {
            let recent_failures = sessions.iter().take(3).filter(|x| x.status == SessionStatus::Failed || x.id == s.id).count();
            if recent_failures >= 2 {
                block("Две сессии подряд завершились ошибкой — нужно вмешательство (проверьте вход в бэкенд и доступы).".into());
            } else {
                let _ = store.transition(oid, ObligationState::Accepted, TransitionArgs { note: "сессия упала — повтор".into(), ..Default::default() }, "system");
            }
        }
        _ => {
            // Succeeded but did not report. Continue only while the agent shows a next step, up to a cap.
            if !o.next_step.trim().is_empty() && sessions.len() < 8 {
                let _ = store.transition(oid, ObligationState::Accepted, TransitionArgs { note: "работа продолжается".into(), ..Default::default() }, "system");
            } else {
                block("Сессия завершилась, не обновив задачу, — подтвердите результат или уточните задачу.".into());
            }
        }
    }
}

/// Stop a running or queued session. Returns true if something was stopped.
pub(crate) fn cancel_session(app: &App, session_id: &str) -> bool {
    if let Some(tx) = app.cancels.lock().unwrap().get(session_id) {
        let _ = tx.send(true);
        return true;
    }
    // Not started yet: close it right here.
    if let Ok(s) = app.store.get_session(session_id) {
        if s.status == SessionStatus::Queued {
            let _ = app.store.finish_session(session_id, SessionStatus::Cancelled, "", "", "", 0, 0);
            if let Some(m) = &s.message_id {
                if let Ok(msg) = app.store.update_message(m, None, Some(&json!({"phase": null})), Some(MessageStatus::Cancelled)) {
                    app.publish("message", serde_json::to_value(&msg).unwrap_or_default());
                }
            }
            return true;
        }
    }
    false
}
