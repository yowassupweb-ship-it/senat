//! Senate as an MCP server for the outside world: an ordinary chat (Claude, ChatGPT) or VS Code connects to
//! `https://<node>/mcp` with the node token and can talk to specialists, put tasks on them and read what the
//! environment remembers. This is a different door from `mcp.rs`, which serves one running agent its own tools.
//!
//! Transport: MCP "streamable HTTP", answered with plain JSON (no server-initiated stream).

use crate::App;
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use senate_core::engine::ChatSend;
use senate_core::model::{MessageStatus, NewObligation, NewThread};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

type S = State<Arc<App>>;

fn tools() -> Value {
    json!([
        {"name": "senate_specialists", "description": "Список специалистов Senate: slug, имя, роль, занят ли.",
         "inputSchema": {"type": "object", "properties": {}}},
        {"name": "senate_ask", "description": "Написать специалисту и получить ответ. Без thread_id начинает новый чат; с thread_id продолжает прежний. Если ответ не успел прийти за wait_seconds, вернётся thread_id: прочитайте чат позже через senate_thread.",
         "inputSchema": {"type": "object", "required": ["specialist", "text"], "properties": {
            "specialist": {"type": "string", "description": "slug специалиста из senate_specialists"},
            "text": {"type": "string"},
            "thread_id": {"type": "string"},
            "wait_seconds": {"type": "integer", "description": "сколько ждать ответ, 0-170, по умолчанию 90"}}}},
        {"name": "senate_thread", "description": "Последние сообщения чата Senate.",
         "inputSchema": {"type": "object", "required": ["thread_id"], "properties": {"thread_id": {"type": "string"}, "limit": {"type": "integer"}}}},
        {"name": "senate_task_create", "description": "Поставить задачу специалисту (обязательство): название, ожидаемый результат, критерии.",
         "inputSchema": {"type": "object", "required": ["title"], "properties": {
            "title": {"type": "string"}, "owner": {"type": "string", "description": "slug ответственного"},
            "result": {"type": "string"}, "criteria": {"type": "string"}, "project": {"type": "string", "description": "название или id проекта"}}}},
        {"name": "senate_tasks", "description": "Задачи Senate. По умолчанию только открытые.",
         "inputSchema": {"type": "object", "properties": {"owner": {"type": "string"}, "all": {"type": "boolean"}}}},
        {"name": "senate_projects", "description": "Проекты Senate.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "senate_memory_search", "description": "Поиск по памяти Senate (знания специалистов и проектов).",
         "inputSchema": {"type": "object", "required": ["query"], "properties": {"query": {"type": "string"}, "limit": {"type": "integer"}}}}
    ])
}

fn text(v: impl ToString) -> Value {
    json!({"content": [{"type": "text", "text": v.to_string()}], "isError": false})
}
fn fail(msg: &str) -> Value {
    json!({"content": [{"type": "text", "text": msg}], "isError": true})
}
fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn agent_id(app: &App, slug: &str) -> Result<String, String> {
    let list = app.store.list_agents(false).map_err(|e| e.to_string())?;
    list.iter()
        .find(|a| a.slug == slug || a.id == slug || a.name.eq_ignore_ascii_case(slug))
        .map(|a| a.id.clone())
        .ok_or_else(|| format!("нет специалиста «{slug}»: список в senate_specialists"))
}

async fn call(app: &Arc<App>, name: &str, args: &Value) -> Value {
    match call_inner(app, name, args).await {
        Ok(v) => v,
        Err(e) => fail(&e),
    }
}

