//! HTTP API of the node. Everything the UI (desktop or browser) does goes through here.

use crate::{accounts, executor, external, forum, project_git, skills, uploads, App};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use futures_util::StreamExt;
use senate_core::engine::{ChatSend, TransitionArgs};
use senate_core::model::*;
use senate_core::store::{ProjectPatch, ThreadPatch};
use senate_core::tools::{self, ToolCtx};
use senate_core::Error;
use senate_runtime::Decision;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio_stream::wrappers::BroadcastStream;

type S = State<Arc<App>>;
type R = Result<Json<Value>, ApiError>;

pub struct ApiError(StatusCode, String);

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        let code = match &e {
            Error::NotFound(_) => StatusCode::NOT_FOUND,
            Error::Invalid(_) => StatusCode::BAD_REQUEST,
            Error::Transition(_) => StatusCode::CONFLICT,
            Error::Forbidden(_) => StatusCode::FORBIDDEN,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError(code, e.to_string())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}
fn bad(msg: &str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}
fn ok(v: impl serde::Serialize) -> R {
    Ok(Json(serde_json::to_value(v).unwrap_or(Value::Null)))
}
fn de<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, ApiError> {
    serde_json::from_value(v).map_err(|e| bad(&format!("неверный запрос: {e}")))
}

async fn auth(State(app): S, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    // Static UI and the session-token bridge authenticate differently.
    if !path.starts_with("/api/") || path == "/api/health" || path.starts_with("/api/agent/") {
        return next.run(req).await;
    }
    let bearer = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).map(String::from);
    let query_tok = req.uri().query().and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("token="))).map(String::from);
    if bearer.as_deref() == Some(app.token.as_str()) || query_tok.as_deref() == Some(app.token.as_str()) {
        return next.run(req).await;
    }
    ApiError(StatusCode::UNAUTHORIZED, "нужен токен доступа".into()).into_response()
}

pub fn router(app: Arc<App>) -> Router {
    let api = Router::new()
        .route("/api/health", get(|| async { Json(json!({"ok": true, "name": "senate-node", "version": env!("CARGO_PKG_VERSION")})) }))
        .route("/api/state", get(state))
        .route("/api/events", get(events))
        .route("/api/attention", get(|State(a): S| async move { ok(a.store.attention()?) }))
        .route("/api/live", get(live))
        .route("/api/calendar", get(calendar))
        .route("/api/routines", get(routines_list).post(routine_save))
        .route("/api/routines/:id", axum::routing::patch(routine_update).delete(routine_delete))
        .route("/api/routines/:id/run", post(routine_run))
        .route("/api/accesses", get(accesses_list).post(access_save))
        .route("/api/accesses/:id", axum::routing::patch(access_update).delete(access_delete))
        .route("/api/accesses/:id/log", get(access_log))
        .route("/api/permission-rules", get(|State(a): S| async move { ok(a.store.list_permission_rules()?) }).post(permission_rule_add))
        .route("/api/permission-rules/:id", axum::routing::delete(permission_rule_delete))
        // chats
        .route("/api/threads", get(threads).post(create_thread))
        .route("/api/threads/:id", get(get_thread).patch(patch_thread))
        .route("/api/threads/:id/messages", get(messages).post(send))
        // files the person attaches (screenshots): one upload per file, then messages refer to them by id
        .route("/api/uploads", post(upload).layer(DefaultBodyLimit::max(uploads::MAX_BYTES + 1024 * 1024)))
        .route("/api/uploads/:id/:name", get(serve_upload))
        .route("/api/threads/:id/preview/open", post(open_preview))
        .route("/api/threads/:id/plan/pause", post(plan_pause))
        .route("/api/threads/:id/plan/resume", post(plan_resume))
        .route("/api/messages/:id/answer", post(answer))
        .route("/api/messages/:id/cancel", post(cancel))
        .route("/api/messages/:id/reflect", post(reflect_message))
        // specialists
        .route("/api/agents", get(agents).post(create_agent))
        .route("/api/agents/:id", get(agent_detail).patch(patch_agent))
        .route("/api/agents/:id/profile", post(set_profile))
        .route("/api/agents/:id/retire", post(retire_agent))
        // projects
        .route("/api/projects", get(projects).post(create_project))
        .route("/api/projects/:id", get(project_detail).patch(patch_project))
        .route("/api/projects/:id/members", put(set_members))
        .route("/api/projects/:id/decisions", post(propose_decision))
        .route("/api/projects/:id/git", get(project_git_status))
        .route("/api/projects/:id/diff", get(project_diff))
        .route("/api/exec-modes", get(exec_modes).put(exec_modes_put))
        .route("/api/runner/poll", post(runner_poll))
        .route("/api/runner/job/:id/event", post(runner_event))
        .route("/api/runner/job/:id/ask", post(runner_ask))
        .route("/api/runner/skills", get(runner_skills))
        .route("/api/cloud", get(cloud_list).delete(cloud_delete))
        .route("/api/cloud/config", get(cloud_config).put(cloud_config_put))
        .route("/api/cloud/upload", post(cloud_upload).layer(DefaultBodyLimit::max(120 * 1024 * 1024)))
        .route("/api/cloud/folder", post(cloud_folder))
        .route("/api/cloud/link", get(cloud_link))
        .route("/api/threads/:id/git", get(thread_git_status))
        .route("/api/threads/:id/diff", get(thread_diff))
        .route("/api/decisions/:id/status", post(decision_status))
        // obligations
        .route("/api/obligations", get(obligations).post(create_obligation))
        .route("/api/obligations/:id", get(obligation_detail).patch(patch_obligation))
        .route("/api/obligations/:id/transition", post(transition))
        .route("/api/obligations/:id/answer", post(answer_obligation))
        .route("/api/obligations/:id/reassign", post(reassign))
        // memory
        .route("/api/memory", get(memory).post(add_memory))
        .route("/api/memory/:id/verify", post(verify_memory))
        .route("/api/memory/:id/promote", post(promote_memory))
        .route("/api/memory/:id/correct", post(correct_memory))
        .route("/api/memory/:id/archive", post(archive_memory))
        // authority
        .route("/api/approvals", get(approvals))
        .route("/api/approvals/:id/decide", post(decide_approval))
        .route("/api/grants", get(grants).put(set_grant))
        // environment
        .route("/api/nodes", get(|State(a): S| async move { ok(a.store.list_nodes()?) }))
        .route("/api/settings", get(settings).put(put_settings))
        .route("/api/fs/list", get(fs_list))
        .route("/api/journal", get(journal))
        // forum: specialists discuss a broad question and plan the work
        .route("/api/forums", post(create_forum))
        .route("/api/forums/:id/stop", post(forum_stop))
        .route("/api/forums/:id/conclude", post(forum_conclude))
        .route("/api/forums/:id/apply", post(forum_apply))
        // agent accounts (login like in Shar: button, link and code, status)
        .route("/api/accounts", get(|State(a): S| async move { ok(accounts::view(&a)) }))
        .route("/api/accounts/:p/login", post(account_login).delete(account_login_cancel))
        .route("/api/accounts/:p/login/code", post(account_login_code))
        .route("/api/accounts/:p/open", post(account_open))
        .route("/api/accounts/:p/check", post(account_check))
        .route("/api/accounts/:p", axum::routing::delete(account_logout))
        // companies (clusters of projects)
        .route("/api/companies", get(companies).post(create_company))
        .route("/api/companies/:id", get(company_detail).patch(patch_company))
        .route("/api/companies/:id/decisions", post(company_decision))
        // MCP servers and API connections (separate tabs; owned by specialists)
        .route("/api/mcp", get(mcp_list).post(mcp_save))
        .route("/api/mcp/:id", axum::routing::patch(mcp_update).delete(mcp_delete))
        .route("/api/mcp/:id/test", post(mcp_test))
        .route("/api/integrations", get(integrations_list).post(integration_save))
        .route("/api/integrations/:id", axum::routing::patch(integration_update).delete(integration_delete))
        .route("/api/integrations/:id/test", post(integration_test))
        // character sheet of a specialist
        .route("/api/agents/:id/sheet", get(agent_sheet))
        .route("/api/agents/:id/equipment", put(agent_equipment))
        // agent tools bridge (session token)
        .route("/api/agent/call", post(agent_call))
        .route("/mcp", post(crate::mcp_http::post_root).get(crate::mcp_http::get_not_allowed))
        .route("/mcp/:token", post(crate::mcp_http::post_token).get(crate::mcp_http::get_not_allowed));

    api.fallback(static_ui)
        .layer(middleware::from_fn_with_state(app.clone(), auth))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(app)
}

