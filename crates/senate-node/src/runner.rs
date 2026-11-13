//! Where a specialist works: on the node (the default) or on a person's own computer.
//!
//! A specialist set to "local" has its turns carried out by the Senate app on that computer: the node (the hub, which
//! keeps all data) queues the job, the app picks it up by long polling, runs the agent CLI there with its own folders
//! and tools, and streams progress, permission questions and the result back. Memory, chats and tasks stay on the node.
//!
//! Hub side: `Hub`, `remote_run` and the `/api/runner/*` handlers in api.rs. App side: `run_client`.

use crate::App;
use senate_core::model::PermissionMode;
use senate_runtime::{Approver, Decision, Event, ExtraMcp, McpSpec, Outcome, PermissionRequest, Progress, RunSpec, Step, Work};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, watch, Notify};

// ---------------------------------------------------------------- wire format

pub(crate) fn progress_to_json(p: &Progress) -> Value {
    let mut m = serde_json::Map::new();
    if let Some(v) = &p.phase {
        m.insert("phase".into(), json!(v));
    }
    if let Some(v) = &p.body {
        m.insert("body".into(), json!(v));
    }
    if let Some(v) = &p.steps {
        m.insert("steps".into(), serde_json::to_value(v).unwrap_or_default());
    }
    if let Some(v) = &p.model {
        m.insert("model".into(), json!(v));
    }
    if let Some(v) = &p.session_id {
        m.insert("session_id".into(), json!(v));
    }
    if let Some(v) = &p.rate_limit {
        m.insert("rate_limit".into(), v.clone());
    }
    if let Some(v) = &p.limits {
        m.insert("limits".into(), v.clone());
    }
    if let Some(v) = &p.reasoning {
        m.insert("reasoning".into(), json!(v));
    }
    Value::Object(m)
}

pub(crate) fn progress_from_json(v: &Value) -> Progress {
    let s = |k: &str| v[k].as_str().map(String::from);
    Progress {
        phase: s("phase"),
        body: s("body"),
        steps: v.get("steps").and_then(|x| serde_json::from_value::<Vec<Step>>(x.clone()).ok()),
        model: s("model"),
        session_id: s("session_id"),
        rate_limit: v.get("rate_limit").cloned(),
        limits: v.get("limits").cloned(),
        reasoning: s("reasoning"),
    }
}

fn merge(dst: &mut Progress, src: Progress) {
    macro_rules! take {
        ($f:ident) => {
            if src.$f.is_some() {
                dst.$f = src.$f;
            }
        };
    }
    take!(phase);
    take!(body);
    take!(steps);
    take!(model);
    take!(session_id);
    take!(rate_limit);
    take!(limits);
    take!(reasoning);
}

fn req_to_json(r: &PermissionRequest) -> Value {
    json!({"tool_use_id": r.tool_use_id, "tool": r.tool, "input": r.input, "reason": r.reason})
}
fn req_from_json(v: &Value) -> PermissionRequest {
    PermissionRequest { tool_use_id: v["tool_use_id"].as_str().unwrap_or("").into(), tool: v["tool"].as_str().unwrap_or("").into(), input: v["input"].clone(), reason: v["reason"].as_str().unwrap_or("").into() }
}

fn outcome_to_json(o: &Outcome) -> Value {
    json!({"text": o.text, "session_id": o.session_id, "work": o.work, "failure": o.failure, "stopped": o.stopped, "model": o.model, "steps": o.steps})
}
fn outcome_from_json(v: &Value) -> Outcome {
    Outcome {
        text: v["text"].as_str().unwrap_or("").into(),
        session_id: v["session_id"].as_str().map(String::from),
        work: serde_json::from_value::<Work>(v["work"].clone()).unwrap_or_default(),
        failure: v["failure"].as_str().map(String::from),
        stopped: v["stopped"].as_bool().unwrap_or(false),
        model: v["model"].as_str().unwrap_or("").into(),
        steps: serde_json::from_value::<Vec<Step>>(v["steps"].clone()).unwrap_or_default(),
    }
}

fn pairs(v: &Value) -> Vec<(String, String)> {
    v.as_array().map(|a| a.iter().filter_map(|p| Some((p[0].as_str()?.to_string(), p[1].as_str()?.to_string()))).collect()).unwrap_or_default()
}

