//! Things the node does on the agent's behalf so that secrets never reach the agent: calls to the
//! user's connected APIs, and a connectivity test for MCP servers.

use crate::App;
use senate_core::model::McpServer;
use senate_core::tools::ToolCtx;
use senate_core::Error;
use serde_json::{json, Value};
use std::time::Duration;

const MAX_BODY: usize = 100_000;

fn client() -> reqwest::Client {
    // No redirects: a connected API must not be able to bounce the secret to another host.
    reqwest::Client::builder().timeout(Duration::from_secs(30)).redirect(reqwest::redirect::Policy::none()).build().unwrap()
}

fn safe_path(p: &str) -> bool {
    p.starts_with('/') && !p.contains("://") && !p.contains("..") && !p.contains('\\') && !p.contains('@') && !p.contains('\n') && p.len() < 2000
}

/// `senate_api_call`. Reads go straight through; writes follow the connection's own rule:
/// deny, ask the user (approval), or auto.
pub(crate) async fn api_call(app: &App, ctx: &ToolCtx, args: &Value, bypass: bool) -> Result<Value, Error> {
    let name = args["service"].as_str().unwrap_or("");
    let it = app
        .store
        .integrations_for_agent(&ctx.agent_id)?
        .into_iter()
        .find(|i| i.name == name)
        .ok_or_else(|| Error::Forbidden(format!("подключение «{name}» не выдано этому специалисту (см. senate_api_list)")))?;
    let method = args["method"].as_str().unwrap_or("GET").to_uppercase();
    if !["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"].contains(&method.as_str()) {
        return Err(Error::invalid("неизвестный метод"));
    }
    let path = args["path"].as_str().unwrap_or("");
    if !safe_path(path) {
        return Err(Error::invalid("path должен начинаться с / и не содержать адреса, .. и @"));
    }
    let write = !matches!(method.as_str(), "GET" | "HEAD");
    if write && !bypass {
        match it.write_mode.as_str() {
            "deny" => return Err(Error::Forbidden(format!("запись в «{}» запрещена настройками подключения", it.name))),
            "approve" => {
                let detail = json!({"tool": "senate_api_call", "args": args, "service": it.name, "ctx": {
                    "agent_id": ctx.agent_id, "session_id": ctx.session_id, "obligation_id": ctx.obligation_id,
                    "thread_id": ctx.thread_id, "project_id": ctx.project_id, "company_id": ctx.company_id}});
                let ap = app.store.create_approval(&ctx.agent_id, ctx.session_id.as_deref(), ctx.obligation_id.as_deref(), "external.send", &detail)?;
                app.publish("attention", json!({}));
                return Ok(json!({"status": "pending_approval", "approval_id": ap.id, "message": "Запись во внешний сервис ждёт согласия пользователя."}));
            }
            _ => {}
        }
    }
    let url = format!("{}{}", it.base_url, path);
    let mut req = client().request(method.parse().unwrap_or(reqwest::Method::GET), &url);
    let mut query: Vec<(String, String)> = args["query"]
        .as_object()
        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_str().map(String::from).unwrap_or_else(|| v.to_string()))).collect())
        .unwrap_or_default();
    match it.auth_type.as_str() {
        "bearer" if !it.secret.is_empty() => req = req.bearer_auth(&it.secret),
        "header" if !it.secret.is_empty() => req = req.header(it.auth_name.as_str(), &it.secret),
        "basic" if !it.secret.is_empty() => {
            let (u, p) = it.secret.split_once(':').unwrap_or((&it.secret, ""));
            req = req.basic_auth(u, Some(p));
        }
        "query" if !it.secret.is_empty() => query.push((it.auth_name.clone(), it.secret.clone())),
        _ => {}
    }
    if !query.is_empty() {
        req = req.query(&query);
    }
    if write && !args["body"].is_null() {
        req = req.json(&args["body"]);
    }
    let started = std::time::Instant::now();
    let resp = req.send().await.map_err(|e| Error::invalid(format!("запрос не удался: {}", e.without_url())))?;
    let status = resp.status().as_u16();
    let ctype = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let bytes = resp.bytes().await.map_err(|e| Error::invalid(format!("ответ не прочитан: {}", e.without_url())))?;
    let truncated = bytes.len() > MAX_BODY;
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_BODY)]).into_owned();
    let body = if ctype.contains("json") { serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text)) } else { Value::String(text) };
    let _ = app.store.emit(
        "api.called",
        "integration",
        &it.id,
        &ctx.agent_id,
        json!({"service": it.name, "method": method, "path": path, "status": status, "ms": started.elapsed().as_millis() as u64}),
    );
    Ok(json!({"status": status, "ok": (200..300).contains(&status), "body": body, "truncated": truncated}))
}

