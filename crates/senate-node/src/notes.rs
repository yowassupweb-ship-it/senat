//! Specialists talking while they work.
//!
//! * Notes (`senate_message_colleague`): delivered always — with the recipient's next Senate tool result while they
//!   work, or at the start of their next turn — and the sender can see when one was read.
//! * A question to a colleague that waits (`senate_ask_colleague`): the colleague gets a turn now; the asker's tool
//!   call waits a short while for the answer. If the colleague is busy, out of limits or fails, the asker learns that
//!   within the wait and goes on; a late answer still comes back as a turn of its own.
//! * A question to the person that waits (`senate_ask_person`): a card with an answer field in the chat; the tool
//!   waits up to half an hour, then the specialist goes on with their own best choice.

use crate::App;
use senate_core::model::MessageStatus;
use senate_core::tools::{self, ToolCtx};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Notes in a turn's prompt.
pub(crate) fn block(notes: &[Value]) -> String {
    let mut s = String::from("<senate-notes>\nЗаписки от коллег, пока тебя не было. Учти их в этой работе; срочные — прежде всего.\n");
    // the same note sent again (a retry, a second attempt) is said once
    let mut seen = std::collections::HashSet::new();
    for n in notes.iter().filter(|n| seen.insert((n["from_slug"].to_string(), n["text"].to_string()))) {
        s += &format!("- {}{} (@{}): {}\n", if n["urgent"] == true { "СРОЧНО · " } else { "" }, n["from"].as_str().unwrap_or(""), n["from_slug"].as_str().unwrap_or(""), n["text"].as_str().unwrap_or(""));
    }
    s + "</senate-notes>\n\n"
}

/// Any Senate tool result carries the notes that came in while the specialist worked.
pub(crate) fn attach(a: &App, ctx: &ToolCtx, result: Value) -> Value {
    if let Ok(notes) = a.store.take_notes(&ctx.agent_id) {
        if !notes.is_empty() {
            let mut seen = std::collections::HashSet::new();
            let notes: Vec<Value> = notes.into_iter().filter(|n| seen.insert((n["from_slug"].to_string(), n["text"].to_string()))).collect();
            let wrapped = if result.is_object() { result } else { json!({"result": result}) };
            let mut w = wrapped;
            w["notes_from_colleagues"] = json!(notes);
            w["notes_hint"] = json!("Это записки коллег, пришедшие во время твоей работы: прочитай и учти до следующего шага.");
            return w;
        }
    }
    result
}

fn message_of_turn(a: &App, ctx: &ToolCtx) -> Option<String> {
    ctx.session_id.as_deref().and_then(|s| a.store.get_session(s).ok()).and_then(|s| s.message_id)
}

pub(crate) async fn ask_colleague(a: &Arc<App>, ctx: &ToolCtx, args: &Value) -> Result<Value, String> {
    let mut wait = args["wait_seconds"].as_u64().unwrap_or(120).clamp(10, 300);
    // a colleague already busy will not answer within minutes: do not hold the asker for long, say so
    let target = a.store.get_agent(args["to"].as_str().unwrap_or("")).map_err(|_| format!("нет коллеги «{}»: посмотри slug в senate_colleagues", args["to"].as_str().unwrap_or("")))?;
    let busy = a.store.agent_running(&target.id).unwrap_or(false) || a.store.agent_queued(&target.id).unwrap_or(false);
    if busy {
        wait = wait.min(30);
    }
    let question = args["question"].as_str().unwrap_or("").to_string();
    let r = tools::call(&a.store, ctx, "senate_call_colleague", &json!({"to": args["to"], "request": question})).map_err(|e| e.to_string())?;
    let Some(mid) = r["message_id"].as_str().map(String::from) else { return Ok(r) };
    // the colleague's answer comes here while we wait, not as a separate turn
    let _ = a.store.update_message(&mid, None, Some(&json!({"inline_wait": true})), None);
    a.publish("thread", json!({"id": ctx.thread_id}));
    a.wake.notify_waiters();
    let until = Instant::now() + Duration::from_secs(wait);
    loop {
        if let Ok(m) = a.store.get_message(&mid) {
            match m.status {
                MessageStatus::Done => return Ok(json!({"status": "answered", "answer": m.body})),
                MessageStatus::Failed | MessageStatus::Cancelled => {
                    let why = m.props["error"].as_str().unwrap_or("не смог ответить").to_string();
                    let _ = a.store.update_message(&mid, None, Some(&json!({"inline_gave_up": true})), None);
                    return Ok(json!({"status": "failed", "reason": why, "note": "коллега не ответит сейчас: продолжай по своему решению"}));
                }
                _ => {}
            }
            if Instant::now() >= until {
                let _ = a.store.update_message(&mid, None, Some(&json!({"inline_gave_up": true})), None);
                let state = if m.status == MessageStatus::Queued { "занят другой работой и ещё не начал" } else { "ещё работает над ответом" };
                return Ok(json!({"status": if busy || m.status == MessageStatus::Queued { "busy" } else { "no_answer_yet" }, "colleague": state, "note": format!("за {wait} с ответа нет: {} {state}. Продолжай по своему решению; его ответ придёт к тебе отдельным ходом.", target.name)}));
            }
        }
        tokio::time::sleep(Duration::from_millis(1500)).await;
    }
}