fn spec_to_json(s: &RunSpec, slug: &str, skills_dir: &str) -> Value {
    json!({
        "backend": s.backend, "effort": s.effort, "mode": s.mode.as_str(), "cwd": s.cwd.to_string_lossy(),
        "system_prompt": s.system_prompt, "prompt": s.prompt, "resume": s.resume, "bare": s.bare, "env": s.env, "run_id": s.run_id,
        "mcp": s.mcp.as_ref().map(|m| json!({"command": m.command, "args": m.args, "env": m.env})),
        "extra_mcp": s.extra_mcp.iter().map(|m| json!({"name": m.name, "kind": m.kind, "command": m.command, "args": m.args, "env": m.env, "url": m.url, "headers": m.headers, "trusted": m.trusted})).collect::<Vec<_>>(),
        "slug": slug, "skills_dir": skills_dir,
        "extra_dirs": s.extra_dirs.iter().map(|d| d.to_string_lossy()).collect::<Vec<_>>(),
    })
}

// ---------------------------------------------------------------- hub side

pub(crate) enum RunnerMsg {
    Event(Event),
    Ask { req: PermissionRequest, reply: oneshot::Sender<Decision> },
    Done(Outcome),
}

pub(crate) struct JobHub {
    pub to_hub: mpsc::UnboundedSender<RunnerMsg>,
    pub cancel: watch::Receiver<bool>,
}

/// The person's computers that run the Senate app and take work from this node. Several may be connected at once:
/// a job or a look at a folder goes to the computer it is addressed to ("machine"), or to any when it names none.
#[derive(Default)]
pub struct Hub {
    queue: Mutex<VecDeque<Value>>,
    pub(crate) jobs: Mutex<HashMap<String, JobHub>>,
    notify: Notify,
    /// machine id → when it last asked for work
    seen: Mutex<HashMap<String, Instant>>,
    rpcs: Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>,
}

const ALIVE: Duration = Duration::from_secs(45);

impl Hub {
    /// Any computer connected.
    pub fn online(&self) -> bool {
        self.seen.lock().unwrap().values().any(|t| t.elapsed() < ALIVE)
    }
    pub fn machine_online(&self, id: &str) -> bool {
        self.seen.lock().unwrap().get(id).map(|t| t.elapsed() < ALIVE).unwrap_or(false)
    }
    /// The computer seen most recently: where a job addressed to no computer in particular goes.
    pub fn latest_machine(&self) -> Option<String> {
        self.seen.lock().unwrap().iter().filter(|(_, t)| t.elapsed() < ALIVE).min_by_key(|(_, t)| t.elapsed()).map(|(k, _)| k.clone())
    }
    pub(crate) fn mark(&self, machine: &str) {
        self.seen.lock().unwrap().insert(machine.to_string(), Instant::now());
    }

    fn take_for(&self, machine: &str) -> Option<Value> {
        let mut q = self.queue.lock().unwrap();
        let i = q.iter().position(|j| j["machine"].as_str().map(|m| m.is_empty() || m == machine).unwrap_or(true))?;
        q.remove(i)
    }