// ---------------- state & events ----------------

/// Who is working right now, and on what: the right-hand panel. One line per running or waiting session, taken
/// from the message that session is writing (phase, last step, plan) so the panel needs nothing else.
async fn live(State(a): S) -> R {
    let mut out: Vec<Value> = vec![];
    for status in [SessionStatus::Running, SessionStatus::Queued] {
        for s in a.store.list_sessions(None, None, Some(status), 50)? {
            let msg = s.message_id.as_deref().and_then(|m| a.store.get_message(m).ok());
            let props = msg.as_ref().map(|m| m.props.clone()).unwrap_or(Value::Null);
            let steps = props["steps"].as_array().cloned().unwrap_or_default();
            let hint = steps.iter().rev().find(|x| x["kind"] == "tool").and_then(|x| x["hint"].as_str()).unwrap_or("").to_string();
            let todos = steps.iter().rev().find(|x| x["name"] == "TodoWrite").and_then(|x| x["input"]["todos"].as_array()).cloned().unwrap_or_default();
            let done = todos.iter().filter(|t| t["status"] == "completed").count();
            let current = todos.iter().find(|t| t["status"] == "in_progress").and_then(|t| t["activeForm"].as_str().or(t["content"].as_str())).unwrap_or("").to_string();
            let (kind, title) = match (&s.thread_id, &s.obligation_id) {
                (Some(t), _) => ("chat", a.store.get_thread(t).map(|t| t.title).unwrap_or_default()),
                (None, Some(o)) => ("task", a.store.get_obligation(o).map(|o| o.title).unwrap_or_default()),
                _ => ("run", String::new()),
            };
            out.push(json!({
                "session_id": s.id, "agent_id": s.agent_id, "kind": kind, "title": title,
                "thread_id": s.thread_id, "obligation_id": s.obligation_id, "message_id": s.message_id,
                "status": s.status.as_str(), "since": s.started_at.unwrap_or(s.created_at),
                "phase": props["phase"].as_str().unwrap_or(""), "hint": hint,
                "awaiting": props["ask"]["state"] == "pending",
                "plan": if todos.is_empty() { Value::Null } else { json!({"done": done, "total": todos.len(), "current": current}) },
                "backend": props["backend"],
            }));
        }
    }
    ok(out)
}

