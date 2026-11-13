//! Learning as a property of the environment, not a whim of the model.
//!
//! A specialist that has to remember to call its memory tool mostly will not (a designer analysed a
//! whole website and saved nothing). So after substantive work the environment itself runs a short
//! reflection pass: a cheap model, no tools, reads what was asked / done / answered and proposes up
//! to three things worth keeping. They are stored as unverified observations with their origin
//! ("auto:<session>") and shown under the answer, where the user can verify or remove them.
//! Model confidence is never treated as verification (Senate.md section 15).

use crate::App;
use senate_core::model::*;
use senate_runtime::{Approver, Decision, PermissionMode, PermissionRequest, RunSpec};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

struct Deny;
impl Approver for Deny {
    fn ask(&self, _: PermissionRequest) -> Pin<Box<dyn Future<Output = Decision> + Send + '_>> {
        Box::pin(async { Decision::Deny("рефлексия не использует инструменты".into()) })
    }
}

/// How much the environment learns by itself. `high` is the full behaviour; lower levels are quieter.
///   off    — nothing is extracted (specialists may still save things themselves)
///   low    — only what the person explicitly asks to remember or decides; at most one entry
///   medium — real work (several steps or a long answer) or explicit asks; at most two entries (default)
///   high   — any work with tools, any substantial answer; at most three entries
pub(crate) fn level(app: &App) -> String {
    match app.store.setting("learn_level").ok().flatten().as_deref() {
        Some(l @ ("off" | "low" | "medium" | "high")) => l.to_string(),
        // before the level existed there was an on/off switch
        _ if app.store.setting("auto_learn").ok().flatten().as_deref() == Some("0") => "off".into(),
        _ => "medium".into(),
    }
}

fn max_entries(level: &str) -> usize {
    match level {
        "low" => 1,
        "medium" => 2,
        _ => 3,
    }
}

const MARKERS: &[&str] = &["запомни", "учти", "договорились", "решили", "всегда", "никогда", "предпочитаю", "нужно чтобы", "важно"];

/// Is there anything worth reflecting on? Small talk is not.
pub(crate) fn substantial(steps: &Value, answer: &str, user_text: &str, level: &str) -> bool {
    // Looking something up (memory, colleagues, tool discovery) is not work worth learning from.
    let tools = steps
        .as_array()
        .map(|a| a.iter().filter(|s| s["kind"] == "tool" && !s["name"].as_str().unwrap_or("").starts_with("senate_") && s["name"] != "ToolSearch").count())
        .unwrap_or(0);
    let u = user_text.to_lowercase();
    let a = answer.to_lowercase();
    let knows_nothing = ["ничего не известно", "в памяти нет", "в памяти пусто", "не нашёл", "не нашла", "не знаю"].iter().any(|k| a.contains(k)) && tools == 0;
    if knows_nothing && !MARKERS.iter().any(|m| u.contains(m)) {
        return false;
    }
    let asked = MARKERS.iter().any(|m| u.contains(m));
    let long = answer.chars().count();
    match level {
        "off" => false,
        "low" => asked,
        "medium" => asked || tools >= 2 || (tools >= 1 && long >= 600) || long >= 1500,
        _ => tools >= 1 || long >= 600 || asked,
    }
}

fn words(s: &str) -> std::collections::HashSet<String> {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 4).map(String::from).collect()
}

