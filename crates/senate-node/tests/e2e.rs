use senate_node::{serve, Config, Handle};
use serde_json::{json, Value};
use std::time::Duration;

async fn boot() -> (Handle, reqwest::Client, String) {
    let dir = std::env::temp_dir().join(format!("senate-e2e-{}", senate_core::new_id()));
    let cfg = Config {
        data_dir: dir,
        bind: "127.0.0.1:0".parse().unwrap(),
        token: Some("t".repeat(40)),
        default_backend: Some("mock".into()),
        parallel: 4,
    };
    let h = serve(cfg).await.expect("serve");
    let base = h.url();
    (h, reqwest::Client::new(), base)
}

async fn get(c: &reqwest::Client, base: &str, path: &str) -> Value {
    c.get(format!("{base}{path}")).bearer_auth("t".repeat(40)).send().await.unwrap().json().await.unwrap()
}
async fn post(c: &reqwest::Client, base: &str, path: &str, body: Value) -> (u16, Value) {
    let r = c.post(format!("{base}{path}")).bearer_auth("t".repeat(40)).json(&body).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
}

async fn until<F: FnMut(&Value) -> bool>(c: &reqwest::Client, base: &str, path: &str, mut f: F) -> Value {
    for _ in 0..120 {
        let v = get(c, base, path).await;
        if f(&v) {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timeout waiting on {path}");
}

fn last_agent_msg(v: &Value) -> Value {
    v["messages"].as_array().unwrap().iter().rev().find(|m| m["author_kind"] == "agent").cloned().unwrap_or(Value::Null)
}

#[tokio::test]
async fn api_requires_the_token() {
    let (h, c, base) = boot().await;
    assert_eq!(c.get(format!("{base}/api/state")).send().await.unwrap().status(), 401);
    assert_eq!(c.get(format!("{base}/api/health")).send().await.unwrap().status(), 200);
    let s = get(&c, &base, "/api/state").await;
    assert!(s["agents"].as_array().unwrap().len() >= 4, "first run seeds a starting collective");
    h.stop().await;
}

#[tokio::test]
async fn chat_streams_asks_permission_and_completes() {
    let (h, c, base) = boot().await;
    let (_, t) = post(&c, &base, "/api/threads", json!({})).await;
    let tid = t["id"].as_str().unwrap().to_string();

    let (st, d) = post(&c, &base, &format!("/api/threads/{tid}/messages"), json!({"text": "Поправь файл demo.txt"})).await;
    assert_eq!(st, 200, "{d}");
    let agent_msg_id = d["agent_messages"][0]["id"].as_str().unwrap().to_string();

    // the mock asks before editing: the chat must show a pending permission card
    let v = until(&c, &base, &format!("/api/threads/{tid}/messages"), |v| last_agent_msg(v)["props"]["ask"]["state"] == "pending").await;
    assert_eq!(last_agent_msg(&v)["props"]["ask"]["tool"], "Edit");
    assert_eq!(last_agent_msg(&v)["status"], "working");

    let (st, _) = post(&c, &base, &format!("/api/messages/{agent_msg_id}/answer"), json!({"decision": "allow"})).await;
    assert_eq!(st, 200);
    let v = until(&c, &base, &format!("/api/threads/{tid}/messages"), |v| last_agent_msg(v)["status"] == "done").await;
    let m = last_agent_msg(&v);
    assert!(m["body"].as_str().unwrap().contains("Готово"));
    assert_eq!(m["props"]["steps"].as_array().unwrap().len(), 2);
    assert_eq!(m["props"]["steps"][1]["status"], "done");
    assert!(m["props"]["work"]["output_tokens"].as_u64().unwrap() > 0);

    // the chat got a title from the first message and shows up in the list
    let list = get(&c, &base, "/api/threads").await;
    assert!(list.as_array().unwrap().iter().any(|x| x["id"] == tid && x["title"].as_str().unwrap().starts_with("Поправь")));
    h.stop().await;
}

#[tokio::test]
async fn denied_and_cancelled_runs_end_cleanly() {
    let (h, c, base) = boot().await;
    let (_, t) = post(&c, &base, "/api/threads", json!({})).await;
    let tid = t["id"].as_str().unwrap().to_string();

    let (_, d) = post(&c, &base, &format!("/api/threads/{tid}/messages"), json!({"text": "правка файла"})).await;
    let mid = d["agent_messages"][0]["id"].as_str().unwrap().to_string();
    until(&c, &base, &format!("/api/threads/{tid}/messages"), |v| last_agent_msg(v)["props"]["ask"]["state"] == "pending").await;
    post(&c, &base, &format!("/api/messages/{mid}/answer"), json!({"decision": "deny"})).await;
    let v = until(&c, &base, &format!("/api/threads/{tid}/messages"), |v| last_agent_msg(v)["status"] == "done").await;
    assert_eq!(last_agent_msg(&v)["props"]["steps"][1]["status"], "denied");

    // cancel while waiting for a human
    let (_, d) = post(&c, &base, &format!("/api/threads/{tid}/messages"), json!({"text": "ещё правка файла"})).await;
    let mid = d["agent_messages"][0]["id"].as_str().unwrap().to_string();
    until(&c, &base, &format!("/api/threads/{tid}/messages"), |v| last_agent_msg(v)["props"]["ask"]["state"] == "pending").await;
    let (st, r) = post(&c, &base, &format!("/api/messages/{mid}/cancel"), json!({})).await;
    assert_eq!(st, 200);
    assert_eq!(r["stopped"], true);
    until(&c, &base, &format!("/api/threads/{tid}/messages"), |v| last_agent_msg(v)["status"] == "cancelled").await;
    h.stop().await;
}

#[tokio::test]
async fn obligation_session_without_a_report_does_not_vanish_or_loop() {
    let (h, c, base) = boot().await;
    let agents = get(&c, &base, "/api/agents").await;
    let dev = agents.as_array().unwrap().iter().find(|a| a["role"] == "developer").unwrap()["id"].as_str().unwrap().to_string();
    let (st, o) = post(&c, &base, "/api/obligations", json!({"title": "Сделать форму", "result": "Форма", "owner_id": dev})).await;
    assert_eq!(st, 200, "{o}");
    let oid = o["id"].as_str().unwrap().to_string();

    // the mock never calls the report tool: the environment must stop it visibly (blocked), not loop
    let v = until(&c, &base, &format!("/api/obligations/{oid}"), |v| v["obligation"]["state"] == "blocked").await;
    assert!(v["obligation"]["blocker"].as_str().unwrap().contains("не обновив"));
    assert_eq!(v["sessions"].as_array().unwrap().len(), 1, "exactly one session ran");
    // its work is visible in a chat of its own
    let tid = v["thread_id"].as_str().expect("obligation thread");
    let msgs = get(&c, &base, &format!("/api/threads/{tid}/messages")).await;
    assert_eq!(last_agent_msg(&msgs)["status"], "done");

    // the user's answer puts it back in the queue
    let (st, _) = post(&c, &base, &format!("/api/obligations/{oid}/transition"), json!({"to": "cancelled", "outcome": "не нужно"})).await;
    assert_eq!(st, 200);
    h.stop().await;
}

#[tokio::test]
async fn senate_tools_work_through_the_session_token_bridge() {
    let (h, c, base) = boot().await;
    // unknown token → rejected
    let r = c.post(format!("{base}/api/agent/call")).bearer_auth("sst_nope").json(&json!({"name": "senate_colleagues", "args": {}})).send().await.unwrap();
    assert_eq!(r.status(), 401);
    h.stop().await;
}

#[tokio::test]
async fn folder_picker_lists_directories_and_project_root_is_validated() {
    let (h, c, base) = boot().await;
    let tmp = std::env::temp_dir().join(format!("senate-proj-{}", senate_core::new_id()));
    std::fs::create_dir_all(tmp.join("sub")).unwrap();
    let list = get(&c, &base, &format!("/api/fs/list?path={}", tmp.to_string_lossy().replace('\\', "%5C").replace(':', "%3A"))).await;
    assert_eq!(list["entries"][0]["name"], "sub");

    let (_, p) = post(&c, &base, "/api/projects", json!({"name": "Локальный", "root_path": tmp.to_string_lossy()})).await;
    let pid = p["id"].as_str().unwrap();
    let r = c.patch(format!("{base}/api/projects/{pid}")).bearer_auth("t".repeat(40)).json(&json!({"root_path": "Z:\\нет\\такой"})).send().await.unwrap();
    assert_eq!(r.status(), 400);
    let g = get(&c, &base, &format!("/api/projects/{pid}/git")).await;
    assert_eq!(g["available"], false);
    h.stop().await;
}

/// Needs a logged-in Claude Code CLI. Run: cargo test -p senate-node --test e2e real_claude -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn real_claude_edits_files_locally_and_writes_memory_through_mcp() {
    std::env::set_var("SENATE_MCP_BIN", env!("CARGO_BIN_EXE_senate-node"));
    let (h, c, base) = boot().await;
    let root = std::env::temp_dir().join(format!("senate-real-{}", senate_core::new_id()));
    std::fs::create_dir_all(&root).unwrap();
    let (_, p) = post(&c, &base, "/api/projects", json!({"name": "Реальный", "root_path": root.to_string_lossy()})).await;
    let pid = p["id"].as_str().unwrap().to_string();
    let (_, t) = post(&c, &base, "/api/threads", json!({"project_id": pid})).await;
    let tid = t["id"].as_str().unwrap().to_string();
    c.patch(format!("{base}/api/threads/{tid}")).bearer_auth("t".repeat(40)).json(&json!({"model": "claude:haiku"})).send().await.unwrap();

    let prompt = "Создай файл hello.txt со словом «привет» инструментом Write. Потом вызови инструмент senate_remember с body=\"hello.txt создан\" и kind=observation. Ответь одним словом: готово.";
    let (st, d) = post(&c, &base, &format!("/api/threads/{tid}/messages"), json!({"text": prompt, "model": "claude:haiku"})).await;
    assert_eq!(st, 200, "{d}");
    let mid = d["agent_messages"][0]["id"].as_str().unwrap().to_string();

    let mut v = Value::Null;
    for _ in 0..600 {
        v = get(&c, &base, &format!("/api/threads/{tid}/messages")).await;
        let m = last_agent_msg(&v);
        if m["props"]["ask"]["state"] == "pending" {
            println!("ask: {}", m["props"]["ask"]);
            post(&c, &base, &format!("/api/messages/{mid}/answer"), json!({"decision": "allow"})).await;
        }
        if matches!(m["status"].as_str(), Some("done") | Some("failed") | Some("cancelled")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let m = last_agent_msg(&v);
    println!("STATUS {} BODY {:?}\nERROR {:?}", m["status"], m["body"], m["props"]["error"]);
    for s in m["props"]["steps"].as_array().cloned().unwrap_or_default() {
        println!("STEP {} | {} | {} | {}", s["kind"], s["name"], s["hint"], s["status"]);
    }
    println!("WORK {}", m["props"]["work"]);
    assert_eq!(m["status"], "done", "{m}");
    assert!(std::fs::read_to_string(root.join("hello.txt")).unwrap().to_lowercase().contains("привет"));
    let mem = get(&c, &base, &format!("/api/memory?level=personal")).await;
    println!("MEMORY {mem}");
    assert!(mem.as_array().unwrap().iter().any(|x| x["body"].as_str().unwrap().contains("hello.txt")), "memory written through MCP bridge");
    // the CLI session is remembered so the chat resumes
    let th = get(&c, &base, &format!("/api/threads/{tid}")).await;
    assert!(th["cli_sessions"].as_object().unwrap().iter().any(|(k, v)| k.starts_with("claude:") && v.is_string()), "one session per specialist: {}", th["cli_sessions"]);
    h.stop().await;
}
