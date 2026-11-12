//! Codex CLI backend: `codex exec --json`. Codex has no per-action approval in headless mode, so the
//! permission modes map onto its sandbox: plan/ask → read-only, edit/auto → workspace-write.
//! (`approval_policy=never`: Senate, not the CLI, owns what the specialist may do.)

use crate::common::{human_error, limit_from_text, pump, spawn, toml_str, write_stdin, Tracker};
use crate::{clip, cli, Approver, CancelRx, EventTx, Outcome, PermissionMode, Progress, RunSpec, Work};
use serde_json::{json, Value};
use std::time::Duration;

const RUN_LIMIT: Duration = Duration::from_secs(2 * 60 * 60);

fn sandbox(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Plan | PermissionMode::Ask => "read-only",
        PermissionMode::Edit | PermissionMode::Auto => "workspace-write",
    }
}

fn hint(name: &str, input: &Value) -> String {
    let s = |k: &str| input.get(k).and_then(|v| v.as_str()).unwrap_or("");
    match name {
        "shell" => format!("Команда: {}", clip(s("command").lines().next().unwrap_or(""), 90)),
        "web_search" => format!("Ищет в сети: {}", clip(s("query"), 70)),
        "apply_patch" => format!("Правит {}", clip(s("paths"), 70)),
        n if n.starts_with("senate_") => crate::claude::hint_of(n, input),
        other => other.to_string(),
    }
}

fn mcp_flags(spec: &RunSpec) -> Vec<String> {
    let mut f: Vec<String> = vec![];
    let mut add = |k: String, v: String| {
        f.push("-c".into());
        f.push(format!("{k}={v}"));
    };
    if let Some(m) = &spec.mcp {
        add("mcp_servers.senate.command".into(), toml_str(&m.command));
        add("mcp_servers.senate.args".into(), json!(m.args).to_string());
        for (k, v) in &m.env {
            add(format!("mcp_servers.senate.env.{k}"), toml_str(v));
        }
        add("mcp_servers.senate.tool_timeout_sec".into(), "120".into());
        add("mcp_servers.senate.default_tools_approval_mode".into(), toml_str("approve"));
    }
    for x in &spec.extra_mcp {
        let n = &x.name;
        if x.kind == "http" {
            add(format!("mcp_servers.{n}.url"), toml_str(&x.url));
            if !x.headers.is_empty() {
                let tbl = x.headers.iter().map(|(k, v)| format!("{} = {}", toml_str(k), toml_str(v))).collect::<Vec<_>>().join(", ");
                add(format!("mcp_servers.{n}.http_headers"), format!("{{ {tbl} }}"));
            }
        } else {
            add(format!("mcp_servers.{n}.command"), toml_str(&x.command));
            add(format!("mcp_servers.{n}.args"), json!(x.args).to_string());
            for (k, v) in &x.env {
                add(format!("mcp_servers.{n}.env.{k}"), toml_str(v));
            }
        }
        // Tool approval is the user's decision per server (Trusted), made in Senate.
        add(format!("mcp_servers.{n}.default_tools_approval_mode"), toml_str("approve"));
        add(format!("mcp_servers.{n}.tool_timeout_sec"), "120".into());
    }
    f
}

pub async fn run(spec: RunSpec, _approver: &dyn Approver, tx: EventTx, mut cancel: CancelRx) -> Outcome {
    let Some(exe) = cli::resolve("codex") else {
        return Outcome { failure: Some("Codex не найден на этой машине. Установите: npm i -g @openai/codex, затем войдите в Настройках.".into()), ..Default::default() };
    };
    if !spec.cwd.is_dir() {
        return Outcome { failure: Some(format!("Рабочая папка недоступна: {}", spec.cwd.display())), ..Default::default() };
    }
    let resumed = spec.resume.is_some();
    let mut args: Vec<String> = vec!["exec".into()];
    if let Some(r) = &spec.resume {
        args.push("resume".into());
        args.push("--json".into());
        let _ = r;
    } else {
        args.push("--json".into());
    }
    args.push("--skip-git-repo-check".into());
    for (k, v) in [
        ("approval_policy", "never".to_string()),
        ("sandbox_mode", toml_str(sandbox(spec.mode))),
        ("model_reasoning_summary", "auto".into()),
        ("hide_agent_reasoning", "false".into()),
        ("shell_environment_policy.inherit", toml_str("core")),
    ] {
        args.push("-c".into());
        args.push(format!("{k}={v}"));
    }
    args.extend(mcp_flags(&spec));
    if let Some(m) = spec.model() {
        args.push("-m".into());
        args.push(m.into());
    }
    if let Some(e) = spec.effort.as_deref().filter(|e| ["low", "medium", "high"].contains(e)) {
        args.push("-c".into());
        args.push(format!("model_reasoning_effort={}", toml_str(e)));
    }
    if let Some(r) = &spec.resume {
        args.push(r.clone());
    } else {
        args.push("-C".into());
        args.push(spec.cwd.to_string_lossy().into_owned());
    }
    args.push("-".into());

    let mut cmd = cli::command(&exe);
    cmd.args(&args).current_dir(&spec.cwd).env_clear().envs(cli::child_env()).envs(spec.env.iter().cloned());
    let mut proc = match spawn(cmd) {
        Ok(p) => p,
        Err(e) => return Outcome { failure: Some(format!("Не удалось запустить Codex: {e}")), ..Default::default() },
    };
    // Codex exec has no separate system prompt: the role goes first in a new session's message.
    let prompt = if resumed || spec.system_prompt.is_empty() { spec.prompt.clone() } else { format!("{}\n\n---\n\n{}", spec.system_prompt, spec.prompt) };
    write_stdin(&mut proc, &prompt).await;

    let mut t = Tracker::new(tx, spec.model().unwrap_or(""));
    t.phase("запускается");
    let started = std::time::Instant::now();
    let stopped = match pump(&mut proc, &mut cancel, RUN_LIMIT, |line| handle(line.trim(), &mut t)).await {
        Ok(s) => s,
        Err(e) => {
            t.failure = Some(e);
            false
        }
    };
    let _ = proc.child.kill().await;
    if !stopped && t.failure.is_none() && t.text.is_empty() {
        let tail = proc.tail.lock().unwrap().clone();
        t.failure = Some(match limit_from_text(&tail) {
            Some(l) => format!("Исчерпан лимит подписки: {l}"),
            None if tail.trim().is_empty() => "Codex завершился без ответа".into(),
            None => clip(&human_error(tail.trim()), 600),
        });
    }
    if t.work.duration_ms == 0 {
        t.work.duration_ms = started.elapsed().as_millis() as u64;
    }
    t.finish(stopped)
}