fn near_duplicate(a: &str, b: &str) -> bool {
    let (x, y) = (words(a), words(b));
    if x.is_empty() || y.is_empty() {
        return false;
    }
    let inter = x.intersection(&y).count() as f64;
    inter / (x.len().min(y.len()) as f64) >= 0.5
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

fn extract_json(text: &str) -> Option<Value> {
    let a = text.find('{')?;
    let b = text.rfind('}')?;
    serde_json::from_str(&text[a..=b]).ok()
}

/// Run reflection for a finished session. `force` skips the "is it substantial" gate (manual button).
pub(crate) async fn reflect(app: &Arc<App>, session_id: &str, force: bool) -> Result<Vec<Memory>, String> {
    let store = &app.store;
    let s = store.get_session(session_id).map_err(|e| e.to_string())?;
    let provider = s.backend.split(':').next().unwrap_or("").to_string();
    if provider == "mock" || provider.is_empty() {
        return Ok(vec![]);
    }
    let msg_id = s.message_id.clone().ok_or("у сессии нет сообщения")?;
    let msg = store.get_message(&msg_id).map_err(|e| e.to_string())?;
    let agent = store.get_agent(&s.agent_id).map_err(|e| e.to_string())?;
    let thread = s.thread_id.as_deref().and_then(|t| store.get_thread(t).ok());
    let project = thread.as_ref().and_then(|t| t.project_id.as_deref()).and_then(|p| store.get_project(p).ok());
    let company_id = thread.as_ref().and_then(|t| t.company_id.clone()).or_else(|| project.as_ref().and_then(|p| p.company_id.clone()));

    // What was asked: the human message right before this answer (for obligation runs: the obligation itself).
    let user_text = match &s.thread_id {
        Some(t) => store
            .list_messages(t, None, 40)
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|m| m.author_kind == "user" && m.created_at <= msg.created_at)
            .last()
            .map(|m| m.body)
            .unwrap_or_default(),
        None => String::new(),
    };
    let user_text = if user_text.is_empty() { s.reason.clone() } else { user_text };
    let lvl = level(app);
    if !force && !substantial(&msg.props["steps"], &msg.body, &user_text, &lvl) {
        return Ok(store.memory_of_session(session_id).map_err(|e| e.to_string())?);
    }

    let mut steps_txt = String::new();
    for st in msg.props["steps"].as_array().cloned().unwrap_or_default().iter().filter(|x| x["kind"] == "tool").take(25) {
        steps_txt += &format!("- {} | {}\n", st["name"].as_str().unwrap_or(""), clip(st["hint"].as_str().unwrap_or(""), 120));
    }
    let known = store
        .search_memory_in(&format!("{} {}", user_text, clip(&msg.body, 300)), Some(&agent.id), project.as_ref().map(|p| p.id.as_str()), company_id.as_deref(), 10)
        .map_err(|e| e.to_string())?;
    let known_txt = known.iter().map(|m| format!("- {}", clip(&m.body, 200))).collect::<Vec<_>>().join("\n");
    let mut layers = vec!["personal — общее мастерство специалиста, пригодится в любой компании"];
    if project.is_some() {
        layers.push("project — факт или решение именно этого проекта");
    }
    if company_id.is_some() {
        layers.push("company_personal — что специалист узнал о компании (только ему)");
        layers.push("company — знание о компании, полезное всем, кто там работает");
    }
    let prompt = format!(
        "Ты — модуль рефлексии постоянной среды Senate. Специалист «{name}» ({role}) только что выполнил работу. \
Выдели из неё то, что стоит сохранить в памяти, чтобы в следующий раз работать лучше и не переспрашивать: устойчивые факты о компании, проекте или клиенте; \
предпочтения и требования человека; принятые решения вместе с причиной; найденные ошибки и способ их избежать; воспроизводимые приёмы.\n\
НЕ сохраняй: пересказ разговора, одноразовые детали, общеизвестное, то, что уже известно (список ниже), догадки как факты; общие правила поведения вида «если не знаешь — скажи об этом» и любые выводы из ответа «я ничего не знаю»; вид decision — только если человек в этом разговоре сам принял решение.\n\
Максимум {max} записи; каждая — самостоятельное утверждение до 300 знаков, понятное без контекста. Если ничего ценного нет — верни пустой список.\n\n\
Допустимые слои:\n{layers}\n\nВиды: observation (наблюдение), hypothesis (гипотеза), preference (предпочтение человека), decision (решение), error (ошибка и как избежать).\n\n\
## Что просил человек\n{ask}\n\n## Что делал специалист (инструменты)\n{steps}\n## Ответ специалиста\n{answer}\n\n## Уже известно (не повторять)\n{known}\n\n\
Ответь ТОЛЬКО JSON без пояснений: {{\"learnings\":[{{\"body\":\"...\",\"kind\":\"observation\",\"layer\":\"personal\",\"why\":\"почему это стоит помнить\"}}]}}",
        name = agent.name,
        role = agent.role,
        layers = layers.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n"),
        ask = clip(&user_text, 1500),
        steps = if steps_txt.is_empty() { "(без инструментов)\n".to_string() } else { steps_txt },
        answer = clip(&msg.body, 6000),
        known = if known_txt.is_empty() { "(пока ничего)".to_string() } else { known_txt },
        max = if force { 3 } else { max_entries(&lvl) },
    );

    let backend = if provider == "claude" { "claude:haiku".to_string() } else { provider.clone() };
    let cwd = app.cfg.data_dir.join("workspaces").join("_reflect");
    let _ = std::fs::create_dir_all(&cwd);
    let spec = RunSpec {
        backend,
        effort: Some("low".into()),
        mode: PermissionMode::Plan,
        cwd,
        system_prompt: String::new(),
        prompt,
        resume: None,
        mcp: None,
        extra_mcp: vec![],
        extra_dirs: vec![],
        env: vec![],
        bare: true,
        work_dir: app.work_dir(),
        run_id: format!("rf-{}", &session_id[..12.min(session_id.len())]),
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let (_keep, crx) = tokio::sync::watch::channel(false);
    let out = tokio::time::timeout(Duration::from_secs(150), senate_runtime::run(spec, &Deny, tx, crx)).await.map_err(|_| "рефлексия не уложилась во время".to_string())?;
    if let Some(f) = &out.failure {
        return Err(format!("рефлексия не удалась: {f}"));
    }
    let v = extract_json(&out.text).ok_or("рефлексия вернула не JSON")?;

    let mut stored = vec![];
    for l in v["learnings"].as_array().cloned().unwrap_or_default().into_iter().take(if force { 3 } else { max_entries(&lvl) }) {
        let body = l["body"].as_str().unwrap_or("").trim().to_string();
        if body.chars().count() < 12 {
            continue;
        }
        let body = clip(&body, 320);
        let kind = match l["kind"].as_str().unwrap_or("observation") {
            "hypothesis" => MemoryKind::Hypothesis,
            "preference" => MemoryKind::Preference,
            "decision" => MemoryKind::Decision,
            "error" => MemoryKind::Error,
            _ => MemoryKind::Observation,
        };
        // Layer, falling back to what the context allows.
        let (level, co, pr) = match (l["layer"].as_str().unwrap_or("personal"), &company_id, &project) {
            ("project", _, Some(p)) => (MemoryLevel::Project, None, Some(p.id.clone())),
            ("company", Some(c), _) => (MemoryLevel::Company, Some(c.clone()), None),
            ("company_personal", Some(c), _) => (MemoryLevel::Personal, Some(c.clone()), None),
            _ => (MemoryLevel::Personal, None, None),
        };
        // Not already known?
        let mut existing = store.search_memory_in(&body, Some(&agent.id), project.as_ref().map(|p| p.id.as_str()), company_id.as_deref(), 20).unwrap_or_default();
        existing.extend(store.list_memory(Some(MemoryLevel::Personal), Some(&agent.id), None, false, 150).unwrap_or_default());
        if existing.iter().any(|m| near_duplicate(&m.body, &body)) {
            continue;
        }
        let m = store
            .add_memory(NewMemory {
                level,
                agent_id: if level == MemoryLevel::Company { None } else { Some(agent.id.clone()) },
                project_id: pr,
                company_id: co,
                kind,
                body,
                source: format!("auto:{session_id}"),
                scope_note: clip(l["why"].as_str().unwrap_or(""), 200),
                verification: String::new(),
                fresh_until: None,
            })
            .map_err(|e| e.to_string())?;
        stored.push(m);
    }
    let _ = store.emit("learning.reflected", "session", session_id, &agent.id, json!({"stored": stored.len()}));
    Ok(store.memory_of_session(session_id).map_err(|e| e.to_string())?)
}

pub(crate) fn learned_props(list: &[Memory]) -> Value {
    json!(list.iter().map(|m| json!({"id": m.id, "body": m.body, "kind": m.kind.as_str(), "level": m.level.as_str(), "auto": m.source.starts_with("auto:"), "company": m.company_id})).collect::<Vec<_>>())
}

/// After a session: mark the message "learning", run reflection, attach the traces to the message.
pub(crate) async fn reflect_and_publish(app: Arc<App>, session_id: String, force: bool) {
    let Ok(sess) = app.store.get_session(&session_id) else { return };
    let Some(mid) = sess.message_id.clone() else { return };
    let enabled = force || level(&app) != "off";
    if !enabled {
        return;
    }
    let publish = |app: &Arc<App>, props: Value| {
        if let Ok(m) = app.store.update_message(&mid, None, Some(&props), None) {
            app.publish("message", serde_json::to_value(&m).unwrap_or_default());
        }
    };
    publish(&app, json!({"learning": true}));
    let result = reflect(&app, &session_id, force).await;
    match result {
        Ok(list) => publish(&app, json!({"learning": null, "learned": learned_props(&list), "learn_error": null})),
        Err(e) => {
            tracing::warn!("рефлексия: {e}");
            let list = app.store.memory_of_session(&session_id).unwrap_or_default();
            publish(&app, json!({"learning": null, "learned": learned_props(&list), "learn_error": e}));
        }
    }
    crate::accounts::publish(&app);
}

/// A memory entry changed (verified, removed, corrected): the "learned" list under the answer it came
/// from must follow, otherwise a removed entry reappears with the next update of the chat.
pub(crate) fn refresh_learned(app: &Arc<App>, memory_id: &str) {
    let Ok(m) = app.store.get_memory(memory_id) else { return };
    let session_id = m.source.strip_prefix("auto:").unwrap_or(&m.source).to_string();
    let Ok(sess) = app.store.get_session(&session_id) else { return };
    let Some(mid) = sess.message_id else { return };
    let list = app.store.memory_of_session(&session_id).unwrap_or_default();
    if let Ok(msg) = app.store.update_message(&mid, None, Some(&json!({"learned": learned_props(&list)})), None) {
        app.publish("message", serde_json::to_value(&msg).unwrap_or_default());
    }
}