async fn state(State(a): S) -> R {
    let caps = a.caps.read().unwrap().clone();
    ok(json!({
        "agents": agents_view(&a)?,
        "projects": a.store.list_projects()?,
        "companies": a.store.list_companies()?,
        "accounts": accounts::view(&a),
        "threads": thread_views(&a, None, false)?,
        "attention": a.store.attention()?,
        "nodes": a.store.list_nodes()?,
        "backends": caps,
        "default_backend": a.backend_default(),
        "node_id": a.node_id,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn events(State(a): S) -> Sse<impl futures_util::Stream<Item = Result<SseEvent, std::convert::Infallible>>> {
    let stream = BroadcastStream::new(a.bus.subscribe()).filter_map(|m| async move { m.ok().map(|s| Ok(SseEvent::default().data(s))) });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}

// ---------------- chats ----------------

fn thread_views(a: &App, project: Option<&str>, archived: bool) -> Result<Vec<Value>, Error> {
    let mut out = vec![];
    for t in a.store.list_threads(project, archived)? {
        let last = a.store.thread_last_message(&t.id)?;
        let (text, role, working) = match &last {
            Some(m) => (m.body.chars().take(140).collect::<String>(), m.author_kind.clone(), matches!(m.status, MessageStatus::Queued | MessageStatus::Working)),
            None => (String::new(), String::new(), false),
        };
        let mut v = serde_json::to_value(&t).unwrap_or_default();
        v["last_text"] = json!(text);
        v["last_role"] = json!(role);
        v["working"] = json!(working);
        let ask_pending = last.as_ref().map(|m| m.props.get("ask").map(|k| k["state"] == "pending").unwrap_or(false)).unwrap_or(false);
        let plan_waiting = t.kind == ThreadKind::Forum && !t.meta["plan"].is_null() && t.meta["plan_applied"] != true;
        v["awaiting"] = json!(ask_pending || plan_waiting);
        v["working"] = json!(working || (t.kind == ThreadKind::Forum && matches!(t.meta["status"].as_str(), Some("open" | "summarizing"))));
        out.push(v);
    }
    Ok(out)
}

async fn threads(State(a): S, Query(q): Query<HashMap<String, String>>) -> R {
    ok(thread_views(&a, q.get("project").map(|s| s.as_str()), q.get("archived").map(|v| v == "1" || v == "true").unwrap_or(false))?)
}

async fn create_thread(State(a): S, Json(b): Json<Value>) -> R {
    let n: NewThread = de(b)?;
    let t = a.store.create_thread(n)?;
    a.publish("thread", json!({"id": t.id}));
    ok(t)
}

async fn get_thread(State(a): S, Path(id): Path<String>) -> R {
    ok(a.store.get_thread(&id)?)
}

async fn patch_thread(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let p: ThreadPatch = de(b)?;
    let t = a.store.patch_thread(&id, p)?;
    a.publish("thread", json!({"id": t.id}));
    ok(t)
}

async fn messages(State(a): S, Path(id): Path<String>, Query(q): Query<HashMap<String, String>>) -> R {
    let mut list = a.store.list_messages(&id, q.get("before").map(|s| s.as_str()), 61)?;
    let has_more = list.len() > 60;
    if has_more {
        list.remove(0);
    }
    ok(json!({"messages": list, "has_more": has_more}))
}

async fn send(State(a): S, Path(id): Path<String>, Json(mut b): Json<Value>) -> R {
    b["thread_id"] = json!(id);
    let mut req: ChatSend = de(b)?;
    // The client names files by id only; what they are and where they lie is the node's own knowledge.
    req.attachments = uploads::resolve(&a.cfg.data_dir, &req.attachments);
    if req.text.trim().is_empty() && req.attachments.is_empty() {
        return Err(bad("пустое сообщение"));
    }
    let d = a.store.send_chat(req, &a.backend_default())?;
    if a.store.get_thread(&id)?.kind == ThreadKind::Forum {
        forum::on_user_message(&a, &id);
    }
    a.publish("message", serde_json::to_value(&d.user_message).unwrap_or_default());
    for m in &d.agent_messages {
        a.publish("message", serde_json::to_value(m).unwrap_or_default());
    }
    a.publish("thread", json!({"id": id}));
    a.poke();
    ok(d)
}

/// One file, sent as the raw request body; its name comes in `X-Filename` (percent-encoded).
async fn upload(State(a): S, headers: HeaderMap, body: Bytes) -> R {
    let raw_name = headers.get("x-filename").and_then(|v| v.to_str().ok()).map(uploads::percent_decode).unwrap_or_default();
    let ctype = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").split(';').next().unwrap_or("").trim().to_lowercase();
    let v = uploads::store(&a.cfg.data_dir, &raw_name, &ctype, &body).await.map_err(|e| bad(&e))?;
    // `path` is for the agent; the UI has no use for where the file lies
    let mut out = v.clone();
    out.as_object_mut().map(|o| o.remove("path"));
    ok(out)
}

async fn serve_upload(State(a): S, Path((id, name)): Path<(String, String)>) -> Response {
    let Some(path) = uploads::locate(&a.cfg.data_dir, &id, &name) else { return (StatusCode::NOT_FOUND, "нет такого файла").into_response() };
    let Ok(data) = tokio::fs::read(&path).await else { return (StatusCode::NOT_FOUND, "нет такого файла").into_response() };
    let (ctype, inline) = uploads::kind(&name);
    let disposition = if inline { "inline".to_string() } else { format!("attachment; filename*=UTF-8''{}", uploads::percent_encode(&name)) };
    (
        [
            (header::CONTENT_TYPE, ctype.to_string()),
            (header::CONTENT_DISPOSITION, disposition),
            // never let the browser guess a more dangerous type than the one we chose
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            (header::CACHE_CONTROL, "private, max-age=31536000, immutable".to_string()),
        ],
        data,
    )
        .into_response()
}

// ---------------- accesses (servers, databases, sites the person lets specialists use) ----------

async fn accesses_list(State(a): S) -> R {
    ok(a.store.list_accesses()?.iter().map(|m| m.view()).collect::<Vec<_>>())
}
async fn access_save(State(a): S, Json(b): Json<Value>) -> R {
    let v = a.store.save_access(None, de(b)?)?.view();
    a.publish("accesses", json!({}));
    ok(v)
}
async fn access_update(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let v = a.store.save_access(Some(&id), de(b)?)?.view();
    a.publish("accesses", json!({}));
    ok(v)
}
async fn access_delete(State(a): S, Path(id): Path<String>) -> R {
    a.store.delete_access(&id)?;
    a.publish("accesses", json!({}));
    ok(json!({"ok": true}))
}
/// Who took it, when and what for. Never the secret itself.
async fn access_log(State(a): S, Path(id): Path<String>) -> R {
    let acc = a.store.get_access(&id)?;
    ok(a.store.access_log(&acc.id, 60)?.into_iter().map(|e| json!({"at": e.ts, "kind": e.kind, "actor": e.actor, "data": e.payload})).collect::<Vec<_>>())
}

// ---------------- routines (work on a schedule) ----------------

fn routine_view(a: &App, r: &Routine) -> Value {
    let mut v = serde_json::to_value(r).unwrap_or_default();
    v["agent_name"] = json!(a.store.get_agent(&r.agent_id).map(|x| x.name).unwrap_or_default());
    v
}
/// `?from=<ms>&to=<ms>`: the plan of the schedules, what they did, deadlines. The offset says how to read the cron.
async fn calendar(State(a): S, Query(q): Query<HashMap<String, String>>) -> R {
    let now = senate_core::now_ms();
    let from: i64 = q.get("from").and_then(|v| v.parse().ok()).unwrap_or(now - 7 * 86_400_000);
    let to: i64 = q.get("to").and_then(|v| v.parse().ok()).unwrap_or(now + 35 * 86_400_000);
    if to <= from || to - from > 400 * 86_400_000 {
        return Err(bad("период: от 1 до 400 дней"));
    }
    ok(a.store.calendar(from, to, skills::offset_secs())?)
}
async fn routines_list(State(a): S, Query(q): Query<HashMap<String, String>>) -> R {
    ok(a.store.list_routines(q.get("agent").map(String::as_str))?.iter().map(|r| routine_view(&a, r)).collect::<Vec<_>>())
}
async fn routine_save(State(a): S, Json(b): Json<Value>) -> R {
    let r = a.store.save_routine(None, de(b)?, skills::offset_secs())?;
    a.publish("routines", json!({}));
    ok(routine_view(&a, &r))
}
async fn routine_update(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let r = a.store.save_routine(Some(&id), de(b)?, skills::offset_secs())?;
    a.publish("routines", json!({}));
    ok(routine_view(&a, &r))
}
async fn routine_delete(State(a): S, Path(id): Path<String>) -> R {
    a.store.delete_routine(&id)?;
    a.publish("routines", json!({}));
    ok(json!({"ok": true}))
}
/// Run it now, without touching the schedule.
async fn routine_run(State(a): S, Path(id): Path<String>) -> R {
    let o = a.store.run_routine(&id, senate_core::now_ms(), skills::offset_secs())?;
    a.publish("attention", json!({}));
    a.poke();
    ok(json!({"obligation": o.id, "title": o.title}))
}

/// "Never ask me about this": the same rules the card's "Allow always" makes, set directly. A tool rule may end with *.
async fn permission_rule_add(State(a): S, Json(b): Json<Value>) -> R {
    let kind = b["kind"].as_str().unwrap_or("");
    let value = b["value"].as_str().unwrap_or("").trim();
    if kind == "cmd" && value.is_empty() {
        return Err(bad("укажите начало команды"));
    }
    let label = b["label"].as_str().filter(|l| !l.is_empty()).map(String::from).unwrap_or_else(|| match kind {
        "tool" => format!("Инструмент {value}"),
        "cmd" => format!("Команды, начинающиеся с «{value}»"),
        _ => format!("Запись в файлы папки {value}"),
    });
    ok(a.store.add_permission_rule(kind, value, &label)?)
}

async fn permission_rule_delete(State(a): S, Path(id): Path<String>) -> R {
    a.store.delete_permission_rule(&id)?;
    ok(json!({"ok": true}))
}

async fn answer(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let allow = b["decision"].as_str() == Some("allow");
    let note = b["note"].as_str().unwrap_or("").to_string();
    let pending = a.pending.lock().unwrap().remove(&id);
    let Some(p) = pending else { return Err(ApiError(StatusCode::CONFLICT, "вопрос уже закрыт".into())) };
    // "Allow always": remember what the card said it would remember — never more, and never for a refusal.
    if allow && b["always"] == true {
        if let Some(c) = &p.rule {
            a.store.add_permission_rule(c.kind, &c.value, &c.label)?;
        }
    }
    let _ = p.tx.send(if allow { Decision::Allow } else { Decision::Deny(note) });
    ok(json!({"ok": true, "tool_use_id": p.tool_use_id}))
}

/// "Запомнить главное": run reflection on an answer now (also for conversations that happened before).
async fn reflect_message(State(a): S, Path(id): Path<String>) -> R {
    let m = a.store.get_message(&id)?;
    let sid = m.props.get("sessionId").and_then(|v| v.as_str()).map(String::from).or_else(|| {
        a.store.list_sessions(None, None, None, 2000).ok().and_then(|l| l.into_iter().find(|s| s.message_id.as_deref() == Some(&id)).map(|s| s.id))
    });
    let Some(sid) = sid else { return Err(bad("у этого сообщения нет сессии")) };
    crate::reflect::reflect_and_publish(a.clone(), sid, true).await;
    ok(a.store.get_message(&id)?)
}

fn stop_message(a: &App, id: &str) -> Result<bool, Error> {
    let m = a.store.get_message(id)?;
    let sid = m.props.get("sessionId").and_then(|v| v.as_str()).map(String::from).or_else(|| {
        a.store.list_sessions(None, None, None, 500).ok().and_then(|l| l.into_iter().find(|s| s.message_id.as_deref() == Some(id)).map(|s| s.id))
    });
    let stopped = sid.map(|s| executor::cancel_session(a, &s)).unwrap_or(false);
    // Free a waiting permission question too, so the CLI can unwind.
    if let Some(p) = a.pending.lock().unwrap().remove(id) {
        let _ = p.tx.send(Decision::Deny("Остановлено.".into()));
    }
    Ok(stopped)
}

async fn cancel(State(a): S, Path(id): Path<String>) -> R {
    ok(json!({"stopped": stop_message(&a, &id)?}))
}

/// "I do not agree with this plan": everyone working in the chat is stopped, the plan is kept, and the
/// person's next message reaches the specialist as an objection to it.
async fn plan_pause(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    a.store.get_thread(&id)?;
    let live: Vec<String> = a
        .store
        .list_messages(&id, None, 200)?
        .into_iter()
        .filter(|m| m.author_kind == "agent" && matches!(m.status, MessageStatus::Working | MessageStatus::Queued))
        .map(|m| m.id)
        .collect();
    let mut stopped = 0;
    for m in &live {
        if stop_message(&a, m).unwrap_or(false) {
            stopped += 1;
        }
    }
    let hold = a.store.hold_plan(&id, &b["todos"])?;
    a.publish("thread", json!({"id": id}));
    ok(json!({"stopped": stopped, "hold": hold}))
}

async fn plan_resume(State(a): S, Path(id): Path<String>) -> R {
    a.store.release_plan(&id)?;
    a.publish("thread", json!({"id": id}));
    ok(json!({"ok": true}))
}

// ---------------- specialists ----------------

/// The specialists as the UI shows them: the profile plus level and open work. One source for `/api/state`
/// and `/api/agents`, so a screen opened fresh shows the same numbers as one that was refreshed.
fn agents_view(a: &App) -> Result<Vec<Value>, Error> {
    let mut out = vec![];
    for ag in a.store.list_agents(false)? {
        let open = a.store.list_obligations(Some(&ag.id), None, true)?;
        let mut v = serde_json::to_value(&ag).unwrap_or_default();
        v["open_obligations"] = json!(open.len());
        v["running"] = json!(open.iter().filter(|o| o.state == ObligationState::Running).count());
        v["blocked"] = json!(open.iter().filter(|o| o.state == ObligationState::Blocked).count());
        let st = a.store.agent_stats(&ag.id)?;
        v["level"] = json!(st["level"]);
        v["xp"] = json!(st["xp"]);
        v["next_level_xp"] = json!(st["next_level_xp"]);
        out.push(v);
    }
    Ok(out)
}

async fn agents(State(a): S) -> R {
    ok(agents_view(&a)?)
}

async fn create_agent(State(a): S, Json(b): Json<Value>) -> R {
    let n: NewAgent = de(b)?;
    let ag = a.store.create_agent(n)?;
    a.publish("agents", json!({}));
    ok(ag)
}

async fn agent_detail(State(a): S, Path(id): Path<String>) -> R {
    let ag = a.store.get_agent(&id)?;
    ok(json!({
        "agent": ag,
        "projects": a.store.agent_projects(&ag.id)?,
        "obligations": a.store.list_obligations(Some(&ag.id), None, false)?.into_iter().take(40).collect::<Vec<_>>(),
        "memory": a.store.list_memory(Some(MemoryLevel::Personal), Some(&ag.id), None, false, 60)?,
        "profile_versions": a.store.profile_versions(&ag.id)?,
        "sessions": a.store.list_sessions(Some(&ag.id), None, None, 20)?,
    }))
}

async fn patch_agent(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let name = b["name"].as_str().map(str::trim);
    if let Some(n) = name {
        if n.is_empty() || n.chars().count() > 60 || n.chars().any(|c| c.is_control()) {
            return Err(bad("имя: от 1 до 60 символов"));
        }
    }
    let mut ag = a.store.update_agent_basics(&id, name, b["accent"].as_str(), b["model_pref"].as_str())?;
    // The username is what `@` addresses and what the specialist's folders are named after: they move with it.
    if let Some(wanted) = b["slug"].as_str() {
        let (renamed, old) = a.store.set_agent_slug(&ag.id, wanted)?;
        if old != renamed.slug {
            for dir in ["skills", "workspaces"] {
                let (from, to) = (a.cfg.data_dir.join(dir).join(&old), a.cfg.data_dir.join(dir).join(&renamed.slug));
                if from.is_dir() && !to.exists() {
                    let _ = std::fs::rename(&from, &to);
                }
            }
        }
        ag = renamed;
    }
    if let Some(g) = b["gender"].as_str() {
        ag = a.store.set_agent_gender(&ag.id, g)?;
    }
    // The picture arrives as an upload id; the address stored is the node's own, never the client's.
    if let Some(v) = b.get("avatar_id") {
        let url = match v.as_str().filter(|s| !s.is_empty()) {
            None => String::new(),
            Some(up) => {
                let found = uploads::resolve(&a.cfg.data_dir, &[json!({"id": up})]);
                let f = found.first().ok_or_else(|| bad("такого файла нет — загрузите картинку заново"))?;
                if f["type"].as_str().map(|t| !t.starts_with("image/")).unwrap_or(true) {
                    return Err(bad("аватар — это картинка (png, jpg, gif или webp)"));
                }
                f["url"].as_str().unwrap_or_default().to_string()
            }
        };
        ag = a.store.set_agent_avatar(&id, &url)?;
    }
    a.publish("agents", json!({}));
    ok(ag)
}

async fn set_profile(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let ag = a.store.update_profile(&id, b["profile"].as_str().unwrap_or(""), b["note"].as_str().unwrap_or("правка пользователя"), "user")?;
    ok(ag)
}

async fn retire_agent(State(a): S, Path(id): Path<String>) -> R {
    a.store.retire_agent(&id)?;
    a.publish("agents", json!({}));
    ok(json!({"ok": true}))
}

// ---------------- projects ----------------

async fn projects(State(a): S) -> R {
    let mut out = vec![];
    for p in a.store.list_projects()? {
        let obl = a.store.list_obligations(None, Some(&p.id), true)?;
        let mut v = serde_json::to_value(&p).unwrap_or_default();
        v["open_obligations"] = json!(obl.len());
        v["members"] = json!(a.store.project_members(&p.id)?);
        out.push(v);
    }
    ok(out)
}

async fn create_project(State(a): S, Json(b): Json<Value>) -> R {
    let mut n: NewProject = de(b)?;
    if n.members.is_empty() {
        n.members = a.store.list_agents(false)?.into_iter().map(|x| x.id).collect();
    }
    let p = a.store.create_project(n)?;
    a.publish("projects", json!({}));
    ok(p)
}

async fn project_detail(State(a): S, Path(id): Path<String>) -> R {
    let p = a.store.get_project(&id)?;
    ok(json!({
        "project": p,
        "members": a.store.project_members(&id)?,
        "decisions": a.store.list_decisions(&id)?,
        "obligations": a.store.list_obligations(None, Some(&id), false)?.into_iter().take(80).collect::<Vec<_>>(),
        "artifacts": a.store.list_artifacts(Some(&id))?,
        "threads": thread_views(&a, Some(&id), false)?,
        "memory": a.store.list_memory(Some(MemoryLevel::Project), None, Some(&id), false, 60)?,
    }))
}

async fn patch_project(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let p: ProjectPatch = de(b)?;
    if let Some(r) = &p.root_path {
        if !r.is_empty() && !std::path::Path::new(r).is_dir() {
            return Err(bad("такой папки нет"));
        }
    }
    let p = a.store.update_project(&id, p)?;
    a.publish("projects", json!({}));
    ok(p)
}

async fn set_members(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let ids: Vec<String> = de(b["agents"].clone())?;
    a.store.set_project_members(&id, &ids)?;
    ok(json!({"ok": true}))
}

async fn propose_decision(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let d = a.store.propose_decision(&id, b["title"].as_str().unwrap_or(""), b["body"].as_str().unwrap_or(""), "user", b["supersedes"].as_str())?;
    ok(d)
}

async fn decision_status(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let st = DecisionStatus::parse(b["status"].as_str().unwrap_or("")).ok_or_else(|| bad("неизвестный статус"))?;
    ok(a.store.set_decision_status(&id, st, "user")?)
}

fn project_root(a: &App, id: &str) -> Result<std::path::PathBuf, ApiError> {
    let p = a.store.get_project(id)?;
    if p.root_path.is_empty() {
        return Err(bad("у проекта нет рабочей папки"));
    }
    Ok(std::path::PathBuf::from(p.root_path))
}

/// The folder a chat works in: its project's folder, or the private workspace of the specialist who answers it.
fn thread_root(a: &App, id: &str) -> Option<std::path::PathBuf> {
    let t = a.store.get_thread(id).ok()?;
    if let Some(pid) = &t.project_id {
        if let Ok(p) = a.store.get_project(pid) {
            if !p.root_path.is_empty() {
                return Some(std::path::PathBuf::from(p.root_path));
            }
        }
    }
    let agent_id = t.agent_id.clone().or_else(|| {
        a.store.list_messages(id, None, 20).ok()?.iter().rev().find(|m| m.author_kind != "user").and_then(|m| m.author_id.clone())
    })?;
    let ag = a.store.get_agent(&agent_id).ok()?;
    let dir = a.cfg.data_dir.join("workspaces").join(&ag.slug);
    dir.is_dir().then_some(dir)
}

async fn thread_git_status(State(a): S, Path(id): Path<String>) -> R {
    let Some(root) = thread_root(&a, &id) else { return ok(json!({"available": false, "reason": "у чата нет рабочей папки"})) };
    ok(tokio::task::spawn_blocking(move || project_git::status(&root)).await.unwrap_or(Value::Null))
}

async fn thread_diff(State(a): S, Path(id): Path<String>, Query(q): Query<HashMap<String, String>>) -> R {
    let root = thread_root(&a, &id).ok_or_else(|| bad("у чата нет рабочей папки"))?;
    let path = q.get("path").cloned();
    if let Some(p) = &path {
        if p.contains("..") || std::path::Path::new(p).is_absolute() {
            return Err(bad("недопустимый путь"));
        }
    }
    let d = tokio::task::spawn_blocking(move || project_git::diff(&root, path.as_deref())).await.unwrap_or_default();
    ok(json!({"diff": d}))
}

async fn project_git_status(State(a): S, Path(id): Path<String>) -> R {
    let root = match project_root(&a, &id) {
        Ok(r) => r,
        Err(_) => return ok(json!({"available": false, "reason": "у проекта нет рабочей папки"})),
    };
    let v = tokio::task::spawn_blocking(move || project_git::status(&root)).await.unwrap_or(Value::Null);
    ok(v)
}

async fn project_diff(State(a): S, Path(id): Path<String>, Query(q): Query<HashMap<String, String>>) -> R {
    let root = project_root(&a, &id)?;
    let path = q.get("path").cloned();
    if let Some(p) = &path {
        // never let a diff request leave the project folder
        if p.contains("..") || std::path::Path::new(p).is_absolute() {
            return Err(bad("недопустимый путь"));
        }
    }
    let d = tokio::task::spawn_blocking(move || project_git::diff(&root, path.as_deref())).await.unwrap_or_default();
    ok(json!({"diff": d}))
}

// ---------------- where specialists work ----------------

async fn exec_modes(State(a): S) -> R {
    let mut modes = serde_json::Map::new();
    for ag in a.store.list_agents(false)? {
        let m = a.store.setting(&format!("exec:{}", ag.id))?.unwrap_or_default();
        modes.insert(ag.id, json!(if m == "local" { "local" } else { "node" }));
    }
    ok(json!({"modes": modes, "runner_online": a.runner.online()}))
}

async fn exec_modes_put(State(a): S, Json(b): Json<Value>) -> R {
    let id = b["agent_id"].as_str().unwrap_or("");
    let mode = b["mode"].as_str().unwrap_or("");
    if !matches!(mode, "local" | "node") {
        return Err(bad("режим: local или node"));
    }
    a.store.get_agent(id)?;
    a.store.set_setting(&format!("exec:{id}"), mode)?;
    a.publish("agents", json!({}));
    exec_modes(State(a)).await
}

async fn runner_poll(State(a): S) -> R {
    let job = a.runner.poll().await;
    ok(json!({"job": job}))
}

async fn runner_event(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    crate::runner::job_event(&a, &id, b).await.map(Json).map_err(|e| bad(&e))
}

async fn runner_ask(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    crate::runner::job_ask(&a, &id, b).await.map(Json).map_err(|e| bad(&e))
}

async fn runner_skills(State(a): S, Query(q): Query<HashMap<String, String>>) -> R {
    let slug = q.get("slug").cloned().unwrap_or_default();
    if slug.is_empty() || !slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(bad("недопустимый slug"));
    }
    ok(crate::runner::skills_files(&a, &slug))
}

// ---------------- cloud (S3) ----------------

fn cloud_cfg(a: &App) -> Result<crate::s3::Cfg, ApiError> {
    crate::s3::load(&a.store).ok_or_else(|| bad("облако не настроено: укажите адрес, бакет и ключи в настройках облака"))
}
fn cloud_key(q: &HashMap<String, String>, name: &str) -> Result<String, ApiError> {
    crate::s3::safe_key(q.get(name).map(|s| s.as_str()).unwrap_or("")).ok_or_else(|| bad("недопустимый путь"))
}

async fn cloud_config(State(a): S) -> R {
    let g = |k: &str| a.store.setting(k).ok().flatten().unwrap_or_default();
    ok(json!({
        "configured": crate::s3::load(&a.store).is_some(),
        "endpoint": g("s3_endpoint"), "region": g("s3_region"), "bucket": g("s3_bucket"),
        "access_key": g("s3_access_key"), "prefix": g("s3_prefix"), "has_secret": !g("s3_secret_key").is_empty(),
    }))
}

async fn cloud_config_put(State(a): S, Json(b): Json<Value>) -> R {
    for (field, key) in [("endpoint", "s3_endpoint"), ("region", "s3_region"), ("bucket", "s3_bucket"), ("access_key", "s3_access_key"), ("prefix", "s3_prefix")] {
        if let Some(v) = b[field].as_str() {
            a.store.set_setting(key, v.trim())?;
        }
    }
    // an empty secret keeps the stored one
    if let Some(v) = b["secret_key"].as_str().filter(|v| !v.trim().is_empty()) {
        a.store.set_setting("s3_secret_key", v.trim())?;
    }
    cloud_config(State(a)).await
}

async fn cloud_list(State(a): S, Query(q): Query<HashMap<String, String>>) -> R {
    let Some(cfg) = crate::s3::load(&a.store) else { return ok(json!({"configured": false})) };
    let prefix = cloud_key(&q, "prefix")?;
    let (folders, files) = crate::s3::list(&cfg, &prefix).await.map_err(|e| bad(&e))?;
    let folders: Vec<Value> = folders.iter().map(|p| json!({"prefix": p, "name": p.trim_end_matches('/').rsplit('/').next().unwrap_or(p)})).collect();
    ok(json!({"configured": true, "prefix": prefix, "folders": folders, "files": files}))
}

async fn cloud_upload(State(a): S, Query(q): Query<HashMap<String, String>>, headers: HeaderMap, body: Bytes) -> R {
    let cfg = cloud_cfg(&a)?;
    let prefix = cloud_key(&q, "prefix")?;
    let name = q.get("name").cloned().unwrap_or_default();
    if name.is_empty() || name.contains('/') || name.contains('\\') || name == ".." {
        return Err(bad("недопустимое имя файла"));
    }
    let ctype = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_string();
    let key = format!("{prefix}{name}");
    crate::s3::put(&cfg, &key, body.to_vec(), &ctype).await.map_err(|e| bad(&e))?;
    ok(json!({"ok": true, "key": key, "size": body.len()}))
}

async fn cloud_folder(State(a): S, Json(b): Json<Value>) -> R {
    let cfg = cloud_cfg(&a)?;
    let mut q = HashMap::new();
    q.insert("p".to_string(), format!("{}{}", b["prefix"].as_str().unwrap_or(""), b["name"].as_str().unwrap_or("").trim().trim_matches('/')));
    let key = cloud_key(&q, "p")?;
    if key.is_empty() || key.ends_with("//") {
        return Err(bad("укажите название папки"));
    }
    crate::s3::put(&cfg, &format!("{key}/"), vec![], "application/x-directory").await.map_err(|e| bad(&e))?;
    ok(json!({"ok": true, "prefix": format!("{key}/")}))
}

async fn cloud_link(State(a): S, Query(q): Query<HashMap<String, String>>) -> R {
    let cfg = cloud_cfg(&a)?;
    let key = cloud_key(&q, "key")?;
    let mins: u64 = q.get("minutes").and_then(|m| m.parse().ok()).unwrap_or(60);
    let name = key.rsplit('/').next().map(String::from);
    ok(json!({"url": crate::s3::link(&cfg, &key, mins.clamp(1, 10080) * 60, name.as_deref())}))
}

async fn cloud_delete(State(a): S, Query(q): Query<HashMap<String, String>>) -> R {
    let cfg = cloud_cfg(&a)?;
    let key = cloud_key(&q, "key")?;
    if key.is_empty() {
        return Err(bad("нечего удалять"));
    }
    let n = if key.ends_with('/') { crate::s3::delete_folder(&cfg, &key).await.map_err(|e| bad(&e))? } else { crate::s3::delete(&cfg, &key).await.map_err(|e| bad(&e))?; 1 };
    ok(json!({"ok": true, "deleted": n}))
}

// ---------------- obligations ----------------

async fn obligations(State(a): S, Query(q): Query<HashMap<String, String>>) -> R {
    let all = q.get("all").map(|v| v == "1").unwrap_or(false);
    ok(a.store.list_obligations(q.get("owner").map(|s| s.as_str()), q.get("project").map(|s| s.as_str()), !all)?)
}

async fn create_obligation(State(a): S, Json(b): Json<Value>) -> R {
    let n: NewObligation = de(b)?;
    let o = a.store.create_obligation(n)?;
    a.publish("attention", json!({}));
    a.poke();
    ok(o)
}

async fn obligation_detail(State(a): S, Path(id): Path<String>) -> R {
    let o = a.store.get_obligation(&id)?;
    let thread = a.store.setting(&format!("obl-thread:{id}"))?;
    ok(json!({
        "obligation": o,
        "journal": a.store.events(Some("obligation"), Some(&id), 60)?,
        "sessions": a.store.list_sessions(None, Some(&id), None, 20)?,
        "children": a.store.list_obligations(None, o.project_id.as_deref(), false)?.into_iter().filter(|c| c.parent_id.as_deref() == Some(&id)).collect::<Vec<_>>(),
        "thread_id": thread,
    }))
}

async fn patch_obligation(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let due = if b.get("due_at").is_some() { Some(b["due_at"].as_i64()) } else { None };
    ok(a.store.update_obligation_fields(&id, b["title"].as_str(), b["result"].as_str(), b["criteria"].as_str(), b["next_step"].as_str(), due, "user")?)
}

async fn transition(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let to = ObligationState::parse(b["to"].as_str().unwrap_or("")).ok_or_else(|| bad("неизвестное состояние"))?;
    let wait = match b.get("wait") {
        Some(w) if !w.is_null() => Some(de::<WaitCondition>(w.clone())?),
        _ => None,
    };
    let o = a.store.transition(
        &id,
        to,
        TransitionArgs {
            wait,
            blocker: b["blocker"].as_str().map(String::from),
            outcome: b["outcome"].as_str().map(String::from),
            next_step: b["next_step"].as_str().map(String::from),
            note: b["note"].as_str().unwrap_or("").into(),
        },
        "user",
    )?;
    a.publish("attention", json!({}));
    a.poke();
    ok(o)
}

async fn answer_obligation(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let o = a.store.answer_obligation(&id, b["answer"].as_str().unwrap_or(""))?;
    a.publish("attention", json!({}));
    a.poke();
    ok(o)
}

async fn reassign(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let to = a.store.get_agent(b["to"].as_str().unwrap_or(""))?;
    ok(a.store.reassign_obligation(&id, &to.id, "user")?)
}

// ---------------- memory ----------------

async fn memory(State(a): S, Query(q): Query<HashMap<String, String>>) -> R {
    if let Some(query) = q.get("q").filter(|s| !s.trim().is_empty()) {
        return ok(a.store.search_memory(query, q.get("agent").map(|s| s.as_str()), q.get("project").map(|s| s.as_str()), 40)?);
    }
    let level = q.get("level").and_then(|l| MemoryLevel::parse(l));
    ok(a.store.list_memory_in(level, q.get("agent").map(|s| s.as_str()), q.get("project").map(|s| s.as_str()), q.get("company").map(|s| s.as_str()), q.get("inactive").is_some(), 200)?)
}

async fn add_memory(State(a): S, Json(mut b): Json<Value>) -> R {
    if b.get("source").is_none() {
        b["source"] = json!("user");
    }
    ok(a.store.add_memory(de(b)?)?)
}

async fn verify_memory(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let m = a.store.verify_memory(&id, b["basis"].as_str().unwrap_or(""), "user")?;
    crate::reflect::refresh_learned(&a, &id);
    ok(m)
}
async fn promote_memory(State(a): S, Path(id): Path<String>) -> R {
    ok(a.store.promote_memory(&id, "user")?)
}
async fn correct_memory(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let m = a.store.supersede_memory(&id, b["body"].as_str().unwrap_or(""), "user", "user")?;
    crate::reflect::refresh_learned(&a, &id);
    ok(m)
}
async fn archive_memory(State(a): S, Path(id): Path<String>) -> R {
    a.store.archive_memory(&id, "user")?;
    crate::reflect::refresh_learned(&a, &id);
    ok(json!({"ok": true}))
}

// ---------------- authority ----------------

async fn approvals(State(a): S, Query(q): Query<HashMap<String, String>>) -> R {
    ok(a.store.list_approvals(q.get("all").is_none())?)
}

async fn decide_approval(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let approve = b["approve"].as_bool().unwrap_or(false);
    let r = a.store.decide_approval(&id, approve)?;
    // An approved write to an external API is performed now, by the node, and reported in the chat.
    if approve && r.detail["tool"] == "senate_api_call" {
        let c = &r.detail["ctx"];
        let ctx = ToolCtx {
            agent_id: r.agent_id.clone(),
            session_id: c["session_id"].as_str().map(String::from),
            obligation_id: c["obligation_id"].as_str().map(String::from),
            thread_id: c["thread_id"].as_str().map(String::from),
            project_id: c["project_id"].as_str().map(String::from),
            company_id: c["company_id"].as_str().map(String::from),
            ..Default::default()
        };
        let line = match external::api_call(&a, &ctx, &r.detail["args"], true).await {
            Ok(res) => format!("Выполнено по вашему согласию: {} {} {} → {}", r.detail["args"]["method"].as_str().unwrap_or("POST"), r.detail["service"].as_str().unwrap_or(""), r.detail["args"]["path"].as_str().unwrap_or(""), res["status"]),
            Err(e) => format!("Не удалось выполнить запрос к {}: {e}", r.detail["service"].as_str().unwrap_or("API")),
        };
        if let Some(t) = &ctx.thread_id {
            if let Ok(m) = a.store.post_message(t, "system", None, &line, json!({}), MessageStatus::Done, None) {
                a.publish("message", serde_json::to_value(&m).unwrap_or_default());
            }
        }
    }
    a.publish("attention", json!({}));
    a.poke();
    ok(r)
}

async fn grants(State(a): S) -> R {
    ok(json!({"grants": a.store.list_grants()?, "actions": action::ALL}))
}

async fn set_grant(State(a): S, Json(b): Json<Value>) -> R {
    let mode = GrantMode::parse(b["mode"].as_str().unwrap_or("")).ok_or_else(|| bad("неизвестный режим"))?;
    a.store.set_grant(b["agent_id"].as_str(), b["project_id"].as_str(), b["action"].as_str().unwrap_or(""), mode)?;
    ok(json!({"ok": true}))
}

// ---------------- environment ----------------

async fn settings(State(a): S) -> R {
    ok(json!({
        "default_backend": a.store.setting("default_backend")?.unwrap_or_default(),
        "learn_level": crate::reflect::level(&a),
        "user_name": a.store.setting("user_name")?.unwrap_or_default(),
        "effective_backend": a.backend_default(),
        "data_dir": a.cfg.data_dir.to_string_lossy(),
        "backends": a.caps.read().unwrap().clone(),
    }))
}

async fn put_settings(State(a): S, Json(b): Json<Value>) -> R {
    if let Some(v) = b["default_backend"].as_str() {
        a.store.set_setting("default_backend", v)?;
    }
    if let Some(v) = b["learn_level"].as_str() {
        if !["off", "low", "medium", "high"].contains(&v) {
            return Err(bad("неизвестный уровень обучения"));
        }
        a.store.set_setting("learn_level", v)?;
    }
    if let Some(v) = b["user_name"].as_str() {
        // how the person wants to be addressed; goes into every specialist's prompt
        let v: String = v.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(60).collect();
        a.store.set_setting("user_name", &v)?;
    }
    if b["rescan"].as_bool().unwrap_or(false) {
        let caps = senate_runtime::discover().await;
        let _ = a.store.upsert_node(&a.node_id, "", std::env::consts::OS, "local", "", &json!(caps));
        *a.caps.write().unwrap() = caps;
    }
    settings(State(a)).await
}

async fn journal(State(a): S, Query(q): Query<HashMap<String, String>>) -> R {
    ok(a.store.events(q.get("type").map(|s| s.as_str()), q.get("id").map(|s| s.as_str()), 100)?)
}

/// Folder picker backing: lists sub-folders only.
async fn fs_list(Query(q): Query<HashMap<String, String>>) -> R {
    let path = q.get("path").cloned().unwrap_or_default();
    if path.is_empty() {
        let mut roots: Vec<Value> = vec![];
        #[cfg(windows)]
        for l in b'A'..=b'Z' {
            let p = format!("{}:\\", l as char);
            if std::path::Path::new(&p).exists() {
                roots.push(json!({"name": p, "path": p}));
            }
        }
        if let Some(h) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
            let h = std::path::PathBuf::from(h);
            roots.insert(0, json!({"name": "Домашняя папка", "path": h.to_string_lossy()}));
        }
        #[cfg(not(windows))]
        roots.push(json!({"name": "/", "path": "/"}));
        return ok(json!({"path": "", "parent": null, "entries": roots}));
    }
    let p = std::path::PathBuf::from(&path);
    let rd = std::fs::read_dir(&p).map_err(|e| bad(&format!("не открыть: {e}")))?;
    let mut entries: Vec<Value> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == "node_modules" || name == "$RECYCLE.BIN" || name == "System Volume Information" {
                return None;
            }
            Some(json!({"name": name, "path": e.path().to_string_lossy(), "git": e.path().join(".git").exists()}))
        })
        .collect();
    entries.sort_by(|a, b| a["name"].as_str().unwrap_or("").to_lowercase().cmp(&b["name"].as_str().unwrap_or("").to_lowercase()));
    ok(json!({"path": path, "parent": p.parent().map(|x| x.to_string_lossy().into_owned()), "entries": entries, "git": p.join(".git").exists()}))
}