fn handle(line: &str, t: &mut Tracker) {
    if !line.starts_with('{') {
        return;
    }
    let Ok(mut ev) = serde_json::from_str::<Value>(line) else { return };
    if ev.get("type").is_none() {
        if let Some(m) = ev.get("msg").cloned() {
            ev = m;
        }
    }
    let ty = ev["type"].as_str().unwrap_or("").to_string();
    match ty.as_str() {
        "thread.started" => {
            if let Some(id) = ev["thread_id"].as_str() {
                t.session_id = Some(id.to_string());
                t.send(Progress { session_id: Some(id.to_string()), ..Default::default() });
            }
        }
        "turn.started" => t.phase("думает"),
        "turn.completed" => {
            let u = &ev["usage"];
            let g = |k: &str| u[k].as_u64().unwrap_or(0);
            t.work = Work {
                input_tokens: g("input_tokens"),
                cached_input_tokens: g("cached_input_tokens"),
                output_tokens: g("output_tokens"),
                thinking_tokens: g("reasoning_output_tokens"),
                context_tokens: g("input_tokens"),
                turns: 1,
                ..t.work.clone()
            };
            t.done = true;
        }
        "error" | "turn.failed" => {
            let m = ev["message"].as_str().or(ev["error"]["message"].as_str()).or(ev["error"].as_str()).unwrap_or("Codex вернул ошибку");
            t.failure = Some(human_error(m));
        }
        "item.started" | "item.completed" => item(&ev["item"], ty == "item.started", t),
        _ => {}
    }
}

fn item(it: &Value, started: bool, t: &mut Tracker) {
    let kind = it["type"].as_str().unwrap_or("");
    let id = it["id"].as_str().or(it["call_id"].as_str()).unwrap_or("").to_string();
    match kind {
        "agent_message" if !started => {
            let text = it["text"].as_str().unwrap_or("").trim().to_string();
            if !text.is_empty() {
                t.partial = text.clone();
                t.text = text;
                t.stream_text(&t.partial.clone(), true);
            }
        }
        "reasoning" if !started => {
            let text = it["text"].as_str().or(it["summary"].as_str()).unwrap_or("").replace("**", "");
            if !text.trim().is_empty() {
                t.partial = text;
                t.flush_text_to_step();
            }
            t.phase("думает");
        }
        "command_execution" => {
            if started {
                t.flush_text_to_step();
                let input = json!({"command": it["command"]});
                t.tool_start(&id, "shell", hint("shell", &input), input);
            } else {
                if !t.has_tool(&id) {
                    let input = json!({"command": it["command"]});
                    t.tool_start(&id, "shell", hint("shell", &input), input);
                }
                let failed = it["exit_code"].as_i64().map(|c| c != 0).unwrap_or(false) || it["status"] == "failed";
                t.tool_end(&id, it["aggregated_output"].as_str().unwrap_or(""), failed, None);
            }
        }
        "file_change" => {
            let paths: Vec<String> = it["changes"].as_array().map(|a| a.iter().filter_map(|c| c["path"].as_str().map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p).to_string())).collect()).unwrap_or_default();
            let input = json!({"changes": it["changes"], "paths": paths.join(", ")});
            if started {
                t.flush_text_to_step();
                t.tool_start(&id, "Edit", hint("apply_patch", &input), input);
            } else {
                if !t.has_tool(&id) {
                    t.tool_start(&id, "Edit", hint("apply_patch", &input), input);
                }
                let list = it["changes"].as_array().map(|a| a.iter().map(|c| format!("{} {}", c["kind"].as_str().unwrap_or("изменён"), c["path"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("\n")).unwrap_or_default();
                t.tool_end(&id, &list, it["status"] == "failed", None);
            }
        }
        "mcp_tool_call" | "web_search" => {
            let name = if kind == "web_search" { "web_search".to_string() } else { it["tool"].as_str().unwrap_or("mcp").to_string() };
            let server = it["server"].as_str().unwrap_or("");
            let shown = if server.is_empty() || server == "senate" { name.clone() } else { format!("{server}:{name}") };
            let input = it["arguments"].clone().as_object().map(|_| it["arguments"].clone()).unwrap_or_else(|| json!({"query": it["query"]}));
            if started {
                t.flush_text_to_step();
                t.tool_start(&id, &shown, hint(&name, &input), input);
            } else {
                if !t.has_tool(&id) {
                    t.tool_start(&id, &shown, hint(&name, &input), input);
                }
                let out = match &it["result"]["content"] {
                    Value::Array(a) => a.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
                    _ => it["error"].as_str().map(String::from).unwrap_or_else(|| it["status"].as_str().unwrap_or("").to_string()),
                };
                t.tool_end(&id, &out, it["error"].is_string() || it["status"] == "failed", None);
            }
        }
        _ => {}
    }
}
