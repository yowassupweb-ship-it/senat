//! Kimi Code backend over ACP (Agent Client Protocol, JSON-RPC on stdio). Kimi runs in its
//! `default` mode, so every sensitive tool call arrives as `session/request_permission` and is
//! answered by Senate's own policy (and, when a human must decide, by the chat) — the same rules as
//! for Claude. `plan` mode maps to Kimi's own read-only planning mode.

use crate::common::{auth_problem, human_error, limit_from_text, spawn, Proc, Tracker};
use crate::policy::{self, Verdict};
use crate::{clip, cli, Approver, CancelRx, Decision, Event, EventTx, Outcome, PermissionMode, PermissionRequest, Progress, RunSpec};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const RUN_LIMIT: Duration = Duration::from_secs(2 * 60 * 60);

async fn send(p: &mut Proc, v: &Value) {
    if let Some(w) = p.stdin.as_mut() {
        let _ = w.write_all(format!("{v}\n").as_bytes()).await;
        let _ = w.flush().await;
    }
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(text_of).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n"),
        Value::Object(o) => text_of(o.get("text").or(o.get("content")).or(o.get("output")).unwrap_or(&Value::Null)),
        _ => String::new(),
    }
}

/// A request/response round trip during setup. Anything else Kimi says meanwhile is declined.
async fn call(p: &mut Proc, id: &mut u64, method: &str, params: Value, secs: u64) -> Result<Value, String> {
    *id += 1;
    let me = *id;
    send(p, &json!({"jsonrpc": "2.0", "id": me, "method": method, "params": params})).await;
    let deadline = tokio::time::sleep(Duration::from_secs(secs));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            l = p.lines.recv() => {
                let Some(l) = l else { return Err(format!("Kimi завершил процесс: {}", clip(p.tail.lock().unwrap().trim(), 300))) };
                let Ok(m) = serde_json::from_str::<Value>(l.trim()) else { continue };
                if m.get("method").is_some() {
                    if let Some(rid) = m.get("id") {
                        send(p, &json!({"jsonrpc": "2.0", "id": rid, "error": {"code": -32601, "message": "unsupported"}})).await;
                    }
                    continue;
                }
                if m["id"] == json!(me) {
                    return match m.get("error") {
                        Some(e) => Err(e["message"].as_str().unwrap_or("Kimi вернул ошибку").to_string()),
                        None => Ok(m["result"].clone()),
                    };
                }
            }
            _ = &mut deadline => return Err(format!("{method}: Kimi не ответил вовремя")),
        }
    }
}