    /// A computer asks for work: wait up to ~25 s for a job or a request addressed to it.
    pub async fn poll(&self, machine: &str) -> Option<Value> {
        self.mark(machine);
        let until = Instant::now() + Duration::from_secs(25);
        loop {
            if let Some(j) = self.take_for(machine) {
                return Some(j);
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            let _ = tokio::time::timeout(left.min(Duration::from_secs(5)), self.notify.notified()).await;
            self.mark(machine);
        }
    }

    pub(crate) fn job(&self, id: &str) -> Option<(mpsc::UnboundedSender<RunnerMsg>, bool)> {
        self.jobs.lock().unwrap().get(id).map(|j| (j.to_hub.clone(), *j.cancel.borrow()))
    }

    /// Ask a computer to carry out one `workspace::op` and wait for the answer.
    pub async fn rpc(&self, machine: &str, op: &str, args: Value, limit: Duration) -> Result<Value, String> {
        if !self.machine_online(machine) {
            return Err("компьютер с этой папкой сейчас не в сети: откройте на нём Senate".into());
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (tx, rx) = oneshot::channel();
        self.rpcs.lock().unwrap().insert(id.clone(), tx);
        self.queue.lock().unwrap().push_back(json!({"id": id, "rpc": op, "args": args, "machine": machine}));
        self.notify.notify_waiters();
        let r = tokio::time::timeout(limit, rx).await;
        self.rpcs.lock().unwrap().remove(&id);
        self.queue.lock().unwrap().retain(|j| j["id"].as_str() != Some(&id));
        match r {
            Ok(Ok(v)) => v,
            _ => Err("компьютер не ответил вовремя".into()),
        }
    }

    pub(crate) fn rpc_done(&self, id: &str, result: Result<Value, String>) {
        if let Some(tx) = self.rpcs.lock().unwrap().remove(id) {
            let _ = tx.send(result);
        }
    }
}

/// One turn of a specialist who works on a person's computer.
pub(crate) async fn remote_run(app: &Arc<App>, mut spec: RunSpec, slug: &str, machine: Option<&str>, approver: &dyn Approver, tx: senate_runtime::EventTx, cancel: senate_runtime::CancelRx) -> Outcome {
    let fail = |why: &str| Outcome { failure: Some(why.to_string()), ..Default::default() };
    let machine = match machine {
        Some(m) => {
            if !app.runner.machine_online(m) {
                let name = app.store.list_nodes().ok().and_then(|l| l.into_iter().find(|n| n.id == m)).map(|n| n.name).unwrap_or_else(|| "с папкой проекта".into());
                return fail(&format!("Папка проекта на компьютере «{name}», а он сейчас не в сети. Откройте на нём Senate — работа продолжится там."));
            }
            m.to_string()
        }
        None => match app.runner.latest_machine() {
            Some(m) => m,
            None => return fail("Специалист работает локально, но приложение Senate на вашем компьютере не подключено. Запустите его или переключите специалиста на «На ноде»."),
        },
    };
    let id = uuid::Uuid::new_v4().simple().to_string();
    let (mtx, mut mrx) = mpsc::unbounded_channel::<RunnerMsg>();
    app.runner.jobs.lock().unwrap().insert(id.clone(), JobHub { to_hub: mtx, cancel: cancel.clone() });
    let skills_dir = crate::skills::root(&app.cfg.data_dir, slug).to_string_lossy().into_owned();
    spec.work_dir = PathBuf::new();
    let mut job = spec_to_json(&spec, slug, &skills_dir);
    job["id"] = json!(id);
    job["machine"] = json!(machine);
    app.runner.queue.lock().unwrap().push_back(job);
    app.runner.notify.notify_waiters();

    let started = Instant::now();
    let mut picked = false;
    let mut cancelled_at: Option<Instant> = None;
    // Questions wait for a person, but must not hold up the loop: the card that shows the question travels as an
    // ordinary event through the same channel and would otherwise sit behind the very answer it is waiting for.
    let mut asks: futures_util::stream::FuturesUnordered<Pin<Box<dyn Future<Output = ()> + Send + '_>>> = futures_util::stream::FuturesUnordered::new();
    let outcome = loop {
        tokio::select! {
            m = mrx.recv() => match m {
                Some(RunnerMsg::Event(e)) => { picked = true; let _ = tx.send(e); }
                Some(RunnerMsg::Ask { req, reply }) => {
                    picked = true;
                    let mut cancel = cancel.clone();
                    asks.push(Box::pin(async move {
                        let d = if approver.preapproved(&req) {
                            Decision::Allow
                        } else {
                            tokio::select! {
                                d = approver.ask(req) => d,
                                _ = cancel.changed() => Decision::Deny("Остановлено.".into()),
                            }
                        };
                        let _ = reply.send(d);
                    }));
                }
                Some(RunnerMsg::Done(o)) => break o,
                None => break fail("Связь с приложением на компьютере потеряна."),
            },
            _ = futures_util::StreamExt::next(&mut asks), if !asks.is_empty() => {}
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                // Stopped by the person: the computer gets a few seconds to wind down and report; after that the
                // specialist is freed here anyway — a stuck CLI over there must not keep them "working".
                if *cancel.borrow() {
                    let at = *cancelled_at.get_or_insert_with(Instant::now);
                    if at.elapsed() > Duration::from_secs(8) {
                        break Outcome { stopped: true, ..Default::default() };
                    }
                }
                if !picked && started.elapsed() > Duration::from_secs(60) {
                    break fail("Приложение Senate на компьютере не взяло задачу за минуту. Проверьте, что оно запущено и подключено к этой ноде.");
                }
                if !app.runner.machine_online(&machine) && picked {
                    break fail("Приложение Senate на компьютере перестало отвечать.");
                }
            }
        }
    };
    app.runner.jobs.lock().unwrap().remove(&id);
    app.runner.queue.lock().unwrap().retain(|j| j["id"].as_str() != Some(&id));
    outcome
}

/// What the runner sends about a job: progress, a question, the end.
pub(crate) async fn job_event(app: &Arc<App>, id: &str, v: Value) -> Result<Value, String> {
    let (to_hub, cancel) = app.runner.job(id).ok_or("задача не найдена или уже завершена")?;
    match v["kind"].as_str().unwrap_or("") {
        "progress" => {
            let _ = to_hub.send(RunnerMsg::Event(Event::Progress(Box::new(progress_from_json(&v["progress"])))));
        }
        "ask_card" => {
            let _ = to_hub.send(RunnerMsg::Event(Event::Ask(req_from_json(&v["req"]))));
        }
        "answered" => {
            let _ = to_hub.send(RunnerMsg::Event(Event::Answered { tool_use_id: v["tool_use_id"].as_str().unwrap_or("").into(), allowed: v["allowed"].as_bool().unwrap_or(false) }));
        }
        "done" => {
            let _ = to_hub.send(RunnerMsg::Done(outcome_from_json(&v["outcome"])));
        }
        _ => {}
    }
    Ok(json!({"cancel": cancel}))
}

pub(crate) async fn job_ask(app: &Arc<App>, id: &str, v: Value) -> Result<Value, String> {
    let (to_hub, _) = app.runner.job(id).ok_or("задача не найдена или уже завершена")?;
    let (reply, rx) = oneshot::channel();
    to_hub.send(RunnerMsg::Ask { req: req_from_json(&v), reply }).map_err(|_| "задача завершена")?;
    Ok(match rx.await {
        Ok(Decision::Allow) => json!({"allow": true}),
        Ok(Decision::Deny(r)) => json!({"allow": false, "reason": r}),
        Err(_) => json!({"allow": false, "reason": "Ожидание разрешения прервано."}),
    })
}

/// The files of a specialist's skills, for the app to put next to its own work.
pub(crate) fn skills_files(app: &App, slug: &str) -> Value {
    use base64::Engine;
    let root = crate::skills::root(&app.cfg.data_dir, slug);
    let mut out = vec![];
    let mut total = 0usize;
    fn walk(dir: &Path, root: &Path, out: &mut Vec<Value>, total: &mut usize) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().map(|n| n == "node_modules" || n == ".git").unwrap_or(false) {
                    continue;
                }
                walk(&p, root, out, total);
            } else if let Ok(b) = std::fs::read(&p) {
                if b.len() > 3_000_000 || *total + b.len() > 30_000_000 {
                    continue;
                }
                *total += b.len();
                let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                out.push(json!({"path": rel, "b64": base64::engine::general_purpose::STANDARD.encode(b)}));
            }
        }
    }
    if crate::skills::list(&app.cfg.data_dir, slug).is_empty() {
        return json!({"files": []});
    }
    walk(&root, &root, &mut out, &mut total);
    json!({"files": out})
}

