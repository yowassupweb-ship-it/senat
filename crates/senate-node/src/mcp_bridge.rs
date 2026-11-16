//! MCP servers that live on the node, offered to a person's computer over HTTP.
//!
//! A stdio server such as `node /data/mbox-mcp/mbox-mcp-server.mjs` can only be started where its script is. When a
//! specialist works on a person's computer, that computer cannot start it, so the node starts it instead and the
//! computer's agent talks to it at `POST /mcp-bridge/<server>` ("streamable HTTP", answered with plain JSON).
//!
//! Access is the running session's own token (`SENATE_SESSION_TOKEN`): it opens only the servers assigned to that
//! session's specialist and stops working when the session ends. The server's secrets never leave the node.

use crate::App;
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use senate_runtime::ExtraMcp;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

type S = State<Arc<App>>;

const SESSION_HEADER: &str = "mcp-session-id";
/// A server that does not answer a request within this is considered stuck and is stopped.
const ANSWER_WITHIN: Duration = Duration::from_secs(120);
/// A bridge session nobody has used for this long is closed.
const IDLE: Duration = Duration::from_secs(30 * 60);

struct Proc {
    _child: Child,
    stdin: ChildStdin,
    out: tokio::io::Lines<BufReader<ChildStdout>>,
}

struct Bridge {
    /// The agent session token that opened it: nobody else may use it.
    token: String,
    last: Mutex<Instant>,
    proc: tokio::sync::Mutex<Proc>,
}

fn sessions() -> &'static Mutex<HashMap<String, Arc<Bridge>>> {
    static S: OnceLock<Mutex<HashMap<String, Arc<Bridge>>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

fn bearer(headers: &HeaderMap) -> String {
    headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("").to_string()
}

fn error(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32000, "message": msg}}))).into_response()
}

/// Close what nobody uses and what belongs to a session that has ended (dropping a `Bridge` stops its process).
fn reap(app: &App) {
    let live = app.session_tokens.lock().unwrap();
    sessions().lock().unwrap().retain(|_, b| live.contains_key(&b.token) && b.last.lock().unwrap().elapsed() < IDLE);
}

fn spawn(server: &ExtraMcp, slug: &str) -> Result<Proc, String> {
    let mut cmd = Command::new(&server.command);
    cmd.args(server.args.iter().map(|a| a.replace("{slug}", slug)))
        .envs(server.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| format!("не удалось запустить «{}» на узле: {e}", server.name))?;
    let stdin = child.stdin.take().ok_or("нет stdin у сервера")?;
    let out = BufReader::new(child.stdout.take().ok_or("нет stdout у сервера")?).lines();
    Ok(Proc { _child: child, stdin, out })
}

/// Pass one message to the server. A request gets its answer back; a notification or a reply gets nothing.
async fn forward(p: &mut Proc, req: &Value) -> Result<Option<Value>, String> {
    let mut line = req.to_string();
    line.push('\n');
    p.stdin.write_all(line.as_bytes()).await.map_err(|e| format!("сервер закрыл вход: {e}"))?;
    p.stdin.flush().await.map_err(|e| e.to_string())?;
    let (Some(id), true) = (req.get("id"), req.get("method").is_some()) else { return Ok(None) };
    let wait = async {
        loop {
            let Some(l) = p.out.next_line().await.map_err(|e| e.to_string())? else { return Err("сервер завершился".to_string()) };
            let Ok(v) = serde_json::from_str::<Value>(&l) else { continue };
            // its own notifications and questions are not this request's answer
            if v.get("id") == Some(id) && v.get("method").is_none() {
                return Ok(Some(v));
            }
        }
    };
    tokio::time::timeout(ANSWER_WITHIN, wait).await.map_err(|_| "сервер не ответил вовремя".to_string())?
}

