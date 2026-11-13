//! Claude Code CLI backend: `claude -p` with stream-json in both directions. Permission questions
//! arrive as `control_request` / `can_use_tool` and are answered through our policy and, when a
//! human must decide, through the chat.

use crate::policy::{self, Verdict};
use crate::{clip, cli, Approver, CancelRx, Decision, Event, EventTx, Outcome, PermissionRequest, Progress, RunSpec, Step, Work};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::ChildStdin;

const MAX_STEP_INPUT: usize = 20_000;
const MAX_STEP_OUTPUT: usize = 6_000;
const MAX_STEPS: usize = 600;
const RUN_LIMIT: std::time::Duration = std::time::Duration::from_secs(2 * 60 * 60);

const TOOLS: &str = "Read,Edit,Write,Glob,Grep,Bash,PowerShell,WebSearch,WebFetch,TodoWrite,Task,ToolSearch";

fn display_name(name: &str) -> String {
    name.strip_prefix("mcp__senate__").unwrap_or(name).to_string()
}

fn base(p: &str) -> String {
    std::path::Path::new(p).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string())
}

/// Short human line for a tool call: what is being done, to what.
pub fn hint_of(name: &str, input: &Value) -> String {
    let s = |k: &str| input.get(k).and_then(|v| v.as_str()).unwrap_or("");
    let path = ["file_path", "path", "notebook_path"].iter().map(|k| s(k)).find(|v| !v.is_empty()).unwrap_or("");
    match name {
        "Read" => format!("Читает {}", base(path)),
        "Edit" | "MultiEdit" | "NotebookEdit" => format!("Правит {}", base(path)),
        "Write" => format!("Пишет {}", base(path)),
        "Bash" | "PowerShell" => {
            let d = s("description");
            if d.is_empty() { format!("Команда: {}", clip(s("command").lines().next().unwrap_or(""), 90)) } else { clip(d, 90) }
        }
        "Grep" => format!("Ищет «{}»", clip(s("pattern"), 60)),
        "Glob" => format!("Ищет файлы {}", clip(s("pattern"), 60)),
        "WebFetch" => format!("Открывает {}", clip(s("url"), 70)),
        "WebSearch" => format!("Ищет в сети: {}", clip(s("query"), 70)),
        "TodoWrite" => "Обновляет план".into(),
        "Task" | "Agent" => format!("Поручает помощнику: {}", clip(s("description"), 60)),
        "ExitPlanMode" => "Предлагает план".into(),
        n if n.starts_with("senate_") => match n {
            "senate_recall" => format!("Вспоминает: {}", clip(s("query"), 60)),
            "senate_remember" => "Запоминает".into(),
            "senate_correct_memory" => "Исправляет память".into(),
            "senate_obligations" => "Смотрит задача".into(),
            "senate_obligation_history" => format!("Смотрит историю задачи {}", clip(s("id"), 8)),
            "senate_obligation_reassign" => format!("Передаёт задачу {} → {}", clip(s("id"), 8), s("owner")),
            "senate_obligation_create" => format!("Принимает поручение: {}", clip(s("title"), 60)),
            "senate_obligation_update" => format!("Обновляет задачу → {}", s("state")),
            "senate_decision" => format!("Предлагает решение: {}", clip(s("title"), 60)),
            "senate_artifact_save" => format!("Сохраняет артефакт: {}", clip(s("title"), 60)),
            "senate_colleagues" => "Смотрит коллег".into(),
            "senate_image_generate" => format!("Рисует: {}", clip(s("prompt"), 60)),
            "senate_preview" => {
                if s("clear") == "true" || s("url").is_empty() { "Убирает ссылку на сервер".into() } else { format!("Показывает ссылку: {}", clip(s("url"), 60)) }
            }
            _ => n.to_string(),
        },
        // Any MCP server's tool: "сервер · действие" and the one argument that says what it is about,
        // instead of the raw `mcp__server__tool_name`.
        n if n.starts_with("mcp__") => {
            let rest = &n["mcp__".len()..];
            let (server, tool) = rest.split_once("__").unwrap_or(("", rest));
            let about = ["title", "query", "name", "url", "id", "task_id", "path", "command"].iter().find_map(|k| {
                let v = input.get(*k)?;
                let t = v.as_str().map(String::from).or_else(|| v.as_i64().map(|x| x.to_string()))?;
                if t.is_empty() { None } else { Some(format!("{k} {}", clip(&t, 50))) }
            });
            let what = tool.replace('_', " ");
            let head = if server.is_empty() { what } else { format!("{server} · {what}") };
            match about {
                Some(a) => format!("{head} ({a})"),
                None => head,
            }
        }
        other => other.to_string(),
    }
}