// ---------------- agent tools bridge ----------------

async fn agent_call(State(a): S, req: Request) -> Response {
    let tok = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).map(String::from).unwrap_or_default();
    let ctx: Option<ToolCtx> = a.session_tokens.lock().unwrap().get(&tok).cloned();
    let Some(ctx) = ctx else { return ApiError(StatusCode::UNAUTHORIZED, "сессия не найдена".into()).into_response() };
    let body = match axum::body::to_bytes(req.into_body(), 1 << 20).await {
        Ok(b) => b,
        Err(_) => return bad("слишком большой запрос").into_response(),
    };
    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let name = v["name"].as_str().unwrap_or("").to_string();
    let args = v["args"].clone();
    if name == "senate_api_call" {
        return match external::api_call(&a, &ctx, &args, false).await {
            Ok(r) => Json(json!({"ok": true, "result": r})).into_response(),
            Err(e) => ApiError::from(e).into_response(),
        };
    }
    if name == "senate_image_generate" {
        return match crate::images::generate(&a, &ctx, &args).await {
            Ok(r) => Json(json!({"ok": true, "result": r})).into_response(),
            Err(e) => ApiError::from(e).into_response(),
        };
    }
    match tools::call(&a.store, &ctx, &name, &args) {
        Ok(r) => {
            a.publish("attention", json!({}));
            // The preview button lives in the chat's state: tell the UI to reload the thread.
            if name == "senate_preview" {
                if let Some(t) = &ctx.thread_id {
                    a.publish("thread", json!({"id": t}));
                }
            }
            Json(json!({"ok": true, "result": r})).into_response()
        }
        Err(e) => ApiError::from(e).into_response(),
    }
}

