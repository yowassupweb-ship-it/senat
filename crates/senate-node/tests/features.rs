//! Accounts, companies, MCP, API connections, character sheet, forum.

use senate_node::{serve, Config, Handle};
use serde_json::{json, Value};
use std::time::Duration;

const TOKEN: &str = "tttttttttttttttttttttttttttttttttttttttt";

async fn boot() -> (Handle, reqwest::Client, String) {
    let dir = std::env::temp_dir().join(format!("senate-feat-{}", senate_core::new_id()));
    let cfg = Config { data_dir: dir, bind: "127.0.0.1:0".parse().unwrap(), token: Some(TOKEN.into()), default_backend: Some("mock".into()), parallel: 4 };
    let h = serve(cfg).await.expect("serve");
    let base = h.url();
    (h, reqwest::Client::new(), base)
}

async fn get(c: &reqwest::Client, base: &str, path: &str) -> Value {
    c.get(format!("{base}{path}")).bearer_auth(TOKEN).send().await.unwrap().json().await.unwrap()
}
async fn post(c: &reqwest::Client, base: &str, path: &str, body: Value) -> (u16, Value) {
    let r = c.post(format!("{base}{path}")).bearer_auth(TOKEN).json(&body).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
}
async fn put(c: &reqwest::Client, base: &str, path: &str, body: Value) -> u16 {
    c.put(format!("{base}{path}")).bearer_auth(TOKEN).json(&body).send().await.unwrap().status().as_u16()
}

fn agent_id(agents: &Value, role: &str) -> String {
    agents.as_array().unwrap().iter().find(|a| a["role"] == role).unwrap()["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn a_freshly_opened_screen_gets_levels_and_open_work_of_every_specialist() {
    let (h, c, base) = boot().await;
    let state = get(&c, &base, "/api/state").await;
    let from_list = get(&c, &base, "/api/agents").await;
    let agents = state["agents"].as_array().unwrap();
    assert!(!agents.is_empty());
    for a in agents {
        assert!(a["level"].as_i64().unwrap_or(0) >= 1, "no level in /api/state: {a}");
        assert!(a["open_obligations"].is_number(), "no open work in /api/state: {a}");
    }
    assert_eq!(state["agents"], from_list, "the same view however the screen got its data");
    h.stop().await;
}

#[tokio::test]
async fn supported_agents_are_claude_codex_kimi_only() {
    let (h, c, base) = boot().await;
    let acc = get(&c, &base, "/api/accounts").await;
    let ids: Vec<&str> = acc.as_array().unwrap().iter().map(|a| a["provider"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["claude", "codex", "kimi"]);
    let (st, r) = post(&c, &base, "/api/accounts/gemini/login", json!({})).await;
    assert_eq!(st, 400, "{r}");
    h.stop().await;
}

#[tokio::test]
async fn apple_developer_mcp_is_preinstalled_for_developer_and_designer_only() {
    let (h, c, base) = boot().await;
    let agents = get(&c, &base, "/api/agents").await;
    let list = get(&c, &base, "/api/mcp").await;
    let apple = list.as_array().unwrap().iter().find(|m| m["name"] == "apple-developer").expect("preinstalled");
    assert_eq!(apple["trusted"], true);
    let holders: Vec<String> = apple["agents"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
    assert!(holders.contains(&agent_id(&agents, "developer")) && holders.contains(&agent_id(&agents, "designer")));
    assert!(!holders.contains(&agent_id(&agents, "analyst")), "not everybody gets every tool");
    h.stop().await;
}

#[tokio::test]
async fn mcp_belongs_to_specialists_and_its_secrets_stay_on_the_node() {
    let (h, c, base) = boot().await;
    let agents = get(&c, &base, "/api/agents").await;
    let designer = agent_id(&agents, "designer");
    let analyst = agent_id(&agents, "analyst");

    let (st, m) = post(&c, &base, "/api/mcp", json!({"name": "figma", "kind": "http", "url": "http://127.0.0.1:3845/mcp", "headers": {"Authorization": "Bearer s3cret-token"}, "agents": [designer]})).await;
    assert_eq!(st, 200, "{m}");
    assert!(!m.to_string().contains("s3cret-token"), "secret must never be returned");
    assert_eq!(m["header_keys"][0], "Authorization");
    assert!(!get(&c, &base, "/api/mcp").await.to_string().contains("s3cret-token"));
    assert_eq!(post(&c, &base, "/api/mcp", json!({"name": "senate", "kind": "stdio", "command": "x"})).await.0, 400);
    assert_eq!(post(&c, &base, "/api/mcp", json!({"name": "Bad Name", "kind": "stdio", "command": "x"})).await.0, 400);

    let eq = |s: &Value| s["equipment"]["mcp"].as_array().unwrap().iter().find(|x| x["name"] == "figma").unwrap()["equipped"].clone();
    assert_eq!(eq(&get(&c, &base, &format!("/api/agents/{designer}/sheet")).await), true);
    assert_eq!(eq(&get(&c, &base, &format!("/api/agents/{analyst}/sheet")).await), false);
    // giving access from the specialist's side
    assert_eq!(put(&c, &base, &format!("/api/agents/{analyst}/equipment"), json!({"mcp": [m["id"]]})).await, 200);
    assert_eq!(eq(&get(&c, &base, &format!("/api/agents/{analyst}/sheet")).await), true);
    let sheet = get(&c, &base, &format!("/api/agents/{designer}/sheet")).await;
    assert!(sheet["stats"]["level"].as_i64().unwrap() >= 1);
    h.stop().await;
}

/// Minimal HTTP server that records (request line, authorization header) and answers with JSON.
async fn mock_api() -> (String, std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let s2 = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = l.accept().await else { break };
            let s3 = s2.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = text.lines().next().unwrap_or("").to_string();
                let auth = text.lines().find(|l| l.to_lowercase().starts_with("authorization:")).unwrap_or("").to_string();
                s3.lock().unwrap().push((line, auth));
                let body = r#"{"hello":"world"}"#;
                let _ = sock.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).as_bytes()).await;
            });
        }
    });
    (format!("http://{addr}"), seen)
}