// ---------------------------------------------------------------- the app's side

struct RunnerApprover {
    http: reqwest::Client,
    hub: String,
    token: String,
    job: String,
    cancel: watch::Receiver<bool>,
}

impl Approver for RunnerApprover {
    fn ask(&self, req: PermissionRequest) -> Pin<Box<dyn Future<Output = Decision> + Send + '_>> {
        Box::pin(async move {
            let mut cancel = self.cancel.clone();
            let call = self.http.post(format!("{}/api/runner/job/{}/ask", self.hub, self.job)).bearer_auth(&self.token).json(&req_to_json(&req)).send();
            let resp = tokio::select! {
                r = call => r,
                _ = cancel.changed() => return Decision::Deny("Остановлено.".into()),
            };
            match resp {
                Ok(r) => match r.json::<Value>().await {
                    Ok(v) if v["allow"].as_bool().unwrap_or(false) => Decision::Allow,
                    Ok(v) => Decision::Deny(v["reason"].as_str().unwrap_or("Отклонено.").to_string()),
                    Err(_) => Decision::Deny("Нет ответа от ноды.".into()),
                },
                Err(_) => Decision::Deny("Нет связи с нодой.".into()),
            }
        })
    }
}

/// An MCP server that starts a program by an absolute path works only where that path exists: a server set up on
/// the person's Windows computer (`C:\…\markitdown-mcp.exe`) cannot start on the Linux node, and the other way
/// round. Such a server is left out instead of failing every run where it is missing.
pub(crate) fn usable_here(m: &ExtraMcp) -> bool {
    if m.kind == "http" {
        return true;
    }
    let looks_absolute = |s: &str| {
        let b = s.as_bytes();
        s.starts_with('/') || (b.len() > 2 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
    };
    let cmd_ok = !looks_absolute(&m.command) || Path::new(&m.command).exists();
    // a script handed to an interpreter (`node /data/mbox-mcp/server.mjs`) must be there as well
    let args_ok = m.args.iter().filter(|a| looks_absolute(a) && (a.ends_with(".js") || a.ends_with(".mjs") || a.ends_with(".py") || a.ends_with(".exe"))).all(|a| Path::new(a).exists());
    cmd_ok && args_ok
}

/// Run on a person's computer: keep asking the hub for work and carry it out with the agent CLIs found here.
pub async fn run_client(hub: String, token: String, data_dir: PathBuf, mut stop: watch::Receiver<bool>) {
    let hub = hub.trim_end_matches('/').to_string();
    let http = reqwest::Client::builder().timeout(Duration::from_secs(45)).build().unwrap();
    let (machine_id, machine_name) = crate::workspace::machine_identity(&data_dir);
    let _ = crate::workspace::WS_ROOT.set(data_dir.join("runner").join("workspaces"));
    // the tools are looked up once, off the polling path
    let env = tokio::task::spawn_blocking(crate::workspace::environment).await.unwrap_or(Value::Null);
    let me = json!({"id": machine_id, "name": machine_name, "os": std::env::consts::OS, "env": env});
    // this computer's load, for the strip next to the search
    tokio::spawn(crate::metrics::report_loop(hub.clone(), token.clone(), machine_id.clone(), stop.clone()));
    loop {
        if *stop.borrow() {
            return;
        }
        let r = http.post(format!("{hub}/api/runner/poll")).bearer_auth(&token).json(&json!({"machine": me})).send().await;
        match r {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(v) = resp.json::<Value>().await {
                    if let Some(job) = v.get("job").filter(|j| !j.is_null()) {
                        if let Some(op) = job["rpc"].as_str() {
                            // a look at a folder: quick, answered at once
                            let (op, args, id) = (op.to_string(), job["args"].clone(), job["id"].as_str().unwrap_or("").to_string());
                            let (http, hub, token) = (http.clone(), hub.clone(), token.clone());
                            tokio::spawn(async move {
                                let r = tokio::task::spawn_blocking(move || crate::workspace::op(&op, &args)).await.unwrap_or_else(|_| Err("сбой".into()));
                                let body = match r {
                                    Ok(v) => json!({"ok": true, "result": v}),
                                    Err(e) => json!({"ok": false, "error": e}),
                                };
                                let _ = http.post(format!("{hub}/api/runner/rpc/{id}")).bearer_auth(&token).json(&body).send().await;
                            });
                            continue;
                        }
                        let mut job = job.clone();
                        job["machine_name"] = json!(machine_name);
                        job["machine_env"] = me["env"].clone();
                        tokio::spawn(handle_job(hub.clone(), token.clone(), data_dir.clone(), job));
                        continue;
                    }
                }
            }
            _ => {
                tokio::select! { _ = tokio::time::sleep(Duration::from_secs(5)) => {}, _ = stop.changed() => {} }
            }
        }
    }
}

