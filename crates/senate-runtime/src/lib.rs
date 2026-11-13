//! Execution of agent sessions. A backend (Claude Code CLI, Codex CLI, a mock) turns a prompt into a
//! stream of progress; the environment (senate-core) stays the source of truth. Backends are
//! replaceable: nothing here knows about obligations, memory or projects.

pub mod auth;
pub mod claude;
pub mod cli;
pub mod codex;
pub mod common;
pub mod kimi;
pub mod mock;
pub mod policy;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use tokio::sync::{mpsc, watch};

pub use senate_core::model::PermissionMode;

#[derive(Debug, Clone, Default)]
pub struct McpSpec {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// An MCP server given to a specialist (the user-managed ones; Senate's own bridge is `McpSpec`).
#[derive(Debug, Clone, Default)]
pub struct ExtraMcp {
    pub name: String,
    /// "stdio" | "http"
    pub kind: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Its tools run without asking.
    pub trusted: bool,
}

impl From<&senate_core::model::McpServer> for ExtraMcp {
    fn from(m: &senate_core::model::McpServer) -> Self {
        ExtraMcp {
            name: m.name.clone(),
            kind: m.kind.clone(),
            command: m.command.clone(),
            args: m.args.clone(),
            env: m.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            url: m.url.clone(),
            headers: m.headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            trusted: m.trusted,
        }
    }
}

#[derive(Clone)]
pub struct RunSpec {
    /// "claude", "claude:opus", "codex", "mock"
    pub backend: String,
    pub effort: Option<String>,
    pub mode: PermissionMode,
    pub cwd: PathBuf,
    pub system_prompt: String,
    pub prompt: String,
    /// Backend-native session id to continue the conversation.
    pub resume: Option<String>,
    /// How the agent reaches Senate's own tools (memory, obligations, decisions).
    pub mcp: Option<McpSpec>,
    /// MCP servers assigned to this specialist.
    pub extra_mcp: Vec<ExtraMcp>,
    /// Folders the person allowed work in, besides `cwd` ("Разрешить работу в этой директории"). A CLI with its
    /// own sandbox (Codex) or folder list (Claude) is told about them, so work there is not refused or re-asked.
    pub extra_dirs: Vec<PathBuf>,
    /// One-shot helper run (reflection, probes): no tools at all, nothing to approve.
    pub bare: bool,
    /// Extra environment for the CLI (reserved for provider settings); never logged.
    pub env: Vec<(String, String)>,
    /// Scratch directory for per-run files (system prompt, MCP config).
    pub work_dir: PathBuf,
    pub run_id: String,
}

impl std::fmt::Debug for RunSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RunSpec({} in {})", self.backend, self.cwd.display())
    }
}

impl RunSpec {
    /// Names of MCP servers whose tools may run without asking.
    pub fn trusted_mcp(&self) -> Vec<String> {
        self.extra_mcp.iter().filter(|m| m.trusted).map(|m| m.name.clone()).collect()
    }
    pub fn provider(&self) -> &str {
        self.backend.split(':').next().unwrap_or("")
    }
    pub fn model(&self) -> Option<&str> {
        self.backend.split_once(':').map(|(_, m)| m).filter(|m| !m.is_empty())
    }
}

/// One entry of the work chain: a tool call or an intermediate remark.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Step {
    pub kind: String, // "tool" | "text"
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub id: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub hint: String,
    #[serde(skip_serializing_if = "Value::is_null", default)]
    pub input: Value,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub output: String,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ms: Option<u64>,
    /// running | done | denied
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub status: String,
    /// Structured diff for file edits, when the backend provides one.
    #[serde(skip_serializing_if = "Value::is_null", default)]
    pub patch: Value,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Work {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub thinking_tokens: u64,
    pub context_tokens: u64,
    pub context_window: u64,
    pub duration_ms: u64,
    pub turns: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub phase: Option<String>,
    /// Text currently being written (replaces the body while streaming).
    pub body: Option<String>,
    pub steps: Option<Vec<Step>>,
    pub model: Option<String>,
    pub session_id: Option<String>,
    pub rate_limit: Option<Value>,
    /// Subscription windows reported by the provider (five-hour, weekly).
    pub limits: Option<Value>,
    pub reasoning: Option<String>,
}

#[derive(Debug)]
pub enum Event {
    Progress(Box<Progress>),
    /// A permission question is waiting for the user (shown as a card in the chat).
    Ask(PermissionRequest),
    /// The question was answered; the card turns into a note.
    Answered { tool_use_id: String, allowed: bool },
}