// ---------------- forum ----------------

async fn create_forum(State(a): S, Json(b): Json<Value>) -> R {
    let from = b["from_thread"].as_str().map(String::from);
    let n: NewForum = de(b)?;
    let t = a.store.create_forum(n)?;
    let names: Vec<String> = t.meta["participants"].as_array().map(|l| l.iter().filter_map(|x| x.as_str()).filter_map(|id| a.store.get_agent(id).ok()).map(|x| x.name).collect()).unwrap_or_default();
    // The brief is part of the record: subject, who takes part, how long it can last.
    a.store.post_message(
        &t.id,
        "system",
        None,
        &format!("Форум открыт. Вопрос: {}\nУчаствуют: {}. Раундов не больше {}.", t.meta["topic"].as_str().unwrap_or(""), names.join(", "), t.meta["rounds"]),
        json!({}),
        MessageStatus::Done,
        None,
    )?;
    // A broad question usually grows out of a chat: bring the last exchange along.
    if let Some(src) = from {
        let recent = a.store.list_messages(&src, None, 6)?;
        let ctx: Vec<String> = recent.iter().filter(|m| !m.body.trim().is_empty() && m.author_kind != "system").map(|m| format!("{}: {}", if m.author_kind == "user" { "Человек".to_string() } else { "Специалист".to_string() }, m.body.chars().take(800).collect::<String>())).collect();
        if !ctx.is_empty() {
            a.store.post_message(&t.id, "user", None, &format!("Контекст из чата:\n{}", ctx.join("\n\n")), json!({}), MessageStatus::Done, None)?;
        }
    }
    a.publish("thread", json!({"id": t.id}));
    forum::start(&a, &t.id);
    ok(a.store.get_thread(&t.id)?)
}

