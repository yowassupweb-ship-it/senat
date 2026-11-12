use serde::{Deserialize, Serialize};

macro_rules! str_enum {
    ($(#[$m:meta])* $name:ident { $($var:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name { $($var),+ }
        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$var),+];
            pub fn as_str(&self) -> &'static str { match self { $($name::$var => $s),+ } }
            pub fn parse(s: &str) -> Option<$name> { match s { $($s => Some($name::$var),)+ _ => None } }
        }
        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.serialize_str(self.as_str()) }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                $name::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("unknown {}: {}", stringify!($name), s)))
            }
        }
    };
}

str_enum!(
    /// Obligation lifecycle, Senate.md §9.
    ObligationState {
        Accepted => "accepted",
        Running => "running",
        Waiting => "waiting",
        Blocked => "blocked",
        Review => "review",
        Done => "done",
        Cancelled => "cancelled",
    }
);

impl ObligationState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, ObligationState::Done | ObligationState::Cancelled)
    }
}

str_enum!(MemoryLevel { Personal => "personal", Project => "project", Company => "company", Collective => "collective" });
str_enum!(
    /// Knowledge lifecycle, Senate.md §15.
    MemoryKind {
        Observation => "observation",
        Hypothesis => "hypothesis",
        Verified => "verified",
        Decision => "decision",
        Preference => "preference",
        Error => "error",
    }
);
str_enum!(MemoryStatus { Active => "active", Superseded => "superseded", Archived => "archived" });
str_enum!(DecisionStatus { Proposed => "proposed", Accepted => "accepted", Rejected => "rejected", Superseded => "superseded" });
str_enum!(SessionStatus { Queued => "queued", Running => "running", Done => "done", Failed => "failed", Cancelled => "cancelled" });
str_enum!(GrantMode { Auto => "auto", Approve => "approve", Deny => "deny" });
str_enum!(ApprovalStatus { Pending => "pending", Approved => "approved", Denied => "denied" });
str_enum!(ThreadKind { Lounge => "lounge", Chat => "chat", Forum => "forum" });
str_enum!(MessageStatus { Queued => "queued", Working => "working", Done => "done", Failed => "failed", Cancelled => "cancelled" });
str_enum!(
    /// What an agent may do on its own in the project folder.
    PermissionMode { Plan => "plan", Ask => "ask", Edit => "edit", Auto => "auto" }
);

/// Everything an agent may ask the environment to do. Rights come from the environment, never from
/// a profile, memory entry or a colleague's message (Senate.md §18).
pub mod action {
    pub const MESSAGE_POST: &str = "message.post";
    pub const MEMORY_WRITE: &str = "memory.write";
    pub const MEMORY_PROMOTE: &str = "memory.promote";
    pub const OBLIGATION_UPDATE: &str = "obligation.update";
    pub const OBLIGATION_CREATE: &str = "obligation.create";
    pub const OBLIGATION_DELEGATE: &str = "obligation.delegate";
    /// Acting on a task that is not one's own (close, cancel, hand over): on the person's behalf, so it is theirs to allow.
    pub const OBLIGATION_MANAGE: &str = "obligation.manage";
    /// A specialist calls a colleague into the conversation to ask or hand over a small piece of work.
    pub const COLLEAGUE_CALL: &str = "colleague.call";
    pub const DECISION_PROPOSE: &str = "decision.propose";
    pub const DECISION_ACCEPT: &str = "decision.accept";
    pub const ARTIFACT_WRITE: &str = "artifact.write";
    pub const EXTERNAL_SEND: &str = "external.send";
    pub const PROFILE_CHANGE: &str = "profile.change";
    pub const PROJECT_CREATE: &str = "project.create";
    pub const MCP_INSTALL: &str = "mcp.install";
    pub const API_CONNECT: &str = "api.connect";

