//! Forum: specialists discuss a broad question and plan the work (Senate.md sections 12-13).
//!
//! The environment, not a "chief agent", runs it: a subject, an expected outcome, a bounded number
//! of rounds and an end condition. Participants speak in turn (each sees what was said before, and
//! anything the human adds), read-only. When everyone passes or the rounds are used up, the leader
//! sums up with a structured plan. The plan becomes tasks and decisions only when the user accepts
//! it — an idea does not turn into an assignment by itself.

use crate::{executor, App};
use senate_core::engine::TransitionArgs;
use senate_core::model::*;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

fn status(app: &App, tid: &str) -> String {
    app.store.forum_meta(tid).ok().and_then(|m| m["status"].as_str().map(String::from)).unwrap_or_default()
}

fn publish_thread(app: &App, tid: &str) {
    app.publish("thread", json!({"id": tid}));
    app.publish("attention", json!({}));
}

fn say(app: &App, tid: &str, text: &str) {
    if let Ok(m) = app.store.post_message(tid, "system", None, text, json!({}), MessageStatus::Done, None) {
        app.publish("message", serde_json::to_value(&m).unwrap_or_default());
    }
}

/// Start (or continue) the orchestrator of a forum. Safe to call twice: only one runs per forum.
pub(crate) fn start(app: &Arc<App>, tid: &str) {
    if !app.forums.lock().unwrap().insert(tid.to_string()) {
        return;
    }
    let (a, t) = (app.clone(), tid.to_string());
    tokio::spawn(async move {
        if let Err(e) = drive(&a, &t).await {
            tracing::warn!("форум {t}: {e}");
            say(&a, &t, &format!("Форум остановлен: {e}"));
            let _ = a.store.set_forum_meta(&t, &json!({"status": "stopped"}));
        }
        a.forums.lock().unwrap().remove(&t);
        publish_thread(&a, &t);
    });
}

/// After a restart: forums that were mid-discussion carry on.
pub(crate) fn resume_all(app: &Arc<App>) {
    if let Ok(threads) = app.store.list_threads(None, false) {
        for t in threads.into_iter().filter(|t| t.kind == ThreadKind::Forum) {
            if matches!(t.meta["status"].as_str(), Some("open" | "summarizing")) {
                start(app, &t.id);
            }
        }
    }
}