async fn forum_stop(State(a): S, Path(id): Path<String>) -> R {
    forum::stop(&a, &id);
    ok(json!({"ok": true}))
}
async fn forum_conclude(State(a): S, Path(id): Path<String>) -> R {
    forum::conclude(&a, &id);
    ok(json!({"ok": true}))
}
async fn forum_apply(State(a): S, Path(id): Path<String>) -> R {
    let r = forum::apply_plan(&a, &id).map_err(|e| ApiError(StatusCode::CONFLICT, e))?;
    a.publish("thread", json!({"id": id}));
    ok(r)
}

// ---------------- agent accounts ----------------

async fn account_login(State(a): S, Path(p): Path<String>) -> R {
    accounts::start_login(&a, &p).map_err(|e| bad(&e))?;
    ok(accounts::view(&a))
}
async fn account_login_cancel(State(a): S, Path(p): Path<String>) -> R {
    accounts::cancel_login(&a, &p);
    ok(accounts::view(&a))
}
async fn account_login_code(State(a): S, Path(p): Path<String>, Json(b): Json<Value>) -> R {
    let code = b["code"].as_str().unwrap_or("").trim().to_string();
    if code.is_empty() {
        return Err(bad("вставьте код со страницы"));
    }
    if !accounts::login_code(&a, &p, &code) {
        return Err(ApiError(StatusCode::CONFLICT, "вход не запущен — нажмите «Войти»".into()));
    }
    ok(json!({"ok": true}))
}
/// Open the chat's preview link in the browser of this machine (the desktop window cannot open
/// external links itself). Only the address stored by `senate_preview` is used, and it is checked
/// again here: a request cannot name a URL of its own.
async fn open_preview(State(a): S, Path(id): Path<String>) -> R {
    let t = a.store.get_thread(&id)?;
    let raw = t.meta["preview"]["url"].as_str().unwrap_or_default();
    let (url, _) = tools::local_preview_url(raw).ok_or_else(|| bad("у этого чата нет ссылки на локальный сервер"))?;
    accounts::open_url(&url).map_err(|e| bad(&e))?;
    ok(json!({"ok": true, "url": url}))
}