pub(crate) async fn ask_person(a: &Arc<App>, ctx: &ToolCtx, args: &Value) -> Result<Value, String> {
    let question: String = args["question"].as_str().unwrap_or("").trim().chars().take(2000).collect();
    if question.is_empty() {
        return Err("пустой вопрос".into());
    }
    let options: Vec<String> = args["options"].as_array().map(|l| l.iter().filter_map(|x| x.as_str()).map(|s| s.chars().take(120).collect()).take(6).collect()).unwrap_or_default();
    let wait = args["wait_seconds"].as_u64().unwrap_or(900).clamp(30, 1800);
    let mid = message_of_turn(a, ctx).ok_or("у этого хода нет сообщения в чате: спросить негде")?;
    let qid = uuid::Uuid::new_v4().simple().to_string();
    let (tx, rx) = tokio::sync::oneshot::channel();
    a.questions.lock().unwrap().insert(mid.clone(), (qid.clone(), tx));
    let card = json!({"question": {"id": qid, "text": question, "options": options, "state": "pending", "until": senate_core::now_ms() + (wait as i64) * 1000}});
    if let Ok(m) = a.store.update_message(&mid, None, Some(&card), None) {
        a.publish("message", serde_json::to_value(&m).unwrap_or_default());
    }
    a.publish("attention", json!({}));
    let answer = tokio::time::timeout(Duration::from_secs(wait), rx).await;
    a.questions.lock().unwrap().remove(&mid);
    match answer {
        Ok(Ok(text)) => Ok(json!({"status": "answered", "answer": text})),
        _ => {
            if let Ok(m) = a.store.update_message(&mid, None, Some(&json!({"question": {"id": qid, "text": question, "options": options, "state": "expired"}})), None) {
                a.publish("message", serde_json::to_value(&m).unwrap_or_default());
            }
            Ok(json!({"status": "no_answer", "note": "человек не ответил: выбери самый разумный вариант, продолжай и в итоге скажи, что решил сам"}))
        }
    }
}

/// The person's answer to a question card.
pub(crate) fn answer(a: &App, message_id: &str, text: &str) -> Result<(), String> {
    let (qid, tx) = a.questions.lock().unwrap().remove(message_id).ok_or("вопрос уже закрыт")?;
    let m = a.store.get_message(message_id).map_err(|e| e.to_string())?;
    let mut q = m.props["question"].clone();
    q["state"] = json!("answered");
    q["answer"] = json!(text);
    if q["id"] != json!(qid) {
        return Err("вопрос уже закрыт".into());
    }
    if let Ok(m) = a.store.update_message(message_id, None, Some(&json!({"question": q})), None) {
        a.publish("message", serde_json::to_value(&m).unwrap_or_default());
    }
    let _ = tx.send(text.to_string());
    Ok(())
}