pub(crate) async fn test_integration(it: &senate_core::model::Integration, path: &str) -> Value {
    let path = if path.is_empty() { "/" } else { path };
    if !safe_path(path) {
        return json!({"ok": false, "message": "некорректный путь"});
    }
    let mut req = client().get(format!("{}{}", it.base_url, path));
    match it.auth_type.as_str() {
        "bearer" if !it.secret.is_empty() => req = req.bearer_auth(&it.secret),
        "header" if !it.secret.is_empty() => req = req.header(it.auth_name.as_str(), &it.secret),
        "basic" if !it.secret.is_empty() => {
            let (u, p) = it.secret.split_once(':').unwrap_or((&it.secret, ""));
            req = req.basic_auth(u, Some(p));
        }
        "query" if !it.secret.is_empty() => req = req.query(&[(it.auth_name.as_str(), it.secret.as_str())]),
        _ => {}
    }
    match req.send().await {
        Ok(r) => {
            let s = r.status().as_u16();
            let hint = match s {
                401 | 403 => "сервис ответил, но ключ не принят",
                404 => "сервис отвечает, но по этому пути ничего нет — задайте путь проверки",
                200..=299 => "связь есть, ключ принят",
                _ => "сервис ответил",
            };
            json!({"ok": (200..400).contains(&s), "status": s, "message": format!("{s}: {hint}")})
        }
        Err(e) => json!({"ok": false, "message": format!("нет связи: {}", e.without_url())}),
    }
}

/// Start the MCP server (or ask it over HTTP), list its tools. Tells the user whether it works
/// before an agent depends on it.
pub(crate) async fn test_mcp(m: &McpServer) -> Value {
    if m.kind == "http" {
        return test_mcp_http(m).await;
    }
    use senate_runtime::cli;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let exe = cli::resolve(&m.command).unwrap_or_else(|| std::path::PathBuf::from(&m.command));
    let mut cmd = cli::command(&exe);
    cmd.args(&m.args).envs(cli::child_env()).envs(m.env.iter()).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).kill_on_drop(true);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "message": format!("не удалось запустить «{}»: {e}", m.command)}),
    };
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let run = async {
        let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "senate", "version": env!("CARGO_PKG_VERSION")}}});
        stdin.write_all(format!("{init}\n").as_bytes()).await.ok()?;
        let mut tools: Option<Vec<String>> = None;
        let mut server = String::new();
        while let Ok(Some(l)) = lines.next_line().await {
            let Ok(v) = serde_json::from_str::<Value>(l.trim()) else { continue };
            match v["id"].as_i64() {
                Some(1) => {
                    server = v["result"]["serverInfo"]["name"].as_str().unwrap_or("").to_string();
                    stdin.write_all(format!("{}\n", json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).as_bytes()).await.ok()?;
                    stdin.write_all(format!("{}\n", json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).as_bytes()).await.ok()?;
                }
                Some(2) => {
                    tools = Some(v["result"]["tools"].as_array().map(|a| a.iter().filter_map(|t| t["name"].as_str().map(String::from)).collect()).unwrap_or_default());
                    break;
                }
                _ => {}
            }
        }
        tools.map(|t| (server, t))
    };
    let res = tokio::time::timeout(Duration::from_secs(45), run).await;
    let _ = child.kill().await;
    match res {
        Ok(Some((server, tools))) => json!({"ok": true, "server": server, "tools": tools, "message": format!("Работает: {} инструментов", tools.len())}),
        Ok(None) => json!({"ok": false, "message": "Сервер запустился, но не ответил по протоколу MCP"}),
        Err(_) => json!({"ok": false, "message": "Сервер не ответил за 45 секунд (первый запуск через npx может быть долгим — повторите)"}),
    }
}

async fn test_mcp_http(m: &McpServer) -> Value {
    let http = client();
    let post = |body: Value, session: Option<&str>| {
        let mut r = http.post(&m.url).header("Accept", "application/json, text/event-stream").header("Content-Type", "application/json");
        for (k, v) in &m.headers {
            r = r.header(k.as_str(), v);
        }
        if let Some(s) = session {
            r = r.header("Mcp-Session-Id", s);
        }
        r.json(&body)
    };
    let parse = |text: &str| -> Option<Value> {
        let t = text.trim();
        if t.starts_with('{') {
            return serde_json::from_str(t).ok();
        }
        t.lines().filter_map(|l| l.strip_prefix("data:")).filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok()).last()
    };
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "senate", "version": env!("CARGO_PKG_VERSION")}}});
    let r = match post(init, None).send().await {
        Ok(r) => r,
        Err(e) => return json!({"ok": false, "message": format!("нет связи: {}", e.without_url())}),
    };
    if !r.status().is_success() {
        return json!({"ok": false, "message": format!("сервер ответил {}", r.status().as_u16())});
    }
    let session = r.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()).map(String::from);
    let body = r.text().await.unwrap_or_default();
    let Some(v) = parse(&body) else { return json!({"ok": false, "message": "ответ не похож на MCP"}) };
    let server = v["result"]["serverInfo"]["name"].as_str().unwrap_or("").to_string();
    let _ = post(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}), session.as_deref()).send().await;
    let tools = match post(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}), session.as_deref()).send().await {
        Ok(r) => parse(&r.text().await.unwrap_or_default()).and_then(|v| v["result"]["tools"].as_array().map(|a| a.iter().filter_map(|t| t["name"].as_str().map(String::from)).collect::<Vec<_>>())).unwrap_or_default(),
        Err(_) => vec![],
    };
    json!({"ok": true, "server": server, "tools": tools, "message": format!("Работает: {} инструментов", tools.len())})
}