pub(crate) fn clip_value(v: &Value, n: usize) -> Value {
    match v {
        Value::String(s) => Value::String(clip(s, n)),
        Value::Object(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), clip_value(v, n))).collect()),
        Value::Array(a) => Value::Array(a.iter().take(60).map(|v| clip_value(v, n)).collect()),
        other => other.clone(),
    }
}

struct State {
    steps: Vec<Step>,
    pending: HashMap<String, (usize, Instant)>,
    partial: String,
    text: String,
    session_id: Option<String>,
    model: String,
    context_tokens: u64,
    work: Work,
    failure: Option<String>,
    thinking_tokens: u64,
    done: bool,
}

impl State {
    fn new(model: &str) -> Self {
        State {
            steps: vec![],
            pending: HashMap::new(),
            partial: String::new(),
            text: String::new(),
            session_id: None,
            model: model.to_string(),
            context_tokens: 0,
            work: Work::default(),
            failure: None,
            thinking_tokens: 0,
            done: false,
        }
    }
}

fn emit(tx: &EventTx, p: Progress) {
    let _ = tx.send(Event::Progress(Box::new(p)));
}

fn snapshot(st: &State) -> Progress {
    Progress { steps: Some(st.steps.clone()), ..Default::default() }
}

fn control_response(request_id: &str, body: Value) -> String {
    format!("{}\n", json!({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": body}}))
}

fn control_error(request_id: &str, msg: &str) -> String {
    format!("{}\n", json!({"type": "control_response", "response": {"subtype": "error", "request_id": request_id, "error": msg}}))
}

/// Claude Code keeps a session as `~/.claude/projects/<folder, every other character a dash>/<id>.jsonl` and loads
/// it only for that folder. A session that is not there for this folder (it was made on another computer, or in
/// another folder) cannot be continued; `None` when it cannot tell (no projects folder at all).
pub(crate) fn session_here(cwd: &std::path::Path, id: &str) -> Option<bool> {
    let home = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(|h| std::path::PathBuf::from(h).join(".claude")))?;
    let projects = home.join("projects");
    if !projects.is_dir() {
        return None;
    }
    let real = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let real = real.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let enc: String = real.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let file = format!("{id}.jsonl");
    if projects.join(&enc).join(&file).exists() {
        return Some(true);
    }
    // long folder names are shortened by the CLI: accept a folder that starts the same way
    let found = std::fs::read_dir(&projects).ok()?.flatten().any(|e| {
        let n = e.file_name().to_string_lossy().to_string();
        enc.len() > 80 && n.len() >= 80 && enc.starts_with(&n[..80]) && e.path().join(&file).exists()
    });
    Some(found)
}

pub async fn run(spec: RunSpec, approver: &dyn Approver, tx: EventTx, cancel: CancelRx) -> Outcome {
    // a session this computer does not have for this folder: start fresh at once instead of failing first
    let spec = match &spec.resume {
        Some(id) if session_here(&spec.cwd, id) == Some(false) => RunSpec { resume: None, ..spec },
        _ => spec,
    };
    let resumed = spec.resume.is_some();
    let first = attempt(&spec, approver, &tx, cancel.clone()).await;
    // A chat whose CLI session was lost (reinstall, cleaned profile) continues as a new session.
    if resumed && !first.stopped {
        if let Some(f) = &first.failure {
            let l = f.to_lowercase();
            if (l.contains("no conversation") || l.contains("session") && (l.contains("not found") || l.contains("no such"))) && first.text.is_empty() {
                let mut again = spec.clone();
                again.resume = None;
                return attempt(&again, approver, &tx, cancel).await;
            }
        }
    }
    first
}

async fn attempt(spec: &RunSpec, approver: &dyn Approver, tx: &EventTx, mut cancel: CancelRx) -> Outcome {
    let Some(exe) = cli::resolve("claude") else {
        return Outcome { failure: Some("Claude Code не найден на этой машине. Установите его: npm i -g @anthropic-ai/claude-code, затем войдите командой `claude`.".into()), ..Default::default() };
    };
    if !spec.cwd.is_dir() {
        return Outcome { failure: Some(format!("Рабочая папка недоступна: {}", spec.cwd.display())), ..Default::default() };
    }
    let _ = std::fs::create_dir_all(&spec.work_dir);
    let sys_file = spec.work_dir.join(format!("system-{}.md", spec.run_id));
    let mcp_file = spec.work_dir.join(format!("mcp-{}.json", spec.run_id));
    let _ = std::fs::write(&sys_file, &spec.system_prompt);
    let mut servers = serde_json::Map::new();
    if let Some(m) = &spec.mcp {
        servers.insert("senate".into(), json!({"type": "stdio", "command": m.command, "args": m.args, "env": m.env.iter().cloned().collect::<HashMap<_, _>>()}));
    }
    for x in &spec.extra_mcp {
        let v = if x.kind == "http" {
            json!({"type": "http", "url": x.url, "headers": x.headers.iter().cloned().collect::<HashMap<_, _>>()})
        } else {
            json!({"type": "stdio", "command": x.command, "args": x.args, "env": x.env.iter().cloned().collect::<HashMap<_, _>>()})
        };
        servers.insert(x.name.clone(), v);
    }
    let mcp_json = json!({"mcpServers": servers});
    let _ = std::fs::write(&mcp_file, mcp_json.to_string());

    let mut args: Vec<String> = vec![
        "-p".into(), "--output-format".into(), "stream-json".into(), "--input-format".into(), "stream-json".into(),
        "--verbose".into(), "--include-partial-messages".into(),
        "--permission-prompt-tool".into(), "stdio".into(),
        "--permission-mode".into(), policy::cli_mode(spec.mode).into(),
        "--tools".into(), if spec.bare { String::new() } else { TOOLS.to_string() },
        "--disallowedTools".into(), "AskUserQuestion".into(),
        "--setting-sources".into(), "project,local".into(),
        "--strict-mcp-config".into(), "--mcp-config".into(), mcp_file.to_string_lossy().into_owned(),
        "--disable-slash-commands".into(),
    ];
    if !spec.system_prompt.is_empty() {
        args.push("--append-system-prompt-file".into());
        args.push(sys_file.to_string_lossy().into_owned());
    }
    if let Some(m) = spec.model() {
        args.push("--model".into());
        args.push(m.into());
    }
    if let Some(e) = spec.effort.as_deref().filter(|e| ["low", "medium", "high"].contains(e)) {
        args.push("--effort".into());
        args.push(e.into());
    }
    if let Some(r) = &spec.resume {
        args.push("--resume".into());
        args.push(r.clone());
    }
    // Folders the person allowed besides the project one: Claude works in them as in its own.
    for d in spec.extra_dirs.iter().filter(|d| d.is_dir()) {
        args.push("--add-dir".into());
        args.push(d.to_string_lossy().into_owned());
    }

    let mut cmd = cli::command(&exe);
    cmd.args(&args)
        .current_dir(&spec.cwd)
        .env_clear()
        .envs(cli::child_env())
        // Senate's tools must be in front of the agent from the first turn, not hidden behind ToolSearch:
        // a specialist that has to "discover" its memory tool does not use it.
        .env("ENABLE_TOOL_SEARCH", "false")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            cleanup(&sys_file, &mcp_file);
            return Outcome { failure: Some(format!("Не удалось запустить Claude Code: {e}")), ..Default::default() };
        }
    };
    let mut stdin: Option<ChildStdin> = child.stdin.take();
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let tail = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let tail2 = tail.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            let mut t = tail2.lock().unwrap();
            t.push_str(&l);
            t.push('\n');
            let n = t.len();
            if n > 4000 {
                *t = t[n - 3000..].to_string();
            }
        }
    });

    emit(tx, Progress { phase: Some("запускается".into()), ..Default::default() });
    let user = json!({"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": spec.prompt}]}});
    if let Some(w) = stdin.as_mut() {
        let _ = w.write_all(format!("{user}\n").as_bytes()).await;
        let _ = w.flush().await;
    }

    let mut st = State::new(spec.model().unwrap_or(""));
    let mut lines = BufReader::new(stdout).lines();
    let started = Instant::now();
    let mut stopped = false;

    loop {
        if started.elapsed() > RUN_LIMIT {
            st.failure = Some("Работа дольше двух часов — остановлена".into());
            break;
        }
        let line = tokio::select! {
            l = lines.next_line() => match l { Ok(Some(l)) => l, _ => break },
            _ = cancel.changed() => { if *cancel.borrow() { stopped = true; break } else { continue } }
        };
        let text = line.trim();
        if !text.starts_with('{') {
            continue;
        }
        let Ok(ev) = serde_json::from_str::<Value>(text) else { continue };
        if let Some(id) = ev.get("session_id").and_then(|v| v.as_str()) {
            if st.session_id.as_deref() != Some(id) {
                st.session_id = Some(id.to_string());
                emit(tx, Progress { session_id: Some(id.to_string()), ..Default::default() });
            }
        }
        match ev.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "control_request" => {
                let reply = handle_control(&ev, spec, approver, tx, &mut st, &mut cancel).await;
                match reply {
                    Some(r) => {
                        if let Some(w) = stdin.as_mut() {
                            let _ = w.write_all(r.as_bytes()).await;
                            let _ = w.flush().await;
                        }
                    }
                    None => {
                        // cancelled while waiting for a human
                        stopped = true;
                        break;
                    }
                }
            }
            _ => handle_event(&ev, tx, &mut st),
        }
        if st.done {
            // Close stdin so the CLI exits instead of waiting for the next user message.
            drop(stdin.take());
        }
    }

    drop(stdin.take());
    if stopped {
        let _ = child.kill().await;
    } else {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
    }
    let _ = child.kill().await;
    cleanup(&sys_file, &mcp_file);

    if !stopped && st.failure.is_none() && st.text.is_empty() && !st.done {
        let t = tail.lock().unwrap().trim().to_string();
        st.failure = Some(if t.is_empty() { "Claude Code завершился без ответа".into() } else { clip(&t, 600) });
    }
    Outcome { text: st.text, session_id: st.session_id, work: st.work, failure: st.failure, stopped, model: st.model, steps: st.steps }
}

