//! Shared bookkeeping for backends that stream events: the step chain, streamed text, usage.

use crate::{clip, Event, EventTx, Outcome, Progress, Step, Work};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Instant;

pub(crate) const MAX_STEP_INPUT: usize = 20_000;
pub(crate) const MAX_STEP_OUTPUT: usize = 6_000;

/// A tool whose result is a secret the person handed over for one job (`senate_access_get`). It goes to the
/// specialist and nowhere else: not into the chat's history, not into the database.
pub(crate) fn hides_result(tool_name: &str) -> bool {
    tool_name.ends_with("senate_access_get")
}
pub(crate) const SECRET_HIDDEN: &str = "Доступ выдан. Значения скрыты: в историю чата они не попадают.";
pub(crate) const MAX_STEPS: usize = 600;

pub(crate) struct Tracker {
    pub steps: Vec<Step>,
    pending: HashMap<String, (usize, Instant)>,
    pub text: String,
    pub partial: String,
    pub session_id: Option<String>,
    pub model: String,
    pub work: Work,
    pub failure: Option<String>,
    pub done: bool,
    tx: EventTx,
}

impl Tracker {
    pub fn new(tx: EventTx, model: &str) -> Self {
        Tracker { steps: vec![], pending: HashMap::new(), text: String::new(), partial: String::new(), session_id: None, model: model.into(), work: Work::default(), failure: None, done: false, tx }
    }

    pub fn send(&self, p: Progress) {
        let _ = self.tx.send(Event::Progress(Box::new(p)));
    }
    pub fn phase(&self, ph: &str) {
        self.send(Progress { phase: Some(ph.into()), ..Default::default() });
    }
    fn snapshot(&self) {
        self.send(Progress { steps: Some(self.steps.clone()), ..Default::default() });
    }

    /// Streamed assistant text: shown live as the message body.
    pub fn stream_text(&mut self, delta: &str, replace: bool) {
        if replace {
            self.partial = delta.to_string();
        } else {
            self.partial.push_str(delta);
        }
        self.text = self.partial.trim().to_string();
        self.send(Progress { body: Some(self.partial.clone()), phase: Some("пишет".into()), ..Default::default() });
    }

    /// Text before a tool call becomes a remark in the chain; the body is cleared.
    pub fn flush_text_to_step(&mut self) {
        let t = self.partial.trim().to_string();
        if !t.is_empty() && self.steps.len() < MAX_STEPS {
            self.steps.push(Step { kind: "text".into(), text: clip(&t, 4000), ..Default::default() });
        }
        self.partial.clear();
        self.send(Progress { body: Some(String::new()), steps: Some(self.steps.clone()), ..Default::default() });
    }

    pub fn tool_start(&mut self, id: &str, name: &str, hint: String, input: Value) {
        if self.steps.len() >= MAX_STEPS {
            return;
        }
        self.steps.push(Step {
            kind: "tool".into(),
            id: id.into(),
            name: name.into(),
            hint,
            input: crate::claude::clip_value(&input, MAX_STEP_INPUT),
            status: "running".into(),
            ..Default::default()
        });
        self.pending.insert(id.to_string(), (self.steps.len() - 1, Instant::now()));
        self.send(Progress { phase: Some("работает".into()), steps: Some(self.steps.clone()), ..Default::default() });
    }

    pub fn tool_input(&mut self, id: &str, hint: String, input: Value) {
        if let Some((i, _)) = self.pending.get(id) {
            if let Some(s) = self.steps.get_mut(*i) {
                s.input = crate::claude::clip_value(&input, MAX_STEP_INPUT);
                if !hint.is_empty() {
                    s.hint = hint;
                }
            }
        }
    }

    /// Some CLIs (Kimi) announce a call before they know what it is: the first event has no name.
    /// When the real name arrives, the step takes it and a readable line; a name already known is kept.
    pub fn tool_identify(&mut self, id: &str, name: &str, hint: String) {
        let known = |n: &str| !n.is_empty() && n != "tool";
        if !known(name) {
            return;
        }
        if let Some((i, _)) = self.pending.get(id) {
            if let Some(s) = self.steps.get_mut(*i) {
                if !known(&s.name) {
                    s.name = name.into();
                }
                if !hint.is_empty() && (s.hint.is_empty() || s.hint == "tool" || s.hint == s.name) {
                    s.hint = hint;
                }
            }
        }
    }

    pub fn tool_status(&mut self, id: &str, status: &str) {
        if let Some((i, _)) = self.pending.get(id) {
            if let Some(s) = self.steps.get_mut(*i) {
                s.status = status.into();
            }
        }
        self.snapshot();
    }

    pub fn tool_end(&mut self, id: &str, output: &str, is_error: bool, patch: Option<Value>) {
        if let Some((i, at)) = self.pending.remove(id) {
            if let Some(s) = self.steps.get_mut(i) {
                s.output = if hides_result(&s.name) && !is_error { SECRET_HIDDEN.into() } else { clip(output, MAX_STEP_OUTPUT) };
                s.is_error = is_error;
                s.ms = Some(at.elapsed().as_millis() as u64);
                if s.status != "denied" {
                    s.status = "done".into();
                }
                if let Some(p) = patch {
                    s.patch = p;
                }
            }
        }
        self.send(Progress { phase: Some("думает".into()), steps: Some(self.steps.clone()), ..Default::default() });
    }