#[tokio::test]
async fn agents_call_connected_apis_through_the_node_and_never_see_the_key() {
    let (h, c, base) = boot().await;
    let (api_url, seen) = mock_api().await;
    let agents = get(&c, &base, "/api/agents").await;
    let dev = agent_id(&agents, "developer");
    let designer = agent_id(&agents, "designer");

    let (st, it) = post(&c, &base, "/api/integrations", json!({"name": "crm", "label": "CRM", "base_url": api_url, "auth_type": "bearer", "secret": "topsecret-key", "docs": "список клиентов: GET /clients", "write_mode": "approve", "agents": [dev]})).await;
    assert_eq!(st, 200, "{it}");
    assert_eq!(it["has_secret"], true);
    assert!(!it.to_string().contains("topsecret-key"));
    assert!(!get(&c, &base, "/api/integrations").await.to_string().contains("topsecret-key"));

    let bridge = |agent: &str| h.app.mint_session_token(senate_core::tools::ToolCtx { agent_id: agent.to_string(), ..Default::default() });
    let call = |tok: String, args: Value| {
        let c = c.clone();
        let base = base.clone();
        async move {
            let r = c.post(format!("{base}/api/agent/call")).bearer_auth(tok).json(&json!({"name": "senate_api_call", "args": args})).send().await.unwrap();
            (r.status().as_u16(), r.json::<Value>().await.unwrap_or(Value::Null))
        }
    };

    let (st, r) = call(bridge(&dev), json!({"service": "crm", "path": "/clients"})).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["result"]["body"]["hello"], "world");
    assert!(!r.to_string().contains("topsecret-key"), "the agent must not see the key");
    assert_eq!(seen.lock().unwrap()[0].1.to_lowercase(), "authorization: bearer topsecret-key");

    assert_eq!(call(bridge(&designer), json!({"service": "crm", "path": "/clients"})).await.0, 403, "not equipped");
    assert_eq!(call(bridge(&dev), json!({"service": "crm", "path": "http://evil.example/x"})).await.0, 400);
    assert_eq!(call(bridge(&dev), json!({"service": "crm", "path": "/a/../../b"})).await.0, 400);

    // a write waits for the user; nothing is sent before approval
    let before = seen.lock().unwrap().len();
    let (st, r) = call(bridge(&dev), json!({"service": "crm", "method": "POST", "path": "/clients", "body": {"name": "Ромашка"}})).await;
    assert_eq!(st, 200);
    assert_eq!(r["result"]["status"], "pending_approval");
    assert_eq!(seen.lock().unwrap().len(), before, "no request before approval");
    let ap = get(&c, &base, "/api/approvals").await[0].clone();
    assert_eq!(post(&c, &base, &format!("/api/approvals/{}/decide", ap["id"].as_str().unwrap()), json!({"approve": true})).await.0, 200);
    assert_eq!(seen.lock().unwrap().len(), before + 1);
    assert!(seen.lock().unwrap().last().unwrap().0.starts_with("POST /clients"));
    h.stop().await;
}