pub async fn post(State(a): S, Path(name): Path<String>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    reap(&a);
    let token = bearer(&headers);
    let ctx = a.session_tokens.lock().unwrap().get(&token).cloned();
    let Some(ctx) = ctx else { return error(StatusCode::UNAUTHORIZED, "сессия не найдена или уже закончилась") };
    // only a server this specialist actually has
    let server = a.store.mcp_for_agent(&ctx.agent_id).ok().and_then(|l| l.iter().find(|m| m.name == name && m.kind == "stdio").map(ExtraMcp::from));
    let Some(server) = server else { return error(StatusCode::NOT_FOUND, "такого сервера у специалиста нет") };

    let sid = headers.get(SESSION_HEADER).and_then(|v| v.to_str().ok()).map(String::from);
    let (bridge, opened) = match sid {
        Some(s) => match sessions().lock().unwrap().get(&s).filter(|b| b.token == token).cloned() {
            Some(b) => (b, None),
            // the client starts over when told the session is gone
            None => return error(StatusCode::NOT_FOUND, "сессия моста закрыта: начните с initialize"),
        },
        None => {
            if body["method"] != "initialize" {
                return error(StatusCode::BAD_REQUEST, "сначала initialize");
            }
            let slug = a.store.get_agent(&ctx.agent_id).map(|g| g.slug).unwrap_or_default();
            let proc = match spawn(&server, &slug) {
                Ok(p) => p,
                Err(e) => return error(StatusCode::BAD_GATEWAY, &e),
            };
            let id = uuid::Uuid::new_v4().simple().to_string();
            let b = Arc::new(Bridge { token: token.clone(), last: Mutex::new(Instant::now()), proc: tokio::sync::Mutex::new(proc) });
            sessions().lock().unwrap().insert(id.clone(), b.clone());
            (b, Some(id))
        }
    };
    *bridge.last.lock().unwrap() = Instant::now();

    let items: Vec<Value> = match body.as_array() {
        Some(a) => a.clone(),
        None => vec![body.clone()],
    };
    let mut answers = vec![];
    {
        let mut p = bridge.proc.lock().await;
        for req in &items {
            match forward(&mut p, req).await {
                Ok(Some(v)) => answers.push(v),
                Ok(None) => {}
                Err(e) => {
                    drop(p);
                    // a stuck or dead server is not reused
                    sessions().lock().unwrap().retain(|_, b| !Arc::ptr_eq(b, &bridge));
                    return error(StatusCode::BAD_GATEWAY, &e);
                }
            }
        }
    }
    let mut resp = match (answers.len(), body.is_array()) {
        (0, _) => StatusCode::ACCEPTED.into_response(),
        (1, false) => Json(answers.remove(0)).into_response(),
        _ => Json(Value::Array(answers)).into_response(),
    };
    if let Some(id) = opened.and_then(|i| HeaderValue::from_str(&i).ok()) {
        resp.headers_mut().insert(SESSION_HEADER, id);
    }
    resp
}

/// The client says it is done.
pub async fn delete(State(a): S, Path(_name): Path<String>, headers: HeaderMap) -> Response {
    let token = bearer(&headers);
    if !a.session_tokens.lock().unwrap().contains_key(&token) {
        return error(StatusCode::UNAUTHORIZED, "сессия не найдена");
    }
    if let Some(s) = headers.get(SESSION_HEADER).and_then(|v| v.to_str().ok()) {
        let mut all = sessions().lock().unwrap();
        if all.get(s).map(|b| b.token == token).unwrap_or(false) {
            all.remove(s);
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

pub async fn get_not_allowed() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST, DELETE")], "только POST").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_request_gets_its_own_answer_and_a_notification_none() {
        // a stdio server that first chatters on its own, then answers whatever id it is asked
        let script = r#"require('readline').createInterface({input:process.stdin}).on('line',l=>{const m=JSON.parse(l);if(m.id===undefined)return;console.log(JSON.stringify({jsonrpc:'2.0',method:'notifications/message',params:{}}));console.log(JSON.stringify({jsonrpc:'2.0',id:m.id,result:{echo:m.method}}))})"#;
        let server = ExtraMcp { name: "t".into(), kind: "stdio".into(), command: "node".into(), args: vec!["-e".into(), script.into()], ..Default::default() };
        let mut p = spawn(&server, "slug").expect("node is installed");
        let a = forward(&mut p, &json!({"jsonrpc": "2.0", "id": 7, "method": "tools/list"})).await.unwrap().unwrap();
        assert_eq!(a["id"], 7);
        assert_eq!(a["result"]["echo"], "tools/list");
        assert!(forward(&mut p, &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await.unwrap().is_none());
        let b = forward(&mut p, &json!({"jsonrpc": "2.0", "id": "x", "method": "ping"})).await.unwrap().unwrap();
        assert_eq!(b["id"], "x");
    }
}