async fn account_open(State(a): S, Path(p): Path<String>) -> R {
    accounts::open_login_link(&a, &p).map_err(|e| bad(&e))?;
    ok(json!({"ok": true}))
}
async fn account_check(State(a): S, Path(p): Path<String>) -> R {
    accounts::refresh(&a, &p).await;
    ok(accounts::view(&a))
}
async fn account_logout(State(a): S, Path(p): Path<String>) -> R {
    accounts::logout(&a, &p).await;
    ok(accounts::view(&a))
}

// ---------------- companies ----------------

async fn companies(State(a): S) -> R {
    let mut out = vec![];
    for c in a.store.list_companies()? {
        let projects: Vec<Project> = a.store.list_projects()?.into_iter().filter(|p| p.company_id.as_deref() == Some(&c.id)).collect();
        let mut open = 0;
        for p in &projects {
            open += a.store.list_obligations(None, Some(&p.id), true)?.len();
        }
        let mut v = serde_json::to_value(&c).unwrap_or_default();
        v["projects"] = json!(projects.len());
        v["open_obligations"] = json!(open);
        out.push(v);
    }
    ok(out)
}

async fn create_company(State(a): S, Json(b): Json<Value>) -> R {
    let c = a.store.create_company(de(b)?)?;
    a.publish("projects", json!({}));
    ok(c)
}