async fn handle_job(hub: String, token: String, data_dir: PathBuf, job: Value) {
    let id = job["id"].as_str().unwrap_or("").to_string();
    let slug = job["slug"].as_str().unwrap_or("agent").to_string();
    let safe: String = slug.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    let long = reqwest::Client::new();
    let post = {
        let (long, hub, token) = (long.clone(), hub.clone(), token.clone());
        move |path: String, body: Value| {
            let (long, hub, token) = (long.clone(), hub.clone(), token.clone());
            async move { long.post(format!("{hub}{path}")).bearer_auth(token).timeout(Duration::from_secs(30)).json(&body).send().await.ok()?.json::<Value>().await.ok() }
        }
    };

    // the specialist's skills, copied beside the work
    let local_skills = data_dir.join("runner").join("skills").join(&safe);
    let mut prompt = job["system_prompt"].as_str().unwrap_or("").to_string();
    if let Ok(r) = long.get(format!("{hub}/api/runner/skills")).query(&[("slug", &slug)]).bearer_auth(&token).timeout(Duration::from_secs(60)).send().await {
        if let Ok(v) = r.json::<Value>().await {
            use base64::Engine;
            for f in v["files"].as_array().cloned().unwrap_or_default() {
                let rel = f["path"].as_str().unwrap_or("");
                if rel.is_empty() || rel.contains("..") {
                    continue;
                }
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(f["b64"].as_str().unwrap_or("")) {
                    let p = local_skills.join(rel);
                    if std::fs::read(&p).map(|old| old != bytes).unwrap_or(true) {
                        if let Some(d) = p.parent() {
                            let _ = std::fs::create_dir_all(d);
                        }
                        let _ = std::fs::write(&p, bytes);
                    }
                }
            }
        }
    }
    if let Some(hub_dir) = job["skills_dir"].as_str().filter(|d| !d.is_empty()) {
        prompt = prompt.replace(hub_dir, &local_skills.to_string_lossy());
    }

    // the working folder: the hub's own path means nothing here unless it exists
    let hub_cwd = PathBuf::from(job["cwd"].as_str().unwrap_or(""));
    let ws_root = data_dir.join("runner").join("workspaces");
    // a private folder (no project folder here): it follows the specialist between the node and this computer
    let private = !hub_cwd.is_dir();
    let cwd = if private { ws_root.join(&safe) } else { hub_cwd };
    let _ = std::fs::create_dir_all(&cwd);
    if private {
        pull_workspaces(&long, &hub, &token, &ws_root).await;
    }
    // where the specialist is and what this computer has
    prompt += &crate::workspace::environment_block_for(job["machine_name"].as_str().unwrap_or("этот компьютер"), &cwd, &job["machine_env"], job["backend"].as_str().unwrap_or(""));
    if private {
        prompt += &format!(
            "Это твоя личная папка; она общая с сервером: перед работой сюда пришли файлы с сервера, после работы твои изменения уйдут обратно. Папки коллег рядом: {}\\<username> (например, ..\\dev). Их не меняй — скопируй к себе, если нужно править.\n",
            ws_root.display()
        );
    }
    let work_dir = data_dir.join("runner").join("run");
    let _ = std::fs::create_dir_all(&work_dir);

    let mode = PermissionMode::parse(job["mode"].as_str().unwrap_or("edit")).unwrap_or(PermissionMode::Edit);
    let me = std::env::current_exe().ok().map(|p| p.to_string_lossy().into_owned());
    let mcp = job.get("mcp").filter(|m| !m.is_null()).map(|m| McpSpec {
        command: me.clone().unwrap_or_else(|| m["command"].as_str().unwrap_or("").into()),
        args: m["args"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        // the bridge talks to the hub the way this app does
        env: pairs(&m["env"]).into_iter().map(|(k, v)| if k == "SENATE_HUB" { (k, hub.clone()) } else { (k, v) }).collect(),
    });
    let extra_mcp = job["extra_mcp"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|m| ExtraMcp {
                    name: m["name"].as_str().unwrap_or("").into(),
                    kind: m["kind"].as_str().unwrap_or("stdio").into(),
                    command: m["command"].as_str().unwrap_or("").into(),
                    args: m["args"].as_array().map(|x| x.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default(),
                    env: pairs(&m["env"]),
                    url: m["url"].as_str().unwrap_or("").into(),
                    headers: pairs(&m["headers"]),
                    trusted: m["trusted"].as_bool().unwrap_or(false),
                })
                .filter(usable_here)
                // `{slug}` in a server's arguments is the specialist: their own browser profile, for one
                .map(|mut m: ExtraMcp| {
                    m.args = m.args.iter().map(|a| a.replace("{slug}", &safe)).collect();
                    m
                })
                .collect()
        })
        .unwrap_or_default();
    let spec = RunSpec {
        backend: job["backend"].as_str().unwrap_or("").into(),
        effort: job["effort"].as_str().map(String::from),
        mode,
        cwd,
        system_prompt: prompt,
        prompt: job["prompt"].as_str().unwrap_or("").into(),
        resume: job["resume"].as_str().map(String::from),
        mcp,
        extra_mcp,
        extra_dirs: job["extra_dirs"].as_array().map(|a| a.iter().filter_map(|d| d.as_str()).map(PathBuf::from).collect()).unwrap_or_default(),
        bare: job["bare"].as_bool().unwrap_or(false),
        env: pairs(&job["env"]),
        work_dir,
        run_id: job["run_id"].as_str().unwrap_or("run").into(),
    };

    let (cancel_tx, cancel_rx) = watch::channel(false);
    let cancel_tx = Arc::new(cancel_tx);
    let (tx, mut rx) = mpsc::unbounded_channel::<Event>();

    // Forward what the agent does: progress in small batches, questions at once.
    let fwd = {
        let (id, cancel_tx, post) = (id.clone(), cancel_tx.clone(), post.clone());
        tokio::spawn(async move {
            let mut acc = Progress::default();
            let mut dirty = false;
            let mut tick = tokio::time::interval(Duration::from_millis(300));
            let path = format!("/api/runner/job/{id}/event");
            loop {
                tokio::select! {
                    ev = rx.recv() => match ev {
                        None => break,
                        Some(Event::Progress(p)) => { merge(&mut acc, *p); dirty = true; }
                        Some(Event::Ask(req)) => {
                            if dirty { let _ = post(path.clone(), json!({"kind": "progress", "progress": progress_to_json(&std::mem::take(&mut acc))})).await; dirty = false; }
                            let _ = post(path.clone(), json!({"kind": "ask_card", "req": req_to_json(&req)})).await;
                        }
                        Some(Event::Answered { tool_use_id, allowed }) => {
                            let _ = post(path.clone(), json!({"kind": "answered", "tool_use_id": tool_use_id, "allowed": allowed})).await;
                        }
                    },
                    _ = tick.tick() => {
                        let body = if dirty { json!({"kind": "progress", "progress": progress_to_json(&std::mem::take(&mut acc))}) } else { json!({"kind": "ping"}) };
                        dirty = false;
                        if let Some(r) = post(path.clone(), body).await {
                            if r["cancel"].as_bool().unwrap_or(false) { let _ = cancel_tx.send(true); }
                        }
                    }
                }
            }
            if dirty {
                let _ = post(path, json!({"kind": "progress", "progress": progress_to_json(&acc)})).await;
            }
        })
    };

    let approver = RunnerApprover { http: long.clone(), hub: hub.clone(), token: token.clone(), job: id.clone(), cancel: cancel_rx.clone() };
    let started = std::time::SystemTime::now();
    let outcome = senate_runtime::run(spec, &approver, tx, cancel_rx).await;
    let _ = fwd.await;
    // what the specialist made in their own folder goes back to the node before the turn is reported done
    if private {
        push_workspace(&long, &hub, &token, &ws_root, &safe, started).await;
    }
    let _ = post(format!("/api/runner/job/{id}/event"), json!({"kind": "done", "outcome": outcome_to_json(&outcome)})).await;
}