fn init_params() -> Value {
    json!({"protocolVersion": 1, "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false}})
}

fn command(cwd: &std::path::Path) -> Option<tokio::process::Command> {
    let exe = cli::resolve("kimi")?;
    let mut c = cli::command(&exe);
    c.arg("acp").current_dir(cwd).env_clear().envs(cli::child_env()).env("KIMI_CODE_TRUST_WORKSPACE", "1");
    Some(c)
}

/// Models offered to this account, asked from Kimi itself.
pub async fn catalog() -> Option<Vec<Value>> {
    let dir = std::env::temp_dir().join(format!("senate-kimi-catalog-{}", senate_core::new_id()));
    std::fs::create_dir_all(&dir).ok()?;
    let mut p = spawn(command(&dir)?).ok()?;
    let mut id = 0u64;
    let res = async {
        call(&mut p, &mut id, "initialize", init_params(), 30).await.ok()?;
        let s = call(&mut p, &mut id, "session/new", json!({"cwd": dir.to_string_lossy(), "mcpServers": []}), 30).await.ok()?;
        let sid = s["sessionId"].as_str().unwrap_or("").to_string();
        let models = s["configOptions"].as_array()?.iter().find(|o| o["id"] == "model")?["options"]
            .as_array()?
            .iter()
            .map(|o| json!({"id": o["value"], "label": o["name"]}))
            .collect::<Vec<_>>();
        if !sid.is_empty() {
            let _ = call(&mut p, &mut id, "session/delete", json!({"sessionId": sid}), 3).await;
        }
        Some(models)
    }
    .await;
    let _ = p.child.kill().await;
    let _ = std::fs::remove_dir_all(&dir);
    res
}

fn mcp_list(spec: &RunSpec) -> Vec<Value> {
    let kv = |v: &[(String, String)]| v.iter().map(|(n, x)| json!({"name": n, "value": x})).collect::<Vec<_>>();
    let mut out = vec![];
    if let Some(m) = &spec.mcp {
        out.push(json!({"name": "senate", "type": "stdio", "command": m.command, "args": m.args, "env": kv(&m.env)}));
    }
    for x in &spec.extra_mcp {
        out.push(if x.kind == "http" {
            json!({"name": x.name, "type": "http", "url": x.url, "headers": kv(&x.headers)})
        } else {
            json!({"name": x.name, "type": "stdio", "command": x.command, "args": x.args, "env": kv(&x.env)})
        });
    }
    out
}

/// Map an ACP permission request onto the tool vocabulary of our policy.
fn judge_acp(spec: &RunSpec, call: &Value) -> Verdict {
    let kind = call["kind"].as_str().unwrap_or("");
    let raw = &call["rawInput"];
    let title = call["title"].as_str().unwrap_or("").to_string();
    let path = call["locations"][0]["path"].as_str().or(raw["path"].as_str()).or(raw["file_path"].as_str()).unwrap_or("").to_string();
    if title.to_lowercase().contains("plan") && (raw.get("plan").is_some() || kind == "switch_mode") {
        return policy::judge(spec.mode, "ExitPlanMode", raw, &spec.cwd, &spec.trusted_mcp());
    }
    match kind {
        "read" | "search" | "fetch" | "think" => Verdict::Allow,
        "edit" | "delete" | "move" => policy::judge(spec.mode, "Edit", &json!({"file_path": path}), &spec.cwd, &[]),
        "execute" => policy::judge(spec.mode, "Bash", &json!({"command": raw["command"].as_str().unwrap_or(&title)}), &spec.cwd, &[]),
        _ => {
            // MCP and other tools: the server's Trusted switch decides, Auto lets everything through.
            let name = title.to_lowercase();
            if spec.extra_mcp.iter().any(|m| m.trusted && name.contains(&m.name)) || name.contains("senate") || spec.mode == PermissionMode::Auto {
                Verdict::Allow
            } else if spec.mode == PermissionMode::Plan {
                Verdict::Deny("В режиме «План» изменения не выполняются.".into())
            } else {
                Verdict::Ask(format!("Использовать инструмент: {title}"))
            }
        }
    }
}

fn pick(options: &[Value], allow: bool) -> Value {
    let find = |kinds: &[&str]| options.iter().find(|o| kinds.contains(&o["kind"].as_str().unwrap_or(""))).cloned();
    let chosen = if allow { find(&["allow_once", "allow_always"]).or_else(|| options.first().cloned()) } else { find(&["reject_once", "reject_always"]) };
    match chosen {
        Some(o) => json!({"outcome": {"outcome": "selected", "optionId": o["optionId"]}}),
        None => json!({"outcome": {"outcome": "cancelled"}}),
    }
}

/// The last error Kimi wrote to its own log in the past two minutes (`provider.api_error: 429 You've reached …`).
fn recent_log_error() -> String {
    let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(std::path::PathBuf::from) else { return String::new() };
    for dir in [".kimi-code", ".kimi"] {
        let Ok(text) = std::fs::read_to_string(home.join(dir).join("logs").join("kimi-code.log")) else { continue };
        let since = chrono_like_now_minus(120);
        for line in text.lines().rev().take(200) {
            // lines start with an ISO time: only fresh ones count
            if line.get(..19).map(|t| t < since.as_str()).unwrap_or(true) {
                break;
            }
            let l = line.to_lowercase();
            if l.contains("error") && (l.contains("429") || l.contains("limit") || l.contains("quota") || l.contains("api_error") || l.contains("provider")) {
                return line.splitn(2, "ERROR").nth(1).unwrap_or(line).trim().chars().take(400).collect();
            }
        }
    }
    String::new()
}

/// `YYYY-MM-DDTHH:MM:SS` (UTC) `secs` ago, comparable with the log's line starts.
fn chrono_like_now_minus(secs: u64) -> String {
    let t = std::time::SystemTime::now() - std::time::Duration::from_secs(secs);
    let s = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (s.div_euclid(86_400), s.rem_euclid(86_400));
    // civil-from-days (Howard Hinnant), no date library needed for this one comparison
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// The folder Kimi recorded for a session (`session_index.jsonl` in its home), if it knows the session.
fn session_workdir(session_id: &str) -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(std::path::PathBuf::from)?;
    for dir in [".kimi-code", ".kimi"] {
        let Ok(text) = std::fs::read_to_string(home.join(dir).join("session_index.jsonl")) else { continue };
        for line in text.lines().rev() {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            if v["sessionId"].as_str() == Some(session_id) {
                return v["workDir"].as_str().map(std::path::PathBuf::from);
            }
        }
    }
    None
}

/// Kimi continues a session in the folder it was started in, whatever `cwd` it is given. When that folder is gone
/// or is not this run's (the specialist's username — and so their folder — was changed, or the project moved), the
/// session cannot be continued: every tool would start in a folder that does not exist ("Internal error").
pub(crate) fn resume_is_stale(recorded: Option<&std::path::Path>, cwd: &std::path::Path) -> bool {
    let Some(w) = recorded else { return false };
    let same = std::fs::canonicalize(w).ok().zip(std::fs::canonicalize(cwd).ok()).map(|(a, b)| a == b).unwrap_or(false);
    !same
}

pub async fn run(spec: RunSpec, approver: &dyn Approver, tx: EventTx, mut cancel: CancelRx) -> Outcome {
    if !spec.cwd.is_dir() {
        return Outcome { failure: Some(format!("Рабочая папка недоступна: {}", spec.cwd.display())), ..Default::default() };
    }
    if let Some(r) = &spec.resume {
        if resume_is_stale(session_workdir(r).as_deref(), &spec.cwd) {
            // the caller reads this marker and runs the turn in a fresh session, with the role in it
            return Outcome { failure: Some("session not found: its folder moved".into()), ..Default::default() };
        }
    }
    let Some(cmd) = command(&spec.cwd) else {
        return Outcome { failure: Some("Kimi Code не найден на этой машине. Установите: npm i -g @moonshot-ai/kimi-code, затем войдите в Настройках.".into()), ..Default::default() };
    };
    let mut p = match spawn(cmd) {
        Ok(p) => p,
        Err(e) => return Outcome { failure: Some(format!("Не удалось запустить Kimi: {e}")), ..Default::default() },
    };
    let mut t = Tracker::new(tx.clone(), spec.model().unwrap_or(""));
    t.phase("подключается");
    let mut id = 0u64;
    let resumed = spec.resume.is_some();

    let setup = async {
        call(&mut p, &mut id, "initialize", init_params(), 40).await?;
        t.phase("готовит диалог");
        let session = match &spec.resume {
            Some(r) => call(&mut p, &mut id, "session/load", json!({"sessionId": r, "cwd": spec.cwd.to_string_lossy(), "mcpServers": mcp_list(&spec)}), 60).await?,
            None => call(&mut p, &mut id, "session/new", json!({"cwd": spec.cwd.to_string_lossy(), "mcpServers": mcp_list(&spec)}), 60).await?,
        };
        let sid = session["sessionId"].as_str().map(String::from).or_else(|| spec.resume.clone()).unwrap_or_default();
        if let Some(m) = spec.model() {
            call(&mut p, &mut id, "session/set_config_option", json!({"sessionId": sid, "configId": "model", "value": m}), 20).await.ok();
        }
        if let Some(th) = spec.effort.as_deref().and_then(|e| match e { "low" => Some("low"), "medium" => Some("high"), "high" => Some("max"), _ => None }) {
            call(&mut p, &mut id, "session/set_config_option", json!({"sessionId": sid, "configId": "thinking", "value": th}), 20).await.ok();
        }
        let mode = if spec.mode == PermissionMode::Plan { "plan" } else { "default" };
        call(&mut p, &mut id, "session/set_mode", json!({"sessionId": sid, "modeId": mode}), 20).await.ok();
        Ok::<String, String>(sid)
    }
    .await;
    let sid = match setup {
        Ok(s) => s,
        Err(e) => {
            let _ = p.child.kill().await;
            let mut o = Outcome { failure: Some(clip(&human_error(&e), 500)), ..Default::default() };
            // A lost session continues as a new one (the caller retries without resume on this marker).
            if resumed && e.to_lowercase().contains("session") {
                o.failure = Some(format!("session not found: {e}"));
            }
            return o;
        }
    };
    t.session_id = Some(sid.clone());
    t.send(Progress { session_id: Some(sid.clone()), ..Default::default() });
    t.phase("думает");

    // The role goes first in a new session's message (Kimi has no separate system prompt over ACP).
    let text = if resumed || spec.system_prompt.is_empty() { spec.prompt.clone() } else { format!("{}\n\n---\n\n{}", spec.system_prompt, spec.prompt) };
    id += 1;
    let prompt_id = id;
    send(&mut p, &json!({"jsonrpc": "2.0", "id": prompt_id, "method": "session/prompt", "params": {"sessionId": sid, "prompt": [{"type": "text", "text": text}]}})).await;

    let deadline = tokio::time::sleep(RUN_LIMIT);
    tokio::pin!(deadline);
    let mut stopped = false;
    let mut thought = String::new();
    let mut answering = false;
    let mut args_buf: std::collections::HashMap<String, String> = Default::default();
    loop {
        tokio::select! {
            l = p.lines.recv() => {
                let Some(l) = l else { break };
                let Ok(m) = serde_json::from_str::<Value>(l.trim()) else { continue };
                if std::env::var_os("SENATE_DEBUG_ACP").is_some() { eprintln!("ACP< {}", clip(&m.to_string(), 700)); }
                if let Some(method) = m["method"].as_str() {
                    match (method, m.get("id")) {
                        ("session/update", None) => update(&m["params"]["update"], &mut t, &mut thought, &mut answering, &mut args_buf),
                        ("session/request_permission", Some(rid)) => {
                            let call = &m["params"]["toolCall"];
                            let options = m["params"]["options"].as_array().cloned().unwrap_or_default();
                            let tid = call["toolCallId"].as_str().unwrap_or("").to_string();
                            let allow = match judge_acp(&spec, call) {
                                Verdict::Allow => true,
                                Verdict::Deny(_) => false,
                                Verdict::Ask(reason) => {
                                    let raw = &call["rawInput"];
                                    let shown_input = if raw.is_null() { json!({"title": call["title"]}) } else { raw.clone() };
                                    let tool = if call["kind"] == "execute" { "Bash" } else if matches!(call["kind"].as_str(), Some("edit" | "delete" | "move")) { "Edit" } else { call["title"].as_str().unwrap_or("tool") };
                                    let req = PermissionRequest { tool_use_id: tid.clone(), tool: tool.into(), input: crate::claude::clip_value(&shown_input, 4000), reason };
                                    if approver.preapproved(&req) {
                                        true
                                    } else {
                                        t.tool_status(&tid, "awaiting");
                                        t.phase("ждёт разрешения");
                                        let _ = tx.send(Event::Ask(req.clone()));
                                        let d = tokio::select! {
                                            d = approver.ask(req) => d,
                                            _ = async { loop { if cancel.changed().await.is_err() || *cancel.borrow() { break } } } => { stopped = true; Decision::Deny(String::new()) }
                                        };
                                        let ok = matches!(d, Decision::Allow);
                                        let _ = tx.send(Event::Answered { tool_use_id: tid.clone(), allowed: ok });
                                        t.tool_status(&tid, if ok { "running" } else { "denied" });
                                        ok
                                    }
                                }
                            };
                            send(&mut p, &json!({"jsonrpc": "2.0", "id": rid, "result": pick(&options, allow)})).await;
                            if stopped { break }
                        }
                        (_, Some(rid)) => send(&mut p, &json!({"jsonrpc": "2.0", "id": rid, "error": {"code": -32601, "message": "unsupported"}})).await,
                        _ => {}
                    }
                } else if m["id"] == json!(prompt_id) {
                    if let Some(e) = m.get("error") {
                        t.failure = Some(human_error(e["message"].as_str().unwrap_or("Kimi вернул ошибку")));
                    } else {
                        t.done = true;
                    }
                    break;
                }
            }
            _ = cancel.changed() => { if *cancel.borrow() { stopped = true; break } }
            _ = &mut deadline => { t.failure = Some("Агент работал дольше двух часов — остановлен".into()); break }
        }
    }
    if !stopped && answering {
        t.text = t.partial.trim().to_string();
    }
    if !thought.trim().is_empty() {
        t.send(Progress { reasoning: Some(clip(thought.trim(), 2000)), ..Default::default() });
    }
    // Politely close the session; the process is killed right after.
    if !stopped {
        let _ = tokio::time::timeout(Duration::from_secs(2), async { p.stdin = None }).await;
    }
    let tail = p.tail.lock().unwrap().clone();
    if stopped {
        crate::common::kill_tree(&mut p.child).await;
    } else {
        let _ = p.child.kill().await;
    }
    // Kimi over ACP ends a prompt it could not run (a spent limit, a provider error) as if it were done: no word, no
    // step. That is a failure with a reason — from its own log — not an empty success a colleague waits on forever.
    let said_nothing = t.done && t.text.trim().is_empty() && t.steps.is_empty();
    if !stopped && t.failure.is_none() && said_nothing {
        let log = recent_log_error();
        t.failure = Some(match limit_from_text(&log).or_else(|| limit_from_text(&tail)) {
            Some(l) => format!("Исчерпан лимит подписки Kimi: {l}"),
            None if !log.is_empty() => clip(&human_error(&log), 600),
            None => "Kimi закончил ход без ответа и без действий: вероятно, исчерпан лимит или сбой провайдера. Повторите позже или смените модель.".into(),
        });
    }
    if !stopped && t.failure.is_none() && t.text.is_empty() && !t.done {
        t.failure = Some(match limit_from_text(&tail) {
            Some(l) => format!("Исчерпан лимит подписки: {l}"),
            None if auth_problem(&tail) => "Вход в Kimi истёк — войдите заново в Настройках".into(),
            None if tail.trim().is_empty() => "Kimi завершился без ответа".into(),
            None => clip(&human_error(tail.trim()), 600),
        });
    }
    t.finish(stopped)
}

fn update(u: &Value, t: &mut Tracker, thought: &mut String, answering: &mut bool, args_buf: &mut std::collections::HashMap<String, String>) {
    match u["sessionUpdate"].as_str().unwrap_or("") {
        "agent_thought_chunk" => {
            thought.push_str(&text_of(&u["content"]));
            t.phase("думает");
        }
        "agent_message_chunk" => {
            let s = text_of(&u["content"]);
            if s.is_empty() {
                return;
            }
            if !*answering {
                t.partial.clear();
                *answering = true;
            }
            t.stream_text(&s, false);
        }
        "tool_call" => {
            let id = u["toolCallId"].as_str().or(u["id"].as_str()).unwrap_or("").to_string();
            if id.is_empty() || t.has_tool(&id) {
                return;
            }
            if *answering {
                t.flush_text_to_step();
                *answering = false;
            }
            let (name, mut input) = tool_identity(u);
            if input.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                input = input_from_context(u, &name);
            }
            // remembered for the updates that come without a name
            args_buf.insert(format!("name:{id}"), name.clone());
            t.tool_start(&id, &name, crate::claude::hint_of(&name, &input), input);
        }
        "tool_call_update" => {
            let id = u["toolCallId"].as_str().or(u["id"].as_str()).unwrap_or("").to_string();
            if !t.has_tool(&id) {
                return;
            }
            // Kimi names the tool in its first event only; an update without a name keeps the one already known
            // (it used to be overwritten with "tool"), and what the step is about comes later — the file in
            // `locations`, the command in the text "Running: …".
            let has_name = u["toolName"].is_string() || u["name"].is_string() || u["title"].is_string();
            let known = args_buf.get(&format!("name:{id}")).cloned();
            let (mut name, mut input) = tool_identity(u);
            if !has_name {
                if let Some(k) = known.clone() {
                    name = k;
                }
            } else {
                args_buf.insert(format!("name:{id}"), name.clone());
            }
            if input.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                input = input_from_context(u, &name);
            }
            if has_name || input.as_object().map(|o| !o.is_empty()).unwrap_or(false) {
                t.tool_identify(&id, &name, crate::claude::hint_of(&name, &input));
            }
            let done = matches!(u["status"].as_str(), Some("completed" | "failed"));
            if !done {
                // While running, `content` may carry accumulated arguments rather than a result.
                let s = text_of(&u["content"]);
                if !s.is_empty() {
                    args_buf.insert(id.clone(), s);
                }
                if input.as_object().map(|o| !o.is_empty()).unwrap_or(false) {
                    t.tool_input(&id, crate::claude::hint_of(&name, &input), input);
                }
                return;
            }
            if input.as_object().map(|o| !o.is_empty()).unwrap_or(false) {
                t.tool_input(&id, crate::claude::hint_of(&name, &input), input);
            } else if let Some(raw) = args_buf.remove(&id) {
                let parsed = serde_json::from_str::<Value>(&raw).unwrap_or(Value::String(raw));
                t.tool_input(&id, crate::claude::hint_of(&name, &parsed), parsed);
            }
            let out = u["rawOutput"].as_str().map(String::from).unwrap_or_else(|| {
                let s = text_of(&u["content"]);
                if s.is_empty() { text_of(&u["rawOutput"]) } else { s }
            });
            t.tool_end(&id, &out, u["status"] == "failed", None);
        }
        "plan" => {
            let entries = u["entries"].as_array().cloned().unwrap_or_default();
            if !entries.is_empty() {
                let lines = entries
                    .iter()
                    .map(|e| format!("{} {}", match e["status"].as_str() { Some("completed") => "✓", Some("in_progress") => "▶", _ => "○" }, text_of(&e["content"])))
                    .collect::<Vec<_>>()
                    .join("\n");
                let tid = "kimi-todo";
                if t.has_tool(tid) {
                    t.tool_end(tid, &lines, false, None);
                    t.tool_start(tid, "Список дел", "План работы".into(), Value::Null);
                } else {
                    t.tool_start(tid, "Список дел", "План работы".into(), Value::Null);
                }
                t.tool_end(tid, &lines, false, None);
            }
            t.phase("планирует");
        }
        "usage_update" => {
            t.work.context_tokens = u["used"].as_u64().or(u["tokens"].as_u64()).unwrap_or(0);
            t.work.context_window = u["size"].as_u64().or(u["limit"].as_u64()).unwrap_or(0);
        }
        _ => {}
    }
}