#[derive(Debug, Clone, Serialize)]
pub struct PermissionRequest {
    pub tool_use_id: String,
    pub tool: String,
    pub input: Value,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub enum Decision {
    Allow,
    Deny(String),
}

pub trait Approver: Send + Sync {
    fn ask(&self, req: PermissionRequest) -> Pin<Box<dyn Future<Output = Decision> + Send + '_>>;
    /// The person already said "always" to this kind of action: no question, no card, no waiting.
    fn preapproved(&self, _req: &PermissionRequest) -> bool {
        false
    }
}

#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub text: String,
    pub session_id: Option<String>,
    pub work: Work,
    pub failure: Option<String>,
    pub stopped: bool,
    pub model: String,
    pub steps: Vec<Step>,
}

pub type EventTx = mpsc::UnboundedSender<Event>;
pub type CancelRx = watch::Receiver<bool>;

/// The CLI refused to resume because its session is still held by another process: a run that has not let go
/// yet, or one orphaned when the app was killed.
fn session_is_held(o: &Outcome) -> bool {
    o.failure.as_deref().map(|f| f.contains("active writer")).unwrap_or(false)
}

/// The CLI no longer knows the session it was asked to resume: it was cleaned up, the CLI was reinstalled, or the
/// node moved to another machine. Each CLI words it its own way.
pub fn session_is_lost(o: &Outcome) -> bool {
    let Some(f) = o.failure.as_deref() else { return false };
    let l = f.to_lowercase();
    o.text.is_empty()
        && !o.stopped
        && (l.contains("no conversation found")
            || l.contains("unknown sessionid")
            || l.contains("unknown session")
            || l.contains("session not found")
            || l.contains("no rollout found")
            || (l.contains("session") && (l.contains("not found") || l.contains("no such") || l.contains("does not exist"))))
}

/// A continued session that failed before doing anything — no text, not one turn, nothing stopped it. Each CLI
/// says "I cannot load that session" its own way (Claude: a bare `error_during_execution`; Codex: "no rollout found
/// for thread id"; Kimi: "Internal error"), and new wordings keep coming. Whatever the words, a session that cannot
/// even start is retried fresh. Spent limits and a lost login are not: a new session would fail the same way.
pub fn resume_died_at_once(o: &Outcome) -> bool {
    let Some(f) = o.failure.as_deref() else { return false };
    let l = f.to_lowercase();
    let not_session = ["limit", "quota", "429", "login", "log in", "logged", "auth", "401", "403", "credit", "billing", "не найден на этой машине", "рабочая папка недоступна"]
        .iter()
        .any(|k| l.contains(k));
    o.text.is_empty() && !o.stopped && o.work.turns == 0 && o.work.output_tokens == 0 && !not_session
}

/// Run one turn. A resume that hits a held session is retried once after a short wait (an orphan exits when its
/// pipes close); if the session is still held, the turn runs in a fresh session rather than failing — the
/// system prompt travels with it, so the specialist is still itself. A session the CLI has lost altogether goes
/// straight to a fresh one: there is nothing to wait for, and the person should not see an error for it.
pub async fn run(spec: RunSpec, approver: &dyn Approver, tx: EventTx, cancel: CancelRx) -> Outcome {
    let mut out = run_once(spec.clone(), approver, tx.clone(), cancel.clone()).await;
    if spec.resume.is_some() && (session_is_lost(&out) || resume_died_at_once(&out)) && !*cancel.borrow() {
        let mut fresh = spec;
        fresh.resume = None;
        return run_once(fresh, approver, tx, cancel).await;
    }
    if spec.resume.is_some() && session_is_held(&out) && !*cancel.borrow() {
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        out = run_once(spec.clone(), approver, tx.clone(), cancel.clone()).await;
        if session_is_held(&out) && !*cancel.borrow() {
            let mut fresh = spec;
            fresh.resume = None;
            out = run_once(fresh, approver, tx, cancel).await;
        }
    }
    out
}

async fn run_once(spec: RunSpec, approver: &dyn Approver, tx: EventTx, cancel: CancelRx) -> Outcome {
    match spec.provider() {
        "claude" => claude::run(spec, approver, tx, cancel).await,
        "codex" => codex::run(spec, approver, tx, cancel).await,
        "kimi" => kimi::run(spec, approver, tx, cancel).await,
        "mock" => mock::run(spec, approver, tx, cancel).await,
        other => Outcome { failure: Some(format!("Бэкенд «{other}» пока не поддержан")), ..Default::default() },
    }
}

pub(crate) fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n).collect();
        t.push('…');
        t
    }
}

/// Which agent CLIs are installed on this machine (login state is checked separately, see `auth`).
pub async fn discover() -> Vec<Value> {
    let mut out = vec![];
    for (id, label, _) in auth::PROVIDERS {
        if let Some(path) = cli::resolve(id) {
            let version = cli::version(&path).await.unwrap_or_default();
            out.push(json!({"backend": id, "label": label, "version": version, "path": path.to_string_lossy()}));
        }
    }
    out
}