/// Before a turn in a private folder: take from the node what is newer there (all specialists' folders).
async fn pull_workspaces(http: &reqwest::Client, hub: &str, token: &str, ws_root: &Path) -> usize {
    let Ok(r) = http.get(format!("{hub}/api/runner/workspaces")).bearer_auth(token).timeout(Duration::from_secs(30)).send().await else { return 0 };
    let Ok(v) = r.json::<Value>().await else { return 0 };
    let mut n = 0;
    for f in v["files"].as_array().cloned().unwrap_or_default() {
        let rel = f["path"].as_str().unwrap_or("");
        let Some(local) = crate::workspace::sync_path(ws_root, rel) else { continue };
        let (size, mtime) = (f["size"].as_u64().unwrap_or(0), f["mtime"].as_i64().unwrap_or(0));
        if !crate::workspace::needs_update(&local, size, mtime) {
            continue;
        }
        let Ok(resp) = http.get(format!("{hub}/api/runner/workspaces/file")).query(&[("path", rel)]).bearer_auth(token).timeout(Duration::from_secs(120)).send().await else { continue };
        if !resp.status().is_success() {
            continue;
        }
        if let Ok(bytes) = resp.bytes().await {
            if crate::workspace::write_synced(&local, &bytes, mtime).is_ok() {
                n += 1;
            }
        }
    }
    n
}

