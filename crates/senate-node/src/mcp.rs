//! MCP bridge (stdio). The agent CLI starts `senate-node mcp`; it exposes Senate's tools and relays
//! each call to the node over HTTP with a token that opens exactly one session's tools.

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn rpc_result(id: &Value, result: Value) -> String {
    format!("{}\n", json!({"jsonrpc": "2.0", "id": id, "result": result}))
}
fn rpc_error(id: &Value, code: i64, msg: &str) -> String {
    format!("{}\n", json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}}))
}

pub async fn run_stdio() {
    let hub = std::env::var("SENATE_HUB").unwrap_or_default();
    let token = std::env::var("SENATE_SESSION_TOKEN").unwrap_or_default();
    let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(120)).build().unwrap();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();

    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let reply = match method {
            "initialize" => Some(rpc_result(
                &id,
                json!({
                    "protocolVersion": req["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "senate", "version": env!("CARGO_PKG_VERSION")}
                }),
            )),
            "tools/list" => {
                let tools: Vec<Value> = senate_core::tools::definitions()
                    .into_iter()
                    .map(|d| json!({"name": d.name, "description": d.description, "inputSchema": d.schema}))
                    .collect();
                Some(rpc_result(&id, json!({"tools": tools})))
            }
            "tools/call" => {
                let name = req["params"]["name"].as_str().unwrap_or("");
                let args = req["params"]["arguments"].clone();
                let resp = http
                    .post(format!("{hub}/api/agent/call"))
                    .bearer_auth(&token)
                    .json(&json!({"name": name, "args": if args.is_null() { json!({}) } else { args }}))
                    .send()
                    .await;
                let (text, is_error) = match resp {
                    Ok(r) => {
                        let ok = r.status().is_success();
                        let body: Value = r.json().await.unwrap_or(Value::Null);
                        if ok {
                            (body["result"].to_string(), false)
                        } else {
                            (body["error"].as_str().unwrap_or("ошибка").to_string(), true)
                        }
                    }
                    Err(e) => (format!("Senate недоступен: {e}"), true),
                };
                Some(rpc_result(&id, json!({"content": [{"type": "text", "text": text}], "isError": is_error})))
            }
            "ping" => Some(rpc_result(&id, json!({}))),
            m if m.starts_with("notifications/") => None,
            _ if id.is_null() => None,
            _ => Some(rpc_error(&id, -32601, "method not found")),
        };
        if let Some(r) = reply {
            let _ = out.write_all(r.as_bytes()).await;
            let _ = out.flush().await;
        }
    }
}