    pub fn has_tool(&self, id: &str) -> bool {
        self.pending.contains_key(id)
    }

    pub fn finish(mut self, stopped: bool) -> Outcome {
        // Anything still "running" when we stop is closed so the chain does not spin forever.
        for s in self.steps.iter_mut() {
            if s.kind == "tool" && s.status == "running" {
                s.status = if stopped { "denied".into() } else { "done".into() };
            }
        }
        Outcome { text: self.text, session_id: self.session_id, work: self.work, failure: self.failure, stopped, model: self.model, steps: self.steps }
    }
}

/// Human text from provider errors that arrive as JSON.
pub(crate) fn human_error(text: &str) -> String {
    let raw = text.trim();
    if !raw.starts_with('{') {
        return raw.to_string();
    }
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().or(v["message"].as_str()).or(v["error"].as_str()).map(String::from))
        .unwrap_or_else(|| raw.to_string())
}

pub(crate) fn limit_from_text(text: &str) -> Option<String> {
    text.lines()
        .find(|l| {
            let l = l.to_lowercase();
            ["usage limit", "session limit", "rate limit", "limit reached", "quota", "resource_exhausted", "resource exhausted", "429"].iter().any(|k| l.contains(k))
        })
        .map(|l| clip(l.trim(), 300))
}

pub(crate) fn auth_problem(text: &str) -> bool {
    let l = text.to_lowercase();
    ["not logged in", "please login", "please log in", "invalid api key", "authentication", "unauthorized", "unauthorised", "oauth token has expired", "re-authenticate", "reauthenticate"].iter().any(|k| l.contains(k))
}

// ---------- process helpers for line-oriented backends ----------

use crate::CancelRx;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::mpsc;

pub(crate) struct Proc {
    pub child: tokio::process::Child,
    pub lines: mpsc::UnboundedReceiver<String>,
    pub tail: Arc<Mutex<String>>,
    pub stdin: Option<ChildStdin>,
}

/// Its own process group on Unix, so a stop can take down everything the agent started (see `kill_tree`).
pub(crate) fn own_group(cmd: &mut Command) {
    #[cfg(unix)]
    cmd.process_group(0);
    #[cfg(not(unix))]
    let _ = cmd;
}

/// Stop the agent and everything it started: a dev server run in the foreground (`npm run tauri dev`) is a child of
/// the CLI's shell and would outlive a kill of the CLI alone, holding the turn open.
pub(crate) async fn kill_tree(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("taskkill").args(["/T", "/F", "/PID", &pid.to_string()]).creation_flags(0x0800_0000).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
        }
        #[cfg(unix)]
        {
            let _ = std::process::Command::new("kill").args(["-KILL", &format!("-{pid}")]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
        }
    }
    let _ = child.kill().await;
}

pub(crate) fn spawn(mut cmd: Command) -> std::io::Result<Proc> {
    own_group(&mut cmd);
    cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            if tx.send(l).is_err() {
                break;
            }
        }
    });
    let tail = Arc::new(Mutex::new(String::new()));
    let t2 = tail.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            let mut t = t2.lock().unwrap();
            t.push_str(&l);
            t.push('\n');
            if t.len() > 4000 {
                let cut = t.len() - 3000;
                let idx = t.char_indices().map(|(i, _)| i).find(|i| *i >= cut).unwrap_or(0);
                *t = t[idx..].to_string();
            }
        }
    });
    Ok(Proc { child, lines: rx, tail, stdin })
}

/// Feed lines to `on_line` until the process ends, the run is cancelled or the limit passes.
/// Returns true if cancelled.
pub(crate) async fn pump(p: &mut Proc, cancel: &mut CancelRx, limit: std::time::Duration, mut on_line: impl FnMut(&str)) -> Result<bool, String> {
    let deadline = tokio::time::sleep(limit);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            l = p.lines.recv() => match l { Some(l) => on_line(&l), None => return Ok(false) },
            _ = cancel.changed() => { if *cancel.borrow() { return Ok(true) } },
            _ = &mut deadline => return Err("Агент работал дольше двух часов — остановлен".into()),
            // The CLI ended, but something it started (a dev server) may still hold its output open: read what is
            // already there and finish, instead of waiting for a server that will never close it.
            _ = p.child.wait() => {
                let quiet = tokio::time::sleep(std::time::Duration::from_millis(500));
                tokio::pin!(quiet);
                loop {
                    tokio::select! {
                        l = p.lines.recv() => match l { Some(l) => on_line(&l), None => return Ok(false) },
                        _ = &mut quiet => return Ok(false),
                    }
                }
            }
        }
    }
}

pub(crate) async fn write_stdin(p: &mut Proc, text: &str) {
    use tokio::io::AsyncWriteExt;
    if let Some(w) = p.stdin.as_mut() {
        let _ = w.write_all(text.as_bytes()).await;
        let _ = w.flush().await;
    }
    // Close stdin so CLIs that read the whole prompt from it can start.
    p.stdin = None;
}

pub(crate) fn toml_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}