/// After a turn: what changed in the specialist's own folder since it started goes back to the node.
async fn push_workspace(http: &reqwest::Client, hub: &str, token: &str, ws_root: &Path, slug: &str, since: std::time::SystemTime) {
    let since_ms = since.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0) - 2000;
    let own = ws_root.join(slug);
    let files = tokio::task::spawn_blocking(move || crate::workspace::manifest(&own)).await.unwrap_or_default();
    for f in files.into_iter().filter(|f| f["mtime"].as_i64().unwrap_or(0) >= since_ms) {
        let rel = format!("{slug}/{}", f["path"].as_str().unwrap_or(""));
        let Some(local) = crate::workspace::sync_path(ws_root, &rel) else { continue };
        let Ok(bytes) = tokio::fs::read(&local).await else { continue };
        let _ = http
            .put(format!("{hub}/api/runner/workspaces/file"))
            .query(&[("path", rel.as_str()), ("slug", slug), ("mtime", &f["mtime"].as_i64().unwrap_or(0).to_string())])
            .bearer_auth(token)
            .timeout(Duration::from_secs(120))
            .body(bytes)
            .send()
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn work_goes_to_the_computer_it_is_addressed_to() {
        let hub = Arc::new(Hub::default());
        hub.mark("home");
        hub.mark("office");
        hub.queue.lock().unwrap().push_back(json!({"id": "j1", "machine": "office"}));
        hub.queue.lock().unwrap().push_back(json!({"id": "j2", "machine": ""}));
        // the home computer does not take the office's job, but takes one addressed to anyone
        assert_eq!(hub.take_for("home").unwrap()["id"], "j2");
        assert!(hub.take_for("home").is_none());
        assert_eq!(hub.take_for("office").unwrap()["id"], "j1");

        // a look at a folder: asked of the office, answered by it
        let h = hub.clone();
        let asker = tokio::spawn(async move { h.rpc("office", "tree", json!({"root": "E:/x"}), Duration::from_secs(5)).await });
        let req = loop {
            if let Some(r) = hub.take_for("office") {
                break r;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert_eq!(req["rpc"], "tree");
        hub.rpc_done(req["id"].as_str().unwrap(), Ok(json!({"entries": []})));
        assert_eq!(asker.await.unwrap().unwrap(), json!({"entries": []}));

        // a computer that is away is reported at once, not waited for
        assert!(hub.rpc("laptop", "tree", json!({}), Duration::from_secs(5)).await.unwrap_err().contains("не в сети"));
    }

    #[test]
    fn an_mcp_server_that_lives_elsewhere_is_left_out() {
        let m = |cmd: &str, args: &[&str]| ExtraMcp { name: "x".into(), kind: "stdio".into(), command: cmd.into(), args: args.iter().map(|s| s.to_string()).collect(), env: vec![], url: String::new(), headers: vec![], trusted: false };
        assert!(usable_here(&m("npx", &["-y", "pkg"])), "a command found on PATH");
        assert!(!usable_here(&m("Z:\\nowhere\\tool.exe", &[])), "a program at a path that is not here");
        assert!(!usable_here(&m("node", &["/nowhere/mbox-mcp-server.mjs"])), "a script that is not here");
        let http = ExtraMcp { kind: "http".into(), url: "https://x".into(), ..m("", &[]) };
        assert!(usable_here(&http));
    }
}