async fn call_inner(app: &Arc<App>, name: &str, a: &Value) -> Result<Value, String> {
    let e = |x: senate_core::Error| x.to_string();
    match name {
        "senate_specialists" => {
            let busy = app.store.list_agents(false).map_err(e)?;
            let v: Vec<Value> = busy.iter().map(|g| json!({"slug": g.slug, "name": g.name, "role": g.role, "status": g.status})).collect();
            Ok(text(serde_json::to_string_pretty(&v).unwrap_or_default()))
        }
        "senate_projects" => {
            let v: Vec<Value> = app.store.list_projects().map_err(e)?.iter().map(|p| json!({"id": p.id, "name": p.name, "status": p.status, "goal": clip(&p.goal, 300)})).collect();
            Ok(text(serde_json::to_string_pretty(&v).unwrap_or_default()))
        }
        "senate_tasks" => {
            let owner = match a["owner"].as_str().filter(|s| !s.is_empty()) {
                Some(s) => Some(agent_id(app, s)?),
                None => None,
            };
            let open_only = !a["all"].as_bool().unwrap_or(false);
            let v: Vec<Value> = app
                .store
                .list_obligations(owner.as_deref(), None, open_only)
                .map_err(e)?
                .iter()
                .take(100)
                .map(|o| json!({"id": o.id, "title": o.title, "state": o.state.as_str(), "owner_id": o.owner_id, "next_step": clip(&o.next_step, 200), "blocker": clip(&o.blocker, 200)}))
                .collect();
            Ok(text(serde_json::to_string_pretty(&v).unwrap_or_default()))
        }
        "senate_task_create" => {
            let title = a["title"].as_str().unwrap_or("").trim().to_string();
            if title.is_empty() {
                return Err("нужен title".into());
            }
            let owner_id = match a["owner"].as_str().filter(|s| !s.is_empty()) {
                Some(s) => Some(agent_id(app, s)?),
                None => None,
            };
            let project_id = match a["project"].as_str().filter(|s| !s.is_empty()) {
                Some(p) => app.store.list_projects().map_err(e)?.iter().find(|x| x.id == p || x.name.eq_ignore_ascii_case(p)).map(|x| x.id.clone()),
                None => None,
            };
            let o = app
                .store
                .create_obligation(NewObligation {
                    title,
                    result: a["result"].as_str().unwrap_or("").into(),
                    criteria: a["criteria"].as_str().unwrap_or("").into(),
                    owner_id,
                    project_id,
                    requested_by: "внешний клиент MCP".into(),
                    ..Default::default()
                })
                .map_err(e)?;
            app.publish("attention", json!({}));
            app.poke();
            Ok(text(format!("Задача создана: {} (id {})", o.title, o.id)))
        }
        "senate_memory_search" => {
            let q = a["query"].as_str().unwrap_or("");
            let n = a["limit"].as_i64().unwrap_or(8).clamp(1, 30);
            let v: Vec<Value> = app.store.search_memory(q, None, None, n).map_err(e)?.iter().map(|m| json!({"id": m.id, "kind": m.kind.as_str(), "body": clip(&m.body, 800)})).collect();
            Ok(text(serde_json::to_string_pretty(&v).unwrap_or_default()))
        }
        "senate_thread" => {
            let id = a["thread_id"].as_str().unwrap_or("");
            let n = a["limit"].as_i64().unwrap_or(10).clamp(1, 40);
            let ms = app.store.list_messages(id, None, n).map_err(e)?;
            let v: Vec<Value> = ms.iter().map(|m| json!({"from": m.author_kind, "status": m.status.as_str(), "text": clip(&m.body, 4000)})).collect();
            Ok(text(serde_json::to_string_pretty(&v).unwrap_or_default()))
        }
        "senate_ask" => {
            let slug = a["specialist"].as_str().unwrap_or("");
            let msg = a["text"].as_str().unwrap_or("").trim().to_string();
            if msg.is_empty() {
                return Err("нужен text".into());
            }
            let who = agent_id(app, slug)?;
            let thread_id = match a["thread_id"].as_str().filter(|s| !s.is_empty()) {
                Some(t) => t.to_string(),
                None => {
                    let t = app.store.create_thread(NewThread { agent_id: Some(who.clone()), title: clip(&msg, 60), ..Default::default() }).map_err(e)?;
                    app.publish("thread", json!({"id": t.id}));
                    t.id
                }
            };
            let d = app.store.send_chat(ChatSend { thread_id: thread_id.clone(), text: msg, ..Default::default() }, &app.backend_default()).map_err(e)?;
            app.publish("message", serde_json::to_value(&d.user_message).unwrap_or_default());
            for m in &d.agent_messages {
                app.publish("message", serde_json::to_value(m).unwrap_or_default());
            }
            app.publish("thread", json!({"id": thread_id}));
            app.poke();
            let wait = a["wait_seconds"].as_u64().unwrap_or(90).min(170);
            let until = Instant::now() + Duration::from_secs(wait);
            let ids: Vec<String> = d.agent_messages.iter().map(|m| m.id.clone()).collect();
            while Instant::now() < until && !ids.is_empty() {
                tokio::time::sleep(Duration::from_millis(1200)).await;
                let ms = app.store.list_messages(&thread_id, None, 12).map_err(e)?;
                if let Some(m) = ms.iter().find(|m| ids.contains(&m.id) && matches!(m.status, MessageStatus::Done | MessageStatus::Failed | MessageStatus::Cancelled)) {
                    return Ok(text(json!({"thread_id": thread_id, "status": m.status.as_str(), "answer": m.body}).to_string()));
                }
            }
            Ok(text(json!({"thread_id": thread_id, "status": "working", "note": "ответа пока нет; прочитайте чат через senate_thread"}).to_string()))
        }
        other => Err(format!("неизвестный инструмент: {other}")),
    }
}

fn authorized(app: &App, headers: &HeaderMap, path_token: Option<&str>) -> bool {
    let bearer = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    bearer == Some(app.token.as_str()) || path_token == Some(app.token.as_str())
}

async fn handle(app: Arc<App>, headers: HeaderMap, path_token: Option<String>, body: Value) -> Response {
    if !authorized(&app, &headers, path_token.as_deref()) {
        return (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Bearer")], "нужен токен доступа").into_response();
    }
    let one = |req: &Value| {
        let app = app.clone();
        let req = req.clone();
        async move {
            let id = req.get("id").cloned();
            let method = req["method"].as_str().unwrap_or("").to_string();
            let result = match method.as_str() {
                "initialize" => Some(json!({
                    "protocolVersion": req["params"]["protocolVersion"].as_str().unwrap_or("2025-03-26"),
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "senate", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": "Senate: постоянные цифровые специалисты. Спросите специалиста (senate_ask), поставьте задачу (senate_task_create), найдите в памяти (senate_memory_search)."
                })),
                "tools/list" => Some(json!({"tools": tools()})),
                "tools/call" => Some(call(&app, req["params"]["name"].as_str().unwrap_or(""), &req["params"]["arguments"]).await),
                "ping" => Some(json!({})),
                _ => None,
            };
            match (id, result) {
                (Some(id), Some(r)) => Some(json!({"jsonrpc": "2.0", "id": id, "result": r})),
                (Some(id), None) if !method.starts_with("notifications/") => Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "method not found"}})),
                _ => None,
            }
        }
    };
    if let Some(batch) = body.as_array() {
        let mut out = vec![];
        for r in batch {
            if let Some(x) = one(r).await {
                out.push(x);
            }
        }
        return if out.is_empty() { StatusCode::ACCEPTED.into_response() } else { Json(Value::Array(out)).into_response() };
    }
    match one(&body).await {
        Some(x) => Json(x).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

pub async fn post_root(State(a): S, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    handle(a, headers, None, body).await
}
pub async fn post_token(State(a): S, Path(token): Path<String>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    handle(a, headers, Some(token), body).await
}
pub async fn get_not_allowed() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")], "только POST").into_response()
}