#[tokio::test]
async fn agents_can_ask_to_install_their_own_mcp_but_only_with_consent() {
    let (h, c, base) = boot().await;
    let agents = get(&c, &base, "/api/agents").await;
    let analyst = agent_id(&agents, "analyst");
    let tok = h.app.mint_session_token(senate_core::tools::ToolCtx { agent_id: analyst.clone(), ..Default::default() });
    let r = c
        .post(format!("{base}/api/agent/call"))
        .bearer_auth(&tok)
        .json(&json!({"name": "senate_mcp_install", "args": {"name": "playwright", "kind": "stdio", "command": "npx", "args": ["-y", "@playwright/mcp@latest"], "env_keys": ["API_TOKEN"], "note": "браузер для проверки страниц"}}))
        .send()
        .await
        .unwrap();
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["result"]["status"], "pending_approval", "{v}");
    assert!(get(&c, &base, "/api/mcp").await.as_array().unwrap().iter().all(|m| m["name"] != "playwright"), "nothing is installed before the user agrees");
    let ap = get(&c, &base, "/api/approvals").await[0].clone();
    assert_eq!(ap["action"], "mcp.install");
    assert_eq!(post(&c, &base, &format!("/api/approvals/{}/decide", ap["id"].as_str().unwrap()), json!({"approve": true})).await.0, 200);
    let m = get(&c, &base, "/api/mcp").await.as_array().unwrap().iter().find(|m| m["name"] == "playwright").cloned().expect("installed after consent");
    assert_eq!(m["agents"], json!([analyst]), "installed for the one who asked");
    assert_eq!(m["trusted"], false);
    assert_eq!(m["missing"], json!(["API_TOKEN"]), "the user is told which secrets are still needed");
    h.stop().await;
}

#[tokio::test]
async fn companies_group_projects_chats_and_decisions() {
    let (h, c, base) = boot().await;
    let (st, co) = post(&c, &base, "/api/companies", json!({"name": "Acme", "brief": "Строгий минимализм"})).await;
    assert_eq!(st, 200, "{co}");
    let cid = co["id"].as_str().unwrap();
    assert_eq!(post(&c, &base, "/api/companies", json!({"name": "Acme"})).await.0, 400, "names are unique");
    let (_, p) = post(&c, &base, "/api/projects", json!({"name": "Сайт Acme", "company_id": cid})).await;
    assert_eq!(p["company_id"], cid);
    assert_eq!(get(&c, &base, &format!("/api/companies/{cid}")).await["projects"].as_array().unwrap().len(), 1);
    let (st, dec) = post(&c, &base, &format!("/api/companies/{cid}/decisions"), json!({"title": "Только Inter"})).await;
    assert_eq!(st, 200);
    post(&c, &base, &format!("/api/decisions/{}/status", dec["id"].as_str().unwrap()), json!({"status": "accepted"})).await;
    assert_eq!(get(&c, &base, &format!("/api/companies/{cid}")).await["decisions"][0]["status"], "accepted");
    let (_, t) = post(&c, &base, "/api/threads", json!({"company_id": cid})).await;
    assert_eq!(t["company_id"], cid);
    h.stop().await;
}

#[tokio::test]
async fn a_forum_discusses_in_rounds_sums_up_with_a_plan_and_acts_only_when_the_user_accepts() {
    let (h, c, base) = boot().await;
    let agents = get(&c, &base, "/api/agents").await;
    let (a1, a2) = (agent_id(&agents, "architect"), agent_id(&agents, "developer"));
    let (_, p) = post(&c, &base, "/api/projects", json!({"name": "Платформа"})).await;

    // a forum needs at least two participants and a question
    assert_eq!(post(&c, &base, "/api/forums", json!({"topic": "x", "participants": [a1]})).await.0, 400);
    assert_eq!(post(&c, &base, "/api/forums", json!({"topic": "", "participants": [a1, a2]})).await.0, 400);

    let (st, t) = post(&c, &base, "/api/forums", json!({"topic": "Как перестроить платформу без простоя?", "outcome": "plan", "participants": [a1, a2], "rounds": 2, "project_id": p["id"]})).await;
    assert_eq!(st, 200, "{t}");
    let tid = t["id"].as_str().unwrap().to_string();
    assert_eq!(t["kind"], "forum");
    assert_eq!(t["mode"], "plan", "a forum is read-only: discussion, not work");

    // wait for the leader's summary with a plan
    let mut th = Value::Null;
    for _ in 0..200 {
        th = get(&c, &base, &format!("/api/threads/{tid}")).await;
        if th["meta"]["status"] == "concluded" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    assert_eq!(th["meta"]["status"], "concluded", "{th}");
    assert_eq!(th["meta"]["plan"]["work"].as_array().unwrap().len(), 2);
    assert_eq!(th["meta"]["plan"]["work"][0]["owner_id"], a1, "owners are resolved to real specialists: {}", th["meta"]["plan"]);

    // everyone spoke in each round (2 participants x 2 rounds) and the leader summed up
    let msgs = get(&c, &base, &format!("/api/threads/{tid}/messages")).await["messages"].as_array().unwrap().clone();
    let spoken: Vec<&Value> = msgs.iter().filter(|m| m["author_kind"] == "agent").collect();
    assert_eq!(spoken.len(), 5, "4 discussion turns + 1 summary");
    assert!(spoken.iter().all(|m| m["status"] == "done"));
    assert!(msgs.iter().any(|m| m["props"]["summary"] == true));

    // nothing became a task by itself
    assert!(get(&c, &base, "/api/obligations").await.as_array().unwrap().is_empty());
    // the thread shows as waiting for the user's decision
    let list = get(&c, &base, "/api/threads").await;
    assert_eq!(list.as_array().unwrap().iter().find(|x| x["id"] == tid).unwrap()["awaiting"], true);

    // the user accepts the plan: tasks (with the dependency) and the decision appear, once
    let (st, r) = post(&c, &base, &format!("/api/forums/{tid}/apply"), json!({})).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["tasks"], 2);
    let tasks = get(&c, &base, "/api/obligations?all=1").await;
    assert_eq!(tasks.as_array().unwrap().len(), 2);
    assert!(tasks.as_array().unwrap().iter().any(|o| o["state"] == "waiting"), "the second task waits for the first");
    assert_eq!(post(&c, &base, &format!("/api/forums/{tid}/apply"), json!({})).await.0, 409, "a plan is applied once");
    let decisions = get(&c, &base, &format!("/api/projects/{}", p["id"].as_str().unwrap())).await["decisions"].clone();
    assert_eq!(decisions[0]["status"], "accepted");
    h.stop().await;
}