    pub const ALL: &[&str] = &[
        MESSAGE_POST,
        MEMORY_WRITE,
        MEMORY_PROMOTE,
        OBLIGATION_UPDATE,
        OBLIGATION_CREATE,
        OBLIGATION_DELEGATE,
        OBLIGATION_MANAGE,
        COLLEAGUE_CALL,
        DECISION_PROPOSE,
        DECISION_ACCEPT,
        ARTIFACT_WRITE,
        EXTERNAL_SEND,
        PROFILE_CHANGE,
        PROJECT_CREATE,
        MCP_INSTALL,
        API_CONNECT,
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub role: String,
    pub profile: String,
    pub accent: String,
    pub status: String,
    pub model_pref: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// Address of the uploaded picture ("" = none: the first letter on the accent colour is shown).
    #[serde(default)]
    pub avatar: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewAgent {
    pub name: String,
    pub role: String,
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub accent: String,
    #[serde(default)]
    pub model_pref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub goal: String,
    pub criteria: String,
    pub constraints: String,
    pub status: String,
    pub budget_tokens: i64,
    pub root_path: String,
    pub node_id: Option<String>,
    pub mode: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub company_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Company {
    pub id: String,
    pub name: String,
    pub brief: String,
    pub color: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewCompany {
    pub name: String,
    #[serde(default)]
    pub brief: String,
    #[serde(default)]
    pub color: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewProject {
    pub name: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub criteria: String,
    #[serde(default)]
    pub constraints: String,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub root_path: String,
    pub node_id: Option<String>,
    pub company_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WaitCondition {
    /// Resume at a point in time (ms).
    Time { at: i64 },
    /// Resume when an event of this kind (optionally on this subject) occurs.
    Event { kind: String, #[serde(default)] subject: Option<String> },
    /// Resume when another obligation is done.
    Obligation { id: String },
    /// Resume when the user answers.
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Obligation {
    pub id: String,
    pub title: String,
    pub result: String,
    pub criteria: String,
    pub owner_id: Option<String>,
    pub project_id: Option<String>,
    pub parent_id: Option<String>,
    pub requested_by: String,
    pub state: ObligationState,
    pub next_step: String,
    pub wait_condition: Option<WaitCondition>,
    pub blocker: String,
    pub outcome: String,
    pub due_at: Option<i64>,
    pub budget_tokens: i64,
    pub spent_tokens: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewObligation {
    pub title: String,
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub criteria: String,
    pub owner_id: Option<String>,
    pub project_id: Option<String>,
    pub parent_id: Option<String>,
    #[serde(default)]
    pub requested_by: String,
    #[serde(default)]
    pub next_step: String,
    pub due_at: Option<i64>,
    #[serde(default)]
    pub budget_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    pub project_id: Option<String>,
    pub company_id: Option<String>,
    pub title: String,
    pub body: String,
    pub status: DecisionStatus,
    pub decided_by: String,
    pub supersedes_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub id: String,
    pub kind: ThreadKind,
    pub project_id: Option<String>,
    pub agent_id: Option<String>,
    pub title: String,
    pub auto_title: bool,
    pub archived: bool,
    pub pinned: bool,
    pub model: String,
    pub effort: String,
    pub mode: String,
    pub cli_sessions: serde_json::Value,
    pub created_at: i64,
    pub updated_at: i64,
    pub company_id: Option<String>,
    /// Forum state (topic, outcome, participants, rounds, status, plan); `{}` for ordinary chats.
    pub meta: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewThread {
    pub project_id: Option<String>,
    pub company_id: Option<String>,
    pub agent_id: Option<String>,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub thread_id: String,
    pub author_kind: String,
    pub author_id: Option<String>,
    pub body: String,
    pub props: serde_json::Value,
    pub status: MessageStatus,
    pub session_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub level: MemoryLevel,
    pub agent_id: Option<String>,
    pub project_id: Option<String>,
    pub kind: MemoryKind,
    pub body: String,
    pub source: String,
    pub scope_note: String,
    pub verification: String,
    pub status: MemoryStatus,
    pub supersedes_id: Option<String>,
    pub fresh_until: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub company_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewMemory {
    pub level: MemoryLevel,
    pub agent_id: Option<String>,
    pub project_id: Option<String>,
    #[serde(default)]
    pub company_id: Option<String>,
    pub kind: MemoryKind,
    pub body: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub scope_note: String,
    #[serde(default)]
    pub verification: String,
    pub fresh_until: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub project_id: Option<String>,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub version: i64,
    pub created_by: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub os: String,
    pub kind: String,
    pub capabilities: serde_json::Value,
    pub last_seen: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub agent_id: String,
    pub obligation_id: Option<String>,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub reason: String,
    pub status: SessionStatus,
    pub backend: String,
    pub node_id: Option<String>,
    pub prompt: String,
    pub output: String,
    pub summary: String,
    pub error: String,
    pub applied: bool,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approval {
    pub id: String,
    pub agent_id: String,
    pub session_id: Option<String>,
    pub obligation_id: Option<String>,
    pub action: String,
    pub detail: serde_json::Value,
    pub status: ApprovalStatus,
    pub created_at: i64,
    pub decided_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub ts: i64,
    pub kind: String,
    pub subject_type: String,
    pub subject_id: String,
    pub actor: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grant {
    pub id: String,
    pub agent_id: Option<String>,
    pub project_id: Option<String>,
    pub action: String,
    pub mode: GrantMode,
}

/// An MCP server a specialist may be given. Secret values (env, headers) exist only here, for the
/// runtime; the API exposes `view()` with key names only.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
    pub url: String,
    pub headers: std::collections::BTreeMap<String, String>,
    pub note: String,
    pub enabled: bool,
    pub trusted: bool,
    pub shared: bool,
    pub agents: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl McpServer {
    pub fn view(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id, "name": self.name, "kind": self.kind, "command": self.command, "args": self.args,
            "url": self.url, "note": self.note, "enabled": self.enabled, "trusted": self.trusted, "shared": self.shared,
            "agents": self.agents, "env_keys": self.env.keys().collect::<Vec<_>>(), "header_keys": self.headers.keys().collect::<Vec<_>>(),
            "missing": self.env.iter().chain(self.headers.iter()).filter(|(_, v)| v.is_empty()).map(|(k, _)| k.clone()).collect::<Vec<_>>(),
            "created_at": self.created_at, "updated_at": self.updated_at,
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpInput {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    /// Merged into the stored env: an empty value keeps the stored one; keys in `env_remove` are dropped.
    pub env: Option<std::collections::BTreeMap<String, String>>,
    pub env_remove: Option<Vec<String>>,
    pub url: Option<String>,
    pub headers: Option<std::collections::BTreeMap<String, String>>,
    pub headers_remove: Option<Vec<String>>,
    pub note: Option<String>,
    pub enabled: Option<bool>,
    pub trusted: Option<bool>,
    pub shared: Option<bool>,
    pub agents: Option<Vec<String>>,
}

/// Work a specialist does on a schedule. Each time it is due, a task is made for them and the usual machinery
/// (scheduler, sessions, reports, review) takes it from there.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Routine {
    pub id: String,
    pub agent_id: String,
    pub project_id: Option<String>,
    pub name: String,
    pub cron: String,
    pub title: String,
    pub prompt: String,
    pub enabled: bool,
    pub source: String,
    pub last_run_at: Option<i64>,
    pub next_run_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoutineInput {
    pub agent_id: Option<String>,
    pub project_id: Option<String>,
    pub name: Option<String>,
    pub cron: Option<String>,
    pub title: Option<String>,
    pub prompt: Option<String>,
    pub enabled: Option<bool>,
    pub source: Option<String>,
}

/// "Allow always": the person said yes to this kind of action and does not want to be asked again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub id: String,
    /// path | cmd | tool
    pub kind: String,
    pub value: String,
    pub label: String,
    pub created_at: i64,
}

/// Something a specialist may be given to use: a server, a database, a site login. The secret stays in the node.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Access {
    pub id: String,
    pub name: String,
    pub label: String,
    pub kind: String,
    pub host: String,
    pub login: String,
    pub secret: String,
    pub note: String,
    pub enabled: bool,
    /// specialist id → "allow" | "ask". A specialist not listed has no access.
    pub grants: std::collections::BTreeMap<String, String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Access {
    /// What the UI and the API may show: everything but the secret.
    pub fn view(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id, "name": self.name, "label": self.label, "kind": self.kind, "host": self.host, "login": self.login,
            "has_secret": !self.secret.is_empty(), "note": self.note, "enabled": self.enabled, "grants": self.grants,
            "created_at": self.created_at, "updated_at": self.updated_at,
        })
    }
    /// "allow", "ask" or "none" for one specialist.
    pub fn mode_for(&self, agent_id: &str) -> &str {
        self.grants.get(agent_id).map(String::as_str).filter(|m| matches!(*m, "allow" | "ask")).unwrap_or("none")
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccessInput {
    pub name: Option<String>,
    pub label: Option<String>,
    pub kind: Option<String>,
    pub host: Option<String>,
    pub login: Option<String>,
    /// Empty keeps the stored secret.
    pub secret: Option<String>,
    pub clear_secret: Option<bool>,
    pub note: Option<String>,
    pub enabled: Option<bool>,
    /// specialist id → "allow" | "ask" | "deny" (deny removes the grant).
    pub grants: Option<std::collections::BTreeMap<String, String>>,
}

/// A connection to an external HTTP API. The secret is used by the node when an agent calls the
/// service; the agent and the API never see it.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Integration {
    pub id: String,
    pub name: String,
    pub label: String,
    pub base_url: String,
    pub auth_type: String,
    pub auth_name: String,
    pub secret: String,
    pub docs: String,
    pub write_mode: String,
    pub enabled: bool,
    pub shared: bool,
    pub agents: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Integration {
    pub fn view(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id, "name": self.name, "label": self.label, "base_url": self.base_url, "auth_type": self.auth_type,
            "auth_name": self.auth_name, "has_secret": !self.secret.is_empty(), "docs": self.docs, "write_mode": self.write_mode,
            "enabled": self.enabled, "shared": self.shared, "agents": self.agents, "created_at": self.created_at, "updated_at": self.updated_at,
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IntegrationInput {
    pub name: Option<String>,
    pub label: Option<String>,
    pub base_url: Option<String>,
    pub auth_type: Option<String>,
    pub auth_name: Option<String>,
    /// Empty keeps the stored secret.
    pub secret: Option<String>,
    pub clear_secret: Option<bool>,
    pub docs: Option<String>,
    pub write_mode: Option<String>,
    pub enabled: Option<bool>,
    pub shared: Option<bool>,
    pub agents: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewForum {
    pub topic: String,
    /// plan | decision | analysis
    #[serde(default)]
    pub outcome: String,
    pub participants: Vec<String>,
    #[serde(default)]
    pub rounds: i64,
    pub project_id: Option<String>,
    pub company_id: Option<String>,
    /// Who sums up. Defaults to the first participant.
    pub leader_id: Option<String>,
}

/// A company or project recognised by name in a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mention {
    pub kind: &'static str,
    pub id: String,
    pub name: String,
    pub company_id: Option<String>,
}