async fn drive(app: &Arc<App>, tid: &str) -> Result<(), String> {
    let store = &app.store;
    loop {
        let meta = store.forum_meta(tid).map_err(|e| e.to_string())?;
        if meta["status"] != "open" {
            break;
        }
        let rounds = meta["rounds"].as_i64().unwrap_or(2);
        let done = meta["round"].as_i64().unwrap_or(0);
        if done >= rounds || meta["conclude_now"] == true {
            break;
        }
        let round = done + 1;
        store.set_forum_meta(tid, &json!({"round": round})).map_err(|e| e.to_string())?;
        say(app, tid, &format!("Раунд {round} из {rounds}"));
        publish_thread(app, tid);
        let mut ids: Vec<String> = meta["participants"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let n = ids.len().max(1);
        ids.rotate_left(((round - 1) as usize) % n);
        let mut passes = 0;
        for id in &ids {
            if status(app, tid) != "open" {
                return Ok(());
            }
            if store.forum_meta(tid).map(|m| m["conclude_now"] == true).unwrap_or(false) {
                break;
            }
            let text = turn(app, tid, id, round, false).await?;
            if text.trim().to_uppercase().starts_with("ПАС") {
                passes += 1;
            }
        }
        if passes == ids.len() {
            say(app, tid, "Все участники сказали, что добавить нечего, — обсуждение завершено.");
            break;
        }
    }
    if status(app, tid) != "open" {
        return Ok(());
    }

    // The leader sums up.
    store.set_forum_meta(tid, &json!({"status": "summarizing", "conclude_now": null})).map_err(|e| e.to_string())?;
    publish_thread(app, tid);
    let meta = store.forum_meta(tid).map_err(|e| e.to_string())?;
    let leader = meta["leader"].as_str().unwrap_or_default().to_string();
    let round = meta["round"].as_i64().unwrap_or(0);
    let (mid, text) = turn_with_id(app, tid, &leader, round, true).await?;
    if status(app, tid) != "summarizing" {
        return Ok(());
    }
    let plan = parse_plan(store, &text);
    let human = text.split("```senate-plan").next().unwrap_or(&text).trim().to_string();
    if let Ok(m) = store.update_message(&mid, Some(&human), Some(&json!({"plan": plan, "summary": true})), None) {
        app.publish("message", serde_json::to_value(&m).unwrap_or_default());
    }
    store.set_forum_meta(tid, &json!({"status": "concluded", "plan": plan, "plan_applied": false})).map_err(|e| e.to_string())?;
    if plan.is_null() {
        say(app, tid, "Итог подведён без структурированного плана — его можно запросить сообщением.");
    }
    Ok(())
}

async fn turn(app: &Arc<App>, tid: &str, agent_id: &str, round: i64, synthesis: bool) -> Result<String, String> {
    turn_with_id(app, tid, agent_id, round, synthesis).await.map(|(_, t)| t)
}

/// One speaker's turn: a normal read-only session in the forum thread; waits until it ends.
async fn turn_with_id(app: &Arc<App>, tid: &str, agent_id: &str, round: i64, synthesis: bool) -> Result<(String, String), String> {
    let store = &app.store;
    let agent = store.get_agent(agent_id).map_err(|e| e.to_string())?;
    let thread = store.get_thread(tid).map_err(|e| e.to_string())?;
    let backend = if !thread.model.is_empty() {
        thread.model.clone()
    } else if !agent.model_pref.is_empty() {
        agent.model_pref.clone()
    } else {
        app.backend_default()
    };
    let prompt = senate_core::workset::build_forum_prompt(store, &agent, &thread, round, synthesis).map_err(|e| e.to_string())?;
    let msg = store
        .post_message(tid, "agent", Some(&agent.id), "", json!({"backend": backend, "phase": "в очереди", "steps": [], "forum": {"round": round, "synthesis": synthesis}}), MessageStatus::Queued, None)
        .map_err(|e| e.to_string())?;
    let sess = store
        .create_session(&agent.id, None, Some(tid), Some(&msg.id), if synthesis { "итог форума" } else { "форум" }, &backend, &prompt)
        .map_err(|e| e.to_string())?;
    app.publish("message", serde_json::to_value(&msg).unwrap_or_default());
    app.poke();
    loop {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let s = store.get_session(&sess.id).map_err(|e| e.to_string())?;
        if matches!(s.status, SessionStatus::Done | SessionStatus::Failed | SessionStatus::Cancelled) {
            if s.status == SessionStatus::Failed {
                return Err(format!("{} не смог ответить: {}", agent.name, if s.error.is_empty() { "ошибка" } else { &s.error }));
            }
            break;
        }
        let st = status(app, tid);
        if st != "open" && st != "summarizing" {
            executor::cancel_session(app, &sess.id);
            return Ok((msg.id, String::new()));
        }
    }
    let m = store.get_message(&msg.id).map_err(|e| e.to_string())?;
    Ok((msg.id, m.body))
}

/// The ```senate-plan block → JSON with owners resolved to specialists.
fn parse_plan(store: &senate_core::Store, text: &str) -> Value {
    let Some(a) = text.find("```senate-plan") else { return Value::Null };
    let rest = &text[a + "```senate-plan".len()..];
    let end = rest.find("```").unwrap_or(rest.len());
    let Ok(mut v) = serde_json::from_str::<Value>(rest[..end].trim()) else { return Value::Null };
    if let Some(work) = v["work"].as_array_mut() {
        for w in work.iter_mut() {
            let slug = w["owner"].as_str().unwrap_or("").trim_start_matches('@').to_string();
            if let Ok(ag) = store.get_agent(&slug) {
                w["owner_id"] = json!(ag.id);
                w["owner_name"] = json!(ag.name);
            }
        }
    }
    v
}

/// The user accepts the plan: tasks and decisions come into being (once).
pub(crate) fn apply_plan(app: &Arc<App>, tid: &str) -> Result<Value, String> {
    let store = &app.store;
    let thread = store.get_thread(tid).map_err(|e| e.to_string())?;
    if thread.meta["plan_applied"] == true {
        return Err("план уже принят".into());
    }
    let plan = thread.meta["plan"].clone();
    if plan.is_null() {
        return Err("в итоге форума нет структурированного плана".into());
    }
    let mut ids: std::collections::HashMap<String, String> = Default::default();
    let mut tasks = 0;
    let work = plan["work"].as_array().cloned().unwrap_or_default();
    for w in &work {
        let title = w["title"].as_str().unwrap_or("").trim().to_string();
        if title.is_empty() {
            continue;
        }
        let o = store
            .create_obligation(NewObligation {
                title: title.clone(),
                result: w["result"].as_str().unwrap_or("").into(),
                criteria: w["criteria"].as_str().unwrap_or("").into(),
                owner_id: w["owner_id"].as_str().map(String::from),
                project_id: thread.project_id.clone(),
                requested_by: "forum".into(),
                ..Default::default()
            })
            .map_err(|e| e.to_string())?;
        ids.insert(title, o.id);
        tasks += 1;
    }
    // Dependencies: a task waits for the one it depends on.
    for w in &work {
        let Some(me) = ids.get(w["title"].as_str().unwrap_or("")) else { continue };
        if let Some(dep) = w["depends_on"].as_array().and_then(|d| d.iter().filter_map(|x| x.as_str()).find_map(|t| ids.get(t))) {
            let _ = store.transition(me, ObligationState::Waiting, TransitionArgs { wait: Some(WaitCondition::Obligation { id: dep.clone() }), note: "зависит от другой задачи форума".into(), ..Default::default() }, "forum");
        }
    }
    let mut decisions = 0;
    for d in plan["decisions"].as_array().cloned().unwrap_or_default() {
        let title = d["title"].as_str().unwrap_or("").trim().to_string();
        if title.is_empty() {
            continue;
        }
        let scope_company = d["scope"] == "company" && thread.company_id.is_some();
        let (p, c) = if scope_company { (None, thread.company_id.clone()) } else { (thread.project_id.clone(), None) };
        if p.is_none() && c.is_none() {
            continue; // a decision needs a project or a company to live in
        }
        if let Ok(dec) = store.propose_decision_in(p.as_deref(), c.as_deref(), &title, d["body"].as_str().unwrap_or(""), "forum", None) {
            let _ = store.set_decision_status(&dec.id, DecisionStatus::Accepted, "user");
            decisions += 1;
        }
    }
    store.set_forum_meta(tid, &json!({"plan_applied": true})).map_err(|e| e.to_string())?;
    say(app, tid, &format!("План принят: создано задач — {tasks}, решений — {decisions}."));
    app.publish("attention", json!({}));
    app.poke();
    Ok(json!({"tasks": tasks, "decisions": decisions}))
}

/// A human message in a forum: joins the discussion; a finished forum gets one more round.
pub(crate) fn on_user_message(app: &Arc<App>, tid: &str) {
    let Ok(meta) = app.store.forum_meta(tid) else { return };
    match meta["status"].as_str() {
        Some("concluded" | "stopped") => {
            let round = meta["round"].as_i64().unwrap_or(0);
            let _ = app.store.set_forum_meta(tid, &json!({"status": "open", "rounds": round + 1, "plan": null, "plan_applied": false}));
            start(app, tid);
        }
        Some("open" | "summarizing") => start(app, tid), // no-op if it is already running
        _ => {}
    }
    publish_thread(app, tid);
}

pub(crate) fn stop(app: &Arc<App>, tid: &str) {
    let _ = app.store.set_forum_meta(tid, &json!({"status": "stopped"}));
    publish_thread(app, tid);
}

/// Skip the remaining rounds and ask the leader for the summary now.
pub(crate) fn conclude(app: &Arc<App>, tid: &str) {
    let meta = app.store.forum_meta(tid).unwrap_or_default();
    match meta["status"].as_str() {
        Some("open") => {
            let _ = app.store.set_forum_meta(tid, &json!({"conclude_now": true}));
        }
        Some("concluded" | "stopped") => {
            let round = meta["round"].as_i64().unwrap_or(0);
            let _ = app.store.set_forum_meta(tid, &json!({"status": "open", "rounds": round, "plan": null, "plan_applied": false}));
            start(app, tid);
        }
        _ => {}
    }
    publish_thread(app, tid);
}