#[tokio::test]
async fn a_forum_can_be_stopped_and_a_human_message_reopens_it_for_one_more_round() {
    let (h, c, base) = boot().await;
    let agents = get(&c, &base, "/api/agents").await;
    let (a1, a2) = (agent_id(&agents, "architect"), agent_id(&agents, "designer"));
    let (_, t) = post(&c, &base, "/api/forums", json!({"topic": "Какой делаем онбординг?", "outcome": "analysis", "participants": [a1, a2], "rounds": 1})).await;
    let tid = t["id"].as_str().unwrap().to_string();
    for _ in 0..200 {
        if get(&c, &base, &format!("/api/threads/{tid}")).await["meta"]["status"] == "concluded" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    let before = get(&c, &base, &format!("/api/threads/{tid}/messages")).await["messages"].as_array().unwrap().len();
    // the human adds a thought: the discussion resumes for one more round and sums up again
    let (st, _) = post(&c, &base, &format!("/api/threads/{tid}/messages"), json!({"text": "А если без регистрации?"})).await;
    assert_eq!(st, 200);
    for _ in 0..200 {
        let th = get(&c, &base, &format!("/api/threads/{tid}")).await;
        if th["meta"]["status"] == "concluded" && th["meta"]["round"] == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    let th = get(&c, &base, &format!("/api/threads/{tid}")).await;
    assert_eq!(th["meta"]["round"], 2);
    assert!(get(&c, &base, &format!("/api/threads/{tid}/messages")).await["messages"].as_array().unwrap().len() > before + 3);
    h.stop().await;
}

#[tokio::test]
async fn a_screenshot_can_be_attached_and_the_agent_is_pointed_at_it() {
    let (h, c, base) = boot().await;
    let png = b"\x89PNG\r\n\x1a\n-pretend-pixels-".to_vec();

    // upload: the raw file as the body, its name in a header
    let r = c.post(format!("{base}/api/uploads")).bearer_auth(TOKEN).header("x-filename", "%D1%81%D0%BA%D1%80%D0%B8%D0%BD.png").header("content-type", "image/png").body(png.clone()).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let up: Value = r.json().await.unwrap();
    assert_eq!(up["name"], "скрин.png");
    assert_eq!(up["type"], "image/png");
    assert!(up.get("path").is_none(), "where the file lies on the disk is not the UI's business");
    let id = up["id"].as_str().unwrap().to_string();

    // it comes back, as an inline image, to whoever holds the token — and only to them
    let url = format!("{base}{}", up["url"].as_str().unwrap());
    let got = c.get(format!("{url}?token={TOKEN}")).send().await.unwrap();
    assert_eq!(got.status(), 200);
    assert_eq!(got.headers()["content-type"], "image/png");
    assert_eq!(got.headers()["x-content-type-options"], "nosniff");
    assert_eq!(got.bytes().await.unwrap().to_vec(), png);
    assert_eq!(c.get(&url).send().await.unwrap().status(), 401);

    // a page uploaded under the app's own address is never rendered as a page
    let html: Value = c.post(format!("{base}/api/uploads")).bearer_auth(TOKEN).header("x-filename", "x.html").header("content-type", "text/html").body("<script>alert(1)</script>").send().await.unwrap().json().await.unwrap();
    let hr = c.get(format!("{base}{}?token={TOKEN}", html["url"].as_str().unwrap())).send().await.unwrap();
    assert_eq!(hr.headers()["content-type"], "application/octet-stream");
    assert!(hr.headers()["content-disposition"].to_str().unwrap().starts_with("attachment"));
    // empty and path-like names do not break anything
    assert_eq!(c.post(format!("{base}/api/uploads")).bearer_auth(TOKEN).body(Vec::<u8>::new()).send().await.unwrap().status(), 400);
    assert_eq!(c.get(format!("{base}/api/uploads/..%2F..%2Fx/token?token={TOKEN}")).send().await.unwrap().status(), 404);

    // a message refers to the file by id; the client's own claims about it are ignored
    let agents = get(&c, &base, "/api/agents").await;
    let dev = agent_id(&agents, "developer");
    let (_, th) = post(&c, &base, "/api/threads", json!({"agent_id": dev})).await;
    let tid = th["id"].as_str().unwrap();
    let (st, d) = post(&c, &base, &format!("/api/threads/{tid}/messages"), json!({"text": "", "attachments": [{"id": id, "path": "C:/secret.txt", "name": "evil"}, {"id": "nope"}]})).await;
    assert_eq!(st, 200, "a picture alone is a message: {d}");
    let att = &d["user_message"]["props"]["attachments"];
    assert_eq!(att.as_array().unwrap().len(), 1, "unknown ids are dropped");
    assert_eq!(att[0]["name"], "скрин.png");
    let stored = att[0]["path"].as_str().unwrap();
    assert!(stored.contains(&id) && !stored.contains("secret"), "{stored}");
    // nothing to send at all is still refused
    let (st, _) = post(&c, &base, &format!("/api/threads/{tid}/messages"), json!({"text": "  ", "attachments": [{"id": "nope"}]})).await;
    assert_eq!(st, 400);
    h.stop().await;
}

#[tokio::test]
async fn the_live_list_shows_who_is_working_and_on_what() {
    let (h, c, base) = boot().await;
    assert_eq!(get(&c, &base, "/api/live").await, json!([]), "nobody works: nothing listed");
    let agents = get(&c, &base, "/api/agents").await;
    let dev = agent_id(&agents, "developer");
    let (_, th) = post(&c, &base, "/api/threads", json!({"agent_id": dev, "title": "Круизный сайт"})).await;
    let tid = th["id"].as_str().unwrap();
    post(&c, &base, &format!("/api/threads/{tid}/messages"), json!({"text": "Поправь файл demo.txt"})).await;
    let live = get(&c, &base, "/api/live").await;
    let row = &live.as_array().unwrap()[0];
    assert_eq!(row["agent_id"], dev);
    assert_eq!(row["thread_id"], tid);
    assert_eq!(row["kind"], "chat");
    assert!(matches!(row["status"].as_str(), Some("queued" | "running")), "{row}");
    assert!(row["message_id"].is_string() && row["since"].is_number());
    h.stop().await;
}

#[tokio::test]
async fn a_specialist_can_have_a_picture() {
    let (h, c, base) = boot().await;
    let agents = get(&c, &base, "/api/agents").await;
    let dev = agent_id(&agents, "developer");
    assert_eq!(agents.as_array().unwrap().iter().find(|a| a["id"] == dev).unwrap()["avatar"], "", "none at first");

    let up = |name: &'static str, ctype: &'static str, body: &'static [u8]| {
        let (c, base) = (c.clone(), base.clone());
        async move {
            c.post(format!("{base}/api/uploads")).bearer_auth(TOKEN).header("x-filename", name).header("content-type", ctype).body(body.to_vec()).send().await.unwrap().json::<Value>().await.unwrap()
        }
    };
    let png = up("лицо.png", "image/png", b"\x89PNG\r\n\x1a\n-face-").await;
    let r = c.patch(format!("{base}/api/agents/{dev}")).bearer_auth(TOKEN).json(&json!({"avatar_id": png["id"]})).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let ag: Value = r.json().await.unwrap();
    assert_eq!(ag["avatar"], png["url"], "the node stores its own address for the file");

    // it shows in the list and in the state a screen starts from
    let listed = get(&c, &base, "/api/agents").await;
    assert_eq!(listed.as_array().unwrap().iter().find(|a| a["id"] == dev).unwrap()["avatar"], png["url"]);
    let state = get(&c, &base, "/api/state").await;
    assert_eq!(state["agents"].as_array().unwrap().iter().find(|a| a["id"] == dev).unwrap()["avatar"], png["url"]);

    // only a picture will do, and only one the node holds
    let txt = up("note.txt", "text/plain", b"hello").await;
    let r = c.patch(format!("{base}/api/agents/{dev}")).bearer_auth(TOKEN).json(&json!({"avatar_id": txt["id"]})).send().await.unwrap();
    assert_eq!(r.status(), 400);
    let r = c.patch(format!("{base}/api/agents/{dev}")).bearer_auth(TOKEN).json(&json!({"avatar_id": "nope"})).send().await.unwrap();
    assert_eq!(r.status(), 400);
    assert_eq!(get(&c, &base, "/api/agents").await.as_array().unwrap().iter().find(|a| a["id"] == dev).unwrap()["avatar"], png["url"], "a refused change leaves the picture as it was");

    // back to the letter
    let r = c.patch(format!("{base}/api/agents/{dev}")).bearer_auth(TOKEN).json(&json!({"avatar_id": null})).send().await.unwrap();
    assert_eq!(r.json::<Value>().await.unwrap()["avatar"], "");
    h.stop().await;
}

#[tokio::test]
async fn allow_always_remembers_what_the_card_promised_and_can_be_taken_back() {
    let (h, c, base) = boot().await;
    let agents = get(&c, &base, "/api/agents").await;
    let dev = agent_id(&agents, "developer");
    let (_, th) = post(&c, &base, "/api/threads", json!({"agent_id": dev})).await;
    let tid = th["id"].as_str().unwrap().to_string();

    let wait = |want: &'static str| {
        let (c, base, tid) = (c.clone(), base.clone(), tid.clone());
        async move {
            for _ in 0..150 {
                let msgs = get(&c, &base, &format!("/api/threads/{tid}/messages")).await;
                let last = msgs["messages"].as_array().unwrap().iter().rev().find(|m| m["author_kind"] == "agent").cloned().unwrap_or(Value::Null);
                let hit = match want {
                    "pending" => last["props"]["ask"]["state"] == "pending",
                    _ => last["status"] == "done",
                };
                if hit {
                    return last;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            panic!("never reached {want}");
        }
    };

    // 1. the first time the person is asked; the card says what "always" would mean
    let (_, d) = post(&c, &base, &format!("/api/threads/{tid}/messages"), json!({"text": "Поправь файл demo.txt"})).await;
    let m1 = d["agent_messages"][0]["id"].as_str().unwrap().to_string();
    let asked = wait("pending").await;
    assert!(asked["props"]["ask"]["always"].as_str().unwrap_or("").starts_with("Запись в файлы папки"), "{asked}");
    let (st, _) = post(&c, &base, &format!("/api/messages/{m1}/answer"), json!({"decision": "allow", "always": true})).await;
    assert_eq!(st, 200);
    wait("done").await;
    let rules = get(&c, &base, "/api/permission-rules").await;
    assert_eq!(rules.as_array().unwrap().len(), 1, "{rules}");
    assert_eq!(rules[0]["kind"], "path");

    // 2. the same kind of thing again: no question at all (nobody answers here, so a question would hang)
    post(&c, &base, &format!("/api/threads/{tid}/messages"), json!({"text": "Ещё правка файла demo.txt"})).await;
    let second = wait("done").await;
    assert!(second["props"]["ask"].is_null(), "no card the second time: {second}");

    // 3. a plain "allow" remembers nothing
    let (_, th2) = post(&c, &base, "/api/threads", json!({"agent_id": dev})).await;
    assert_eq!(get(&c, &base, "/api/permission-rules").await.as_array().unwrap().len(), 1);
    let _ = th2;

    // 4. taking it back: the question returns
    let id = rules[0]["id"].as_str().unwrap();
    assert_eq!(c.delete(format!("{base}/api/permission-rules/{id}")).bearer_auth(TOKEN).send().await.unwrap().status(), 200);
    assert_eq!(get(&c, &base, "/api/permission-rules").await, json!([]));
    post(&c, &base, &format!("/api/threads/{tid}/messages"), json!({"text": "И ещё правка файла demo.txt"})).await;
    wait("pending").await;
    h.stop().await;
}

#[tokio::test]
async fn accesses_are_managed_through_the_api_and_the_secret_never_comes_back() {
    let (h, c, base) = boot().await;
    let agents = get(&c, &base, "/api/agents").await;
    let dev = agent_id(&agents, "developer");

    let (st, acc) = post(&c, &base, "/api/accesses", json!({"name": "prod-vps", "label": "Боевой", "kind": "server", "host": "ssh://deploy@203.0.113.7", "login": "deploy", "secret": "s3cr3t-key", "note": "деплой"})).await;
    assert_eq!(st, 200, "{acc}");
    assert_eq!(acc["has_secret"], true);
    assert!(!acc.to_string().contains("s3cr3t"));
    assert_eq!(acc["grants"], json!({}), "nobody has it until the person says so");
    let id = acc["id"].as_str().unwrap().to_string();

    // grant, change, the list: the secret is never in any answer
    let r = c.patch(format!("{base}/api/accesses/{id}")).bearer_auth(TOKEN).json(&json!({"grants": {dev.clone(): "ask"}, "note": "деплой и логи"})).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let upd: Value = r.json().await.unwrap();
    assert_eq!(upd["grants"][&dev], "ask");
    assert_eq!(upd["has_secret"], true, "editing the rest keeps the secret");
    let listed = get(&c, &base, "/api/accesses").await;
    assert!(!listed.to_string().contains("s3cr3t"), "{listed}");
    assert_eq!(listed[0]["note"], "деплой и логи");

    // bad input is refused with a reason
    let (st, e) = post(&c, &base, "/api/accesses", json!({"name": "Bad Name", "host": "x"})).await;
    assert_eq!(st, 400, "{e}");
    let r = c.patch(format!("{base}/api/accesses/{id}")).bearer_auth(TOKEN).json(&json!({"grants": {dev.clone(): "always"}})).send().await.unwrap();
    assert_eq!(r.status(), 400);
    let (st, _) = post(&c, &base, "/api/accesses", json!({"name": "prod-vps", "host": "x"})).await;
    assert_eq!(st, 400, "names are unique");

    // the log shows who did what, and removing the access removes it all
    let log = get(&c, &base, &format!("/api/accesses/{id}/log")).await;
    assert!(log.as_array().unwrap().iter().any(|e| e["kind"] == "access.granted"), "{log}");
    assert_eq!(c.delete(format!("{base}/api/accesses/{id}")).bearer_auth(TOKEN).send().await.unwrap().status(), 200);
    assert_eq!(get(&c, &base, "/api/accesses").await, json!([]));
    h.stop().await;
}

#[tokio::test]
async fn routines_can_be_set_run_by_hand_and_a_skills_own_are_made_once() {
    let (h, c, base) = boot().await;
    let agents = get(&c, &base, "/api/agents").await;
    let dev = agent_id(&agents, "developer");
    let slug = agents.as_array().unwrap().iter().find(|a| a["id"] == dev).unwrap()["slug"].as_str().unwrap().to_string();

    let (st, r) = post(&c, &base, "/api/routines", json!({"agent_id": dev, "name": "Утренний обход", "cron": "30 8 * * 1-5", "title": "Обход сайта", "prompt": "Пройди по главным страницам и проверь, что всё открывается"})).await;
    assert_eq!(st, 200, "{r}");
    assert!(r["next_run_at"].is_number() && r["agent_name"].is_string());
    let (st, e) = post(&c, &base, "/api/routines", json!({"agent_id": dev, "name": "Плохая", "cron": "99 * * * *"})).await;
    assert_eq!(st, 400, "{e}");

    // by hand: a task for the specialist appears, the schedule stays
    let id = r["id"].as_str().unwrap().to_string();
    let (st, run) = post(&c, &base, &format!("/api/routines/{id}/run"), json!({})).await;
    assert_eq!(st, 200, "{run}");
    assert!(run["title"].as_str().unwrap().starts_with("Обход сайта · 20"));
    let after = get(&c, &base, &format!("/api/routines?agent={dev}")).await;
    assert_eq!(after[0]["next_run_at"], r["next_run_at"]);

    // the specialist's sheet shows them, together with their skills
    let sheet = get(&c, &base, &format!("/api/agents/{dev}/sheet")).await;
    assert_eq!(sheet["routines"].as_array().unwrap().len(), 1);
    assert_eq!(sheet["skills"], json!([]));

    // switching it off sticks
    let off = c.patch(format!("{base}/api/routines/{id}")).bearer_auth(TOKEN).json(&json!({"enabled": false})).send().await.unwrap();
    assert_eq!(off.json::<Value>().await.unwrap()["next_run_at"], Value::Null);
    assert_eq!(c.delete(format!("{base}/api/routines/{id}")).bearer_auth(TOKEN).send().await.unwrap().status(), 200);
    assert_eq!(get(&c, &base, "/api/routines").await, json!([]));
    let _ = slug;
    h.stop().await;
}

#[tokio::test]
async fn a_specialist_can_call_a_colleague_and_gets_the_answer_back() {
    let (h, c, base) = boot().await;
    let agents = get(&c, &base, "/api/agents").await;
    let dev = agent_id(&agents, "developer");
    let arch = agent_id(&agents, "architect");
    let arch_slug = agents.as_array().unwrap().iter().find(|a| a["id"] == arch).unwrap()["slug"].as_str().unwrap().to_string();
    let (_, th) = post(&c, &base, "/api/threads", json!({"agent_id": dev})).await;
    let tid = th["id"].as_str().unwrap().to_string();

    // the developer, in the middle of their work, asks the architect
    let tok = h.app.mint_session_token(senate_core::tools::ToolCtx { agent_id: dev.clone(), thread_id: Some(tid.clone()), ..Default::default() });
    let r = c.post(format!("{base}/api/agent/call")).bearer_auth(&tok).json(&json!({"name": "senate_call_colleague", "args": {"to": arch_slug, "request": "Как лучше разложить модули каталога?"}})).send().await.unwrap();
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["result"]["status"], "called", "{v}");
    // oneself, nobody, and an empty request are refused
    for bad in [json!({"to": "razrabotchik", "request": "x"}), json!({"to": "nobody", "request": "x"}), json!({"to": arch_slug, "request": "  "})] {
        let r = c.post(format!("{base}/api/agent/call")).bearer_auth(&tok).json(&json!({"name": "senate_call_colleague", "args": bad})).send().await.unwrap();
        assert!(r.status().is_client_error(), "{bad}");
    }

    // the architect answers in the same chat, and the answer comes back to the developer as a turn of their own
    let mut seen = Value::Null;
    for _ in 0..200 {
        let msgs = get(&c, &base, &format!("/api/threads/{tid}/messages")).await;
        let list = msgs["messages"].as_array().unwrap().clone();
        let answered = list.iter().any(|m| m["author_id"] == arch && m["status"] == "done" && m["props"]["asked_by"] == dev);
        let back = list.iter().any(|m| m["author_id"] == dev && m["status"] == "done" && m["props"]["reply_from"] == arch);
        if answered && back {
            seen = msgs;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(!seen.is_null(), "no answer round trip");
    let list = seen["messages"].as_array().unwrap();
    assert!(list.iter().any(|m| m["author_id"] == dev && m["body"].as_str().unwrap_or("").starts_with("→ @")), "the request is visible in the chat");
    h.stop().await;
}

/// An image service: answers `/images/generations` with a one-pixel PNG inline, and records what it was asked.
async fn mock_images() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let s2 = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = l.accept().await else { break };
            let s3 = s2.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16384];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                s3.lock().unwrap().push(text.clone());
                // "ABC" as base64: enough to prove the bytes travel end to end
                let body = r#"{"data":[{"b64_json":"QUJD"}]}"#;
                let _ = sock
                    .write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).as_bytes())
                    .await;
            });
        }
    });
    (format!("http://{addr}"), seen)
}

