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
async fn retried_chat_send_is_durable_and_never_dispatches_twice() {
    let (h, c, base) = boot().await;
    let (_, t) = post(&c, &base, "/api/threads", json!({})).await;
    let tid = t["id"].as_str().unwrap();
    let path = format!("/api/threads/{tid}/messages");
    let body = json!({"text":"Проверка доставки", "client_id":"delivery-test-1"});
    let (first, second) = tokio::join!(post(&c, &base, &path, body.clone()), post(&c, &base, &path, body.clone()));
    assert_eq!(first.0, 200);
    assert_eq!(second.0, 200);
    assert_eq!(first.1["user_message"]["id"], second.1["user_message"]["id"]);
    assert_eq!(first.1["agent_messages"][0]["id"], second.1["agent_messages"][0]["id"]);
    assert!(first.1["sessions"][0].get("prompt").is_none());
    until(&c, &base, &path, |v| last_agent_msg(v)["status"] == "done").await;
    let cfg = h.app.cfg.clone();
    h.stop().await;
    let restarted = serve(cfg).await.unwrap();
    let base = restarted.url();
    let (status, replay) = post(&c, &base, &path, body).await;
    assert_eq!(status, 200);
    assert_eq!(replay["agent_messages"][0]["status"], "done");
    let all = get(&c, &base, &path).await;
    assert_eq!(all["messages"].as_array().unwrap().iter().filter(|m| m["author_kind"] == "user").count(), 1);
    let (status, _) = post(&c, &base, &path, json!({"text":"Другой текст", "client_id":"delivery-test-1"})).await;
    assert_eq!(status, 400);
    let page = get(&c, &base, &format!("{path}?limit=1")).await;
    assert_eq!(page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(page["has_more"], true);
    restarted.stop().await;
}

#[tokio::test]
async fn histories_are_compressed_but_live_events_are_not_buffered() {
    let (h, c, base) = boot().await;
    let plain = c.get(format!("{base}/api/state")).bearer_auth("t".repeat(40)).send().await.unwrap().bytes().await.unwrap();
    let zipped = c.get(format!("{base}/api/state")).bearer_auth("t".repeat(40)).header("Accept-Encoding", "gzip").send().await.unwrap();
    assert_eq!(zipped.headers()["content-encoding"], "gzip");
    let zipped = zipped.bytes().await.unwrap();
    assert!(zipped.len() < plain.len() / 2, "{} vs {} bytes", zipped.len(), plain.len());
    let events = c.get(format!("{base}/api/events")).bearer_auth("t".repeat(40)).header("Accept-Encoding", "gzip").send().await.unwrap();
    assert!(!events.headers().contains_key("content-encoding"));
    drop(events);
    h.stop().await;
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


#[tokio::test]
async fn forum_acceptance_preserves_every_dependency_and_rejects_invalid_graphs() {
    use senate_core::model::{NewAgent, NewProject, NewForum};
    let (h,c,base)=boot().await;
    let a=h.app.store.create_agent(NewAgent {name:"Writer".into(),..Default::default()}).unwrap();
    let b=h.app.store.create_agent(NewAgent {name:"Designer".into(),..Default::default()}).unwrap();
    let p=h.app.store.create_project(NewProject {name:"Team test".into(),..Default::default()}).unwrap();
    h.app.store.update_project(&p.id,senate_core::store::ProjectPatch {status:Some("paused".into()),..Default::default()}).unwrap();
    let t=h.app.store.create_forum(NewForum {topic:"Plan".into(),participants:vec![a.id.clone(),b.id.clone()],project_id:Some(p.id.clone()),..Default::default()}).unwrap();
    let plan=json!({"work":[
        {"title":"Page","owner_id":a.id,"depends_on":["Text","Design"]},
        {"title":"Text","owner_id":a.id},
        {"title":"Design","owner_id":b.id}
    ]});
    h.app.store.set_forum_meta(&t.id,&json!({"status":"concluded","plan":plan})).unwrap();
    let path=format!("/api/forums/{}/apply",t.id);
    let (first,second)=tokio::join!(post(&c,&base,&path,json!({})),post(&c,&base,&path,json!({})));
    assert!(matches!((first.0,second.0),(200,409)|(409,200)));
    let tasks=h.app.store.list_obligations(None,Some(&p.id),false).unwrap();
    assert_eq!(tasks.len(),3);
    assert_eq!(tasks.iter().find(|o|o.title=="Page").unwrap().dependencies.len(),2);
    let bad=h.app.store.create_forum(NewForum {topic:"Bad plan".into(),participants:vec![a.id.clone(),b.id.clone()],project_id:Some(p.id.clone()),..Default::default()}).unwrap();
    h.app.store.set_forum_meta(&bad.id,&json!({"status":"concluded","plan":{"work":[{"title":"A","owner_id":a.id,"depends_on":["B"]},{"title":"B","owner_id":b.id,"depends_on":["A"]}]}})).unwrap();
    assert_eq!(post(&c,&base,&format!("/api/forums/{}/apply",bad.id),json!({})).await.0,409);
    assert_eq!(h.app.store.list_obligations(None,Some(&p.id),false).unwrap().len(),3);
    h.stop().await;
}

#[tokio::test]
async fn project_chat_reuse_workspace_and_context_controls() {
    let (h,c,base)=boot().await;
    let (_,p)=post(&c,&base,"/api/projects",json!({"name":"Long project"})).await;
    let pid=p["id"].as_str().unwrap();
    let (_,t)=post(&c,&base,"/api/threads",json!({"project_id":pid,"reuse_project":true})).await;
    let (_,again)=post(&c,&base,"/api/threads",json!({"project_id":pid,"reuse_project":true})).await;
    assert_eq!(again["id"],t["id"]);
    let tid=t["id"].as_str().unwrap();
    let endpoint=format!("/api/threads/{tid}/workspace");
    let (status,_)=post(&c,&base,&endpoint,json!({"path":"relative"})).await;
    assert_eq!(status,400);
    let (status,bound)=post(&c,&base,&endpoint,json!({"path":h.app.cfg.data_dir.to_string_lossy(),"machine":""})).await;
    assert_eq!(status,200,"{bound}");
    assert_eq!(bound["status"],"bound");
    let path=format!("/api/threads/{tid}/context");
    let (status,ctx)=post(&c,&base,&path,json!({"auto":false})).await;
    assert_eq!(status,200); assert_eq!(ctx["auto"],false);
    assert_eq!(get(&c,&base,&path).await["auto"],false);
    h.stop().await;
}

#[tokio::test]
async fn compact_completion_resets_session_but_cancellation_preserves_it() {
    use senate_core::{model::*,store::ThreadPatch};
    let (h,c,base)=boot().await;
    for (backend,success) in [("mock",true),("mock",false)] {
        let a=h.app.store.create_agent(NewAgent{name:format!("Compact {backend} {success}"),model_pref:backend.into(),..Default::default()}).unwrap();
        let t=h.app.store.create_thread(NewThread{agent_id:Some(a.id.clone()),..Default::default()}).unwrap();
        h.app.store.patch_thread(&t.id,ThreadPatch{mode:Some("auto".into()),..Default::default()}).unwrap();
        h.app.store.set_cli_session_for(&t.id,backend,&a.id,"previous-context").unwrap();
        let original=h.app.store.post_message(&t.id,"agent",Some(&a.id),"Original history",json!({"work":{"context_tokens":90,"context_window":100}}),MessageStatus::Done,None).unwrap();
        let (_,queued)=post(&c,&base,&format!("/api/threads/{}/context",t.id),json!({"compact":true})).await;
        assert_eq!(queued["pending"],1);
        let messages=format!("/api/threads/{}/messages",t.id);
        let waiting=until(&c,&base,&messages,|v|v["messages"].as_array().unwrap().iter().any(|m|m["props"]["compact"]==true && m["props"]["ask"]["state"]=="pending")).await;
        let summary=waiting["messages"].as_array().unwrap().iter().find(|m|m["props"]["compact"]==true).unwrap();
        let mid=summary["id"].as_str().unwrap();
        let action=if success {"answer"} else {"cancel"};
        post(&c,&base,&format!("/api/messages/{mid}/{action}"),json!({"decision":"allow"})).await;
        until(&c,&base,&messages,|v|v["messages"].as_array().unwrap().iter().any(|m|m["id"]==mid && m["status"]==if success {"done"} else {"cancelled"})).await;
        let current=h.app.store.get_thread(&t.id).unwrap();
        assert_eq!(senate_core::Store::compact_summary(&current,&a.id).is_some(),success);
        assert_eq!(h.app.store.cli_session_for(&current,backend,&a.id).is_none(),success);
        assert_eq!(h.app.store.get_message(&original.id).unwrap().body,"Original history");
    }
    h.stop().await;
}