fn cleanup(a: &std::path::Path, b: &std::path::Path) {
    let _ = std::fs::remove_file(a);
    let _ = std::fs::remove_file(b);
}

/// Answer a control request. `None` = run was cancelled while waiting for a human.
async fn handle_control(ev: &Value, spec: &RunSpec, approver: &dyn Approver, tx: &EventTx, st: &mut State, cancel: &mut CancelRx) -> Option<String> {
    let request_id = ev.get("request_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let req = ev.get("request").cloned().unwrap_or(Value::Null);
    if req.get("subtype").and_then(|v| v.as_str()) != Some("can_use_tool") {
        return Some(control_error(&request_id, "unsupported request"));
    }
    let tool = req.get("tool_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let input = req.get("input").cloned().unwrap_or(Value::Null);
    let tool_use_id = req.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or(&request_id).to_string();

    let allow = |input: &Value| control_response(&request_id, json!({"behavior": "allow", "updatedInput": input}));
    let deny = |msg: &str| control_response(&request_id, json!({"behavior": "deny", "message": msg}));

    match policy::judge(spec.mode, &tool, &input, &spec.cwd, &spec.trusted_mcp()) {
        Verdict::Allow => Some(allow(&input)),
        Verdict::Deny(msg) => {
            mark(st, &tool_use_id, "denied");
            emit(tx, snapshot(st));
            Some(deny(&msg))
        }
        Verdict::Ask(reason) => {
            let req = PermissionRequest { tool_use_id: tool_use_id.clone(), tool: display_name(&tool), input: clip_value(&input, 4000), reason };
            if approver.preapproved(&req) {
                return Some(allow(&input));
            }
            mark(st, &tool_use_id, "awaiting");
            emit(tx, Progress { phase: Some("ждёт разрешения".into()), steps: Some(st.steps.clone()), ..Default::default() });
            let _ = tx.send(Event::Ask(req.clone()));
            let decision = tokio::select! {
                d = approver.ask(req) => d,
                _ = async { loop { if cancel.changed().await.is_err() || *cancel.borrow() { break } } } => return None,
            };
            let allowed = matches!(decision, Decision::Allow);
            let _ = tx.send(Event::Answered { tool_use_id: tool_use_id.clone(), allowed });
            mark(st, &tool_use_id, if allowed { "running" } else { "denied" });
            emit(tx, Progress { phase: Some("работает".into()), steps: Some(st.steps.clone()), ..Default::default() });
            Some(match decision {
                Decision::Allow => allow(&input),
                Decision::Deny(m) => deny(if m.is_empty() { "Пользователь отклонил это действие." } else { &m }),
            })
        }
    }
}

fn mark(st: &mut State, tool_use_id: &str, status: &str) {
    if let Some((i, _)) = st.pending.get(tool_use_id) {
        if let Some(s) = st.steps.get_mut(*i) {
            s.status = status.to_string();
        }
    }
}

fn handle_event(ev: &Value, tx: &EventTx, st: &mut State) {
    let ty = ev.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let sub = ev.get("subtype").and_then(|v| v.as_str()).unwrap_or("");
    match (ty, sub) {
        ("system", "init") => {
            if let Some(m) = ev.get("model").and_then(|v| v.as_str()) {
                st.model = m.to_string();
            }
            emit(tx, Progress { phase: Some("думает".into()), model: Some(st.model.clone()), ..Default::default() });
        }
        ("system", "thinking_tokens") => {
            st.thinking_tokens = ev.get("estimated_tokens").and_then(|v| v.as_u64()).unwrap_or(st.thinking_tokens);
            emit(tx, Progress { phase: Some("думает".into()), ..Default::default() });
        }
        ("rate_limit_event", _) => {
            if let Some(info) = ev.get("rate_limit_info") {
                let used = info.get("utilization").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let status = info.get("status").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(w) = info.get("unifiedWindows") {
                    emit(tx, Progress { limits: Some(w.clone()), ..Default::default() });
                }
                // Show a banner only when it matters: nearly spent, or refused.
                if used >= 0.8 || status.contains("reject") || status.contains("block") {
                    emit(tx, Progress { rate_limit: Some(info.clone()), ..Default::default() });
                }
            }
        }
        ("stream_event", _) => {
            let e = ev.get("event").cloned().unwrap_or(Value::Null);
            match e.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                "content_block_start" => st.partial.clear(),
                "content_block_delta" => {
                    let d = e.get("delta").cloned().unwrap_or(Value::Null);
                    match d.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                        "text_delta" => {
                            st.partial.push_str(d.get("text").and_then(|v| v.as_str()).unwrap_or(""));
                            emit(tx, Progress { body: Some(st.partial.clone()), phase: Some("пишет".into()), ..Default::default() });
                        }
                        "thinking_delta" => emit(tx, Progress { phase: Some("думает".into()), ..Default::default() }),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        ("assistant", _) => {
            let msg = ev.get("message").cloned().unwrap_or(Value::Null);
            if let Some(u) = msg.get("usage") {
                let g = |k: &str| u.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
                st.context_tokens = g("input_tokens") + g("cache_read_input_tokens") + g("cache_creation_input_tokens");
            }
            let mut changed = false;
            for block in msg.get("content").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
                match block.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                    "text" => {
                        let t = block.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                        if !t.is_empty() && st.steps.len() < MAX_STEPS {
                            st.text = t.clone();
                            st.steps.push(Step { kind: "text".into(), text: clip(&t, 4000), ..Default::default() });
                            changed = true;
                        }
                    }
                    "tool_use" => {
                        if st.steps.len() >= MAX_STEPS {
                            continue;
                        }
                        let raw_name = block.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                        let name = display_name(raw_name);
                        let input = block.get("input").cloned().unwrap_or(Value::Null);
                        let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        st.steps.push(Step {
                            kind: "tool".into(),
                            id: id.clone(),
                            hint: hint_of(&name, &input),
                            name,
                            input: clip_value(&input, MAX_STEP_INPUT),
                            status: "running".into(),
                            ..Default::default()
                        });
                        st.pending.insert(id, (st.steps.len() - 1, Instant::now()));
                        st.partial.clear();
                        changed = true;
                        emit(tx, Progress { body: Some(String::new()), phase: Some("работает".into()), ..Default::default() });
                    }
                    _ => {}
                }
            }
            if changed {
                emit(tx, snapshot(st));
            }
        }
        ("user", _) => {
            let msg = ev.get("message").cloned().unwrap_or(Value::Null);
            let structured = ev.get("tool_use_result").cloned().unwrap_or(Value::Null);
            let mut changed = false;
            for block in msg.get("content").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
                if block.get("type").and_then(|v| v.as_str()) != Some("tool_result") {
                    continue;
                }
                let id = block.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
                let Some((idx, at)) = st.pending.remove(id) else { continue };
                let content = match block.get("content") {
                    Some(Value::Array(parts)) => parts.iter().filter_map(|p| p.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("\n"),
                    Some(Value::String(s)) => s.clone(),
                    _ => String::new(),
                };
                if let Some(step) = st.steps.get_mut(idx) {
                    let failed = block.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                    step.output = if crate::common::hides_result(&step.name) && !failed { crate::common::SECRET_HIDDEN.into() } else { clip(&content, MAX_STEP_OUTPUT) };
                    step.is_error = failed;
                    step.ms = Some(at.elapsed().as_millis() as u64);
                    if step.status != "denied" {
                        step.status = "done".into();
                    }
                    if let Some(p) = structured.get("structuredPatch") {
                        step.patch = clip_value(p, 4000);
                    }
                }
                changed = true;
            }
            if changed {
                emit(tx, Progress { phase: Some("думает".into()), steps: Some(st.steps.clone()), ..Default::default() });
            }
        }
        ("result", _) => {
            st.done = true;
            let r = ev.get("result").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            if !r.is_empty() {
                st.text = r;
            }
            let u = ev.get("usage").cloned().unwrap_or(Value::Null);
            let g = |k: &str| u.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
            let window = ev
                .get("modelUsage")
                .and_then(|m| m.as_object())
                .map(|m| m.values().filter_map(|v| v.get("contextWindow").and_then(|w| w.as_u64())).max().unwrap_or(0))
                .unwrap_or(0);
            st.work = Work {
                input_tokens: g("input_tokens") + g("cache_read_input_tokens") + g("cache_creation_input_tokens"),
                cached_input_tokens: g("cache_read_input_tokens"),
                output_tokens: g("output_tokens"),
                thinking_tokens: u.get("output_tokens_details").and_then(|d| d.get("thinking_tokens")).and_then(|v| v.as_u64()).unwrap_or(st.thinking_tokens),
                context_tokens: st.context_tokens,
                context_window: window,
                duration_ms: ev.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0),
                turns: ev.get("num_turns").and_then(|v| v.as_u64()).unwrap_or(0),
            };
            if ev.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false) {
                st.failure = Some(if st.text.is_empty() { sub.to_string() } else { st.text.clone() });
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_is_continued_only_where_claude_keeps_it() {
        let root = std::env::temp_dir().join(format!("senate-claude-{}", std::process::id()));
        let cwd = root.join("work").join("crm");
        std::fs::create_dir_all(&cwd).unwrap();
        let cfg = root.join("cfg");
        std::fs::create_dir_all(cfg.join("projects")).unwrap();
        std::env::set_var("CLAUDE_CONFIG_DIR", &cfg);
        // a session from another computer: not here
        assert_eq!(session_here(&cwd, "1f39af27"), Some(false));
        // the CLI's own folder for this cwd holds it: here
        let real = std::fs::canonicalize(&cwd).unwrap().to_string_lossy().trim_start_matches(r"\\?\").to_string();
        let enc: String = real.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        std::fs::create_dir_all(cfg.join("projects").join(&enc)).unwrap();
        std::fs::write(cfg.join("projects").join(&enc).join("1f39af27.jsonl"), "{}").unwrap();
        assert_eq!(session_here(&cwd, "1f39af27"), Some(true));
        std::env::remove_var("CLAUDE_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&root);

        // and a resume that dies before doing anything is retried fresh
        let died = crate::Outcome { failure: Some("error_during_execution".into()), ..Default::default() };
        assert!(crate::resume_died_at_once(&died));
        let worked = crate::Outcome { failure: Some("error_during_execution".into()), text: "часть ответа".into(), ..Default::default() };
        assert!(!crate::resume_died_at_once(&worked));
    }
}