async fn company_detail(State(a): S, Path(id): Path<String>) -> R {
    let c = a.store.get_company(&id)?;
    let projects: Vec<Project> = a.store.list_projects()?.into_iter().filter(|p| p.company_id.as_deref() == Some(&id)).collect();
    // Who has experience here: every specialist's own numbers for this company.
    let mut people = vec![];
    for ag in a.store.list_agents(false)? {
        let st = a.store.agent_stats(&ag.id)?;
        if let Some(row) = st["companies"].as_array().and_then(|l| l.iter().find(|x| x["id"] == json!(id))) {
            people.push(json!({"agent": ag, "own_memory": row["own_memory"], "done": row["done"], "projects": row["projects"]}));
        }
    }
    ok(json!({
        "company": c,
        "projects": projects,
        "decisions": a.store.list_company_decisions(&id)?,
        "memory": a.store.list_memory_in(Some(MemoryLevel::Company), None, None, Some(&id), false, 100)?,
        "threads": thread_views(&a, None, false)?.into_iter().filter(|t| t["company_id"] == json!(id) && t["project_id"].is_null()).collect::<Vec<_>>(),
        "people": people,
    }))
}

async fn patch_company(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let c = a.store.update_company(&id, b["name"].as_str(), b["brief"].as_str(), b["color"].as_str(), b["status"].as_str())?;
    a.publish("projects", json!({}));
    ok(c)
}

async fn company_decision(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    ok(a.store.propose_decision_in(None, Some(&id), b["title"].as_str().unwrap_or(""), b["body"].as_str().unwrap_or(""), "user", b["supersedes"].as_str())?)
}

// ---------------- MCP servers ----------------

async fn mcp_list(State(a): S) -> R {
    ok(a.store.list_mcp()?.iter().map(|m| m.view()).collect::<Vec<_>>())
}
async fn mcp_save(State(a): S, Json(b): Json<Value>) -> R {
    let m = a.store.save_mcp(None, de(b)?)?;
    a.publish("mcp", json!({}));
    ok(m.view())
}
async fn mcp_update(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let m = a.store.save_mcp(Some(&id), de(b)?)?;
    a.publish("mcp", json!({}));
    ok(m.view())
}
async fn mcp_delete(State(a): S, Path(id): Path<String>) -> R {
    a.store.delete_mcp(&id)?;
    ok(json!({"ok": true}))
}
async fn mcp_test(State(a): S, Path(id): Path<String>) -> R {
    let m = a.store.get_mcp(&id)?;
    ok(external::test_mcp(&m).await)
}

// ---------------- API connections ----------------

async fn integrations_list(State(a): S) -> R {
    ok(a.store.list_integrations()?.iter().map(|m| m.view()).collect::<Vec<_>>())
}
async fn integration_save(State(a): S, Json(b): Json<Value>) -> R {
    ok(a.store.save_integration(None, de(b)?)?.view())
}
async fn integration_update(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    ok(a.store.save_integration(Some(&id), de(b)?)?.view())
}
async fn integration_delete(State(a): S, Path(id): Path<String>) -> R {
    a.store.delete_integration(&id)?;
    ok(json!({"ok": true}))
}
async fn integration_test(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    let it = a.store.get_integration(&id)?;
    ok(external::test_integration(&it, b["path"].as_str().unwrap_or("")).await)
}

// ---------------- character sheet ----------------

async fn agent_sheet(State(a): S, Path(id): Path<String>) -> R {
    let ag = a.store.get_agent(&id)?;
    let mcp = a.store.list_mcp()?;
    let apis = a.store.list_integrations()?;
    ok(json!({
        "agent": ag,
        "stats": a.store.agent_stats(&ag.id)?,
        "projects": a.store.agent_projects(&ag.id)?,
        "obligations": a.store.list_obligations(Some(&ag.id), None, true)?.into_iter().take(12).collect::<Vec<_>>(),
        "memory": a.store.list_memory(Some(MemoryLevel::Personal), Some(&ag.id), None, false, 120)?,
        "profile_versions": a.store.profile_versions(&ag.id)?,
        "equipment": {
            "mcp": mcp.iter().map(|m| { let mut v = m.view(); v["equipped"] = json!(m.shared || m.agents.contains(&ag.id)); v["via_shared"] = json!(m.shared); v }).collect::<Vec<_>>(),
            "api": apis.iter().map(|m| { let mut v = m.view(); v["equipped"] = json!(m.shared || m.agents.contains(&ag.id)); v["via_shared"] = json!(m.shared); v }).collect::<Vec<_>>(),
        },
        "backends": a.caps.read().unwrap().clone(),
        "skills": skills::list(&a.cfg.data_dir, &ag.slug).iter().map(|k| json!({"name": k.name, "description": k.description, "dir": k.dir.to_string_lossy(), "routines": k.routines.len()})).collect::<Vec<_>>(),
        "routines": a.store.list_routines(Some(&ag.id))?.iter().map(|r| routine_view(&a, r)).collect::<Vec<_>>(),
    }))
}

async fn agent_equipment(State(a): S, Path(id): Path<String>, Json(b): Json<Value>) -> R {
    if let Some(l) = b.get("mcp") {
        a.store.set_agent_mcp(&id, &de::<Vec<String>>(l.clone())?)?;
    }
    if let Some(l) = b.get("api") {
        a.store.set_agent_integrations(&id, &de::<Vec<String>>(l.clone())?)?;
    }
    ok(json!({"ok": true}))
}

// ---------------- static UI ----------------

#[derive(rust_embed::RustEmbed)]
#[folder = "../../ui/dist"]
struct Ui;

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "json" | "webmanifest" => "application/json",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

async fn static_ui(req: Request) -> Response {
    let p = req.uri().path().trim_start_matches('/');
    let p = if p.is_empty() { "index.html" } else { p };
    let (file, name) = match Ui::get(p) {
        Some(f) => (f, p.to_string()),
        // SPA: unknown paths fall back to the app shell
        None => match Ui::get("index.html") {
            Some(f) => (f, "index.html".to_string()),
            None => return (StatusCode::NOT_FOUND, "UI не собран").into_response(),
        },
    };
    let cache = if name.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    ([(header::CONTENT_TYPE, mime(&name)), (header::CACHE_CONTROL, cache)], file.data.into_owned()).into_response()
}