/// Tool name and structured input of an ACP tool call. File edits carry a diff in `content`:
/// shown as an Edit with before/after so the chat can render it.
/// What a Kimi step is about when it sends no arguments: the file in `locations`, or the command in a text like
/// "Running: echo hi" / "Requesting approval to Running: …".
fn input_from_context(u: &Value, name: &str) -> Value {
    if let Some(p) = u["locations"].as_array().and_then(|l| l.first()).and_then(|l| l["path"].as_str()) {
        return json!({"file_path": p});
    }
    let text = text_of(&u["content"]);
    if let Some(i) = text.find("Running:") {
        let cmd = text[i + "Running:".len()..].trim();
        if !cmd.is_empty() {
            return json!({"command": cmd});
        }
    }
    if name == "Bash" && !text.trim().is_empty() && text.len() < 400 {
        return json!({"command": text.trim()});
    }
    json!({})
}

fn tool_identity(u: &Value) -> (String, Value) {
    let name = u["toolName"].as_str().or(u["name"].as_str()).or(u["title"].as_str()).unwrap_or("tool").trim_start_matches("mcp__senate__").to_string();
    if let Some(d) = u["content"].as_array().and_then(|a| a.iter().find(|c| c["type"] == "diff")) {
        return ("Edit".into(), json!({"file_path": d["path"], "old_string": d["oldText"].as_str().unwrap_or(""), "new_string": d["newText"].as_str().unwrap_or("")}));
    }
    let raw = u["rawInput"].clone();
    let input = if raw.is_object() { raw } else { json!({}) };
    let name = match u["kind"].as_str() {
        Some("execute") if name.to_lowercase().contains("shell") || name.to_lowercase().contains("bash") || name.to_lowercase().contains("run") => "Bash".to_string(),
        _ => name,
    };
    (name, input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kimi_step_shows_what_it_does() {
        // what Kimi 2.6 sends: the name only first, the file at the end, the command as text
        let first = json!({"toolCallId": "1", "title": "Read", "kind": "read", "status": "pending"});
        assert_eq!(tool_identity(&first).0, "Read");
        let done = json!({"toolCallId": "1", "status": "completed", "locations": [{"path": "/tmp/x/README.md"}]});
        assert_eq!(input_from_context(&done, "Read"), json!({"file_path": "/tmp/x/README.md"}));
        let ask = json!({"content": [{"type": "content", "content": {"type": "text", "text": "Requesting approval to Running: echo hi"}}]});
        assert_eq!(input_from_context(&ask, "Bash"), json!({"command": "echo hi"}));
    }

    #[test]
    fn a_session_from_another_folder_is_not_continued() {
        let base = std::env::temp_dir().join(format!("senate-kimi-wd-{}", std::process::id()));
        let now = base.join("assist");
        std::fs::create_dir_all(&now).unwrap();
        // the folder the session was started in is gone (username changed): start fresh
        assert!(resume_is_stale(Some(&base.join("assistent")), &now));
        // same folder, even spelled differently: continue
        assert!(!resume_is_stale(Some(&now.join(".")), &now));
        // a session Kimi has no record of: let Kimi decide
        assert!(!resume_is_stale(None, &now));
        // and the marker is the one the caller turns into a fresh session
        let o = crate::Outcome { failure: Some("session not found: its folder moved".into()), ..Default::default() };
        assert!(crate::session_is_lost(&o));
        let _ = std::fs::remove_dir_all(&base);
    }
}