#[tokio::test]
async fn a_specialist_makes_a_picture_and_the_key_stays_on_the_node() {
    let (h, c, base) = boot().await;
    let (img_url, seen) = mock_images().await;
    let agents = get(&c, &base, "/api/agents").await;
    let designer = agent_id(&agents, "designer");
    let dev = agent_id(&agents, "developer");

    // the person connects the service once and gives it to the designer only
    let (st, it) = post(&c, &base, "/api/integrations", json!({"name": "images", "label": "Картинки", "base_url": img_url, "auth_type": "bearer", "secret": "img-key-123", "docs": "Генерация картинок", "agents": [designer]})).await;
    assert_eq!(st, 200, "{it}");
    assert!(!get(&c, &base, "/api/integrations").await.to_string().contains("img-key-123"), "the key never comes back");

    // the designer asks for a picture in a chat of their own
    let (_, th) = post(&c, &base, "/api/threads", json!({"agent_id": designer})).await;
    let tid = th["id"].as_str().unwrap().to_string();
    let (_, d) = post(&c, &base, &format!("/api/threads/{tid}/messages"), json!({"text": "нарисуй"})).await;
    let mid = d["agent_messages"][0]["id"].as_str().unwrap().to_string();
    let sid = h.app.store.list_sessions(None, None, None, 20).unwrap().into_iter().find(|s| s.message_id.as_deref() == Some(&mid)).unwrap().id;
    let tok = h.app.mint_session_token(senate_core::tools::ToolCtx { agent_id: designer.clone(), session_id: Some(sid), thread_id: Some(tid.clone()), ..Default::default() });

    let r = c.post(format!("{base}/api/agent/call")).bearer_auth(&tok).json(&json!({"name": "senate_image_generate", "args": {"prompt": "Морской круиз на закате, акварель", "size": "1024x1024", "model": "gpt-image-1"}})).send().await.unwrap();
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["result"]["status"], "generated", "{v}");

    // what the service was asked, and with whose key
    let asked = seen.lock().unwrap().join("\n");
    assert!(asked.contains("POST /images/generations"), "{asked}");
    assert!(asked.contains("Bearer img-key-123"), "the node sends the key, not the agent");
    assert!(asked.contains("gpt-image-1") && asked.contains("1024x1024"), "{asked}");

    // the picture is on disk, named after the prompt, and holds the bytes the service sent
    let path = std::path::PathBuf::from(v["result"]["path"].as_str().unwrap());
    assert_eq!(std::fs::read(&path).unwrap(), b"ABC");
    assert!(path.to_string_lossy().contains("морской-круиз"), "{}", path.display());
    assert!(!v.to_string().contains("img-key-123"));

    // ...and the person sees it: it is attached to the specialist's message in the chat
    let msgs = get(&c, &base, &format!("/api/threads/{tid}/messages")).await;
    let m = msgs["messages"].as_array().unwrap().iter().find(|m| m["id"] == mid.as_str()).cloned().unwrap();
    assert_eq!(m["props"]["attachments"][0]["type"], "image/png");
    let shown = c.get(format!("{base}{}?token={TOKEN}", m["props"]["attachments"][0]["url"].as_str().unwrap())).send().await.unwrap();
    assert_eq!(shown.bytes().await.unwrap().to_vec(), b"ABC");

    // a specialist without the connection gets a refusal that says what to do
    let tok2 = h.app.mint_session_token(senate_core::tools::ToolCtx { agent_id: dev.clone(), ..Default::default() });
    let r = c.post(format!("{base}/api/agent/call")).bearer_auth(&tok2).json(&json!({"name": "senate_image_generate", "args": {"prompt": "кот"}})).send().await.unwrap();
    assert_eq!(r.status(), 403);
    assert!(r.json::<Value>().await.unwrap()["error"].as_str().unwrap().contains("не подключён"));

    // an empty description is refused before anything is called
    let before = seen.lock().unwrap().len();
    let r = c.post(format!("{base}/api/agent/call")).bearer_auth(&tok).json(&json!({"name": "senate_image_generate", "args": {"prompt": "   "}})).send().await.unwrap();
    assert_eq!(r.status(), 400);
    assert_eq!(seen.lock().unwrap().len(), before);
    h.stop().await;
}
