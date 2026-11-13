//! Schema with real migrations from day one (MBOX had none and paid for it).
//! Append new entries to `MIGRATIONS`; never edit an applied one.

use rusqlite::Connection;

pub const MIGRATIONS: &[&str] = &[
    // 1 — foundation
    r#"
CREATE TABLE agents (
  id TEXT PRIMARY KEY,
  slug TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  role TEXT NOT NULL,                 -- architect | developer | designer | analyst | business | custom
  profile TEXT NOT NULL DEFAULT '',   -- the "mask": optics, quality criteria, boundaries
  accent TEXT NOT NULL DEFAULT '#8e8e93',
  status TEXT NOT NULL DEFAULT 'active',      -- active | retired
  model_pref TEXT NOT NULL DEFAULT '',        -- preferred backend id, '' = environment decides
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE agent_profile_versions (
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL REFERENCES agents(id),
  profile TEXT NOT NULL,
  note TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
CREATE TABLE grants (
  id TEXT PRIMARY KEY,
  agent_id TEXT,                      -- NULL = every agent
  project_id TEXT,                    -- NULL = every project
  action TEXT NOT NULL,
  mode TEXT NOT NULL,                 -- auto | approve | deny
  UNIQUE(agent_id, project_id, action)
);
CREATE TABLE projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  goal TEXT NOT NULL DEFAULT '',
  criteria TEXT NOT NULL DEFAULT '',
  constraints TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'active',      -- active | paused | done | archived
  budget_tokens INTEGER NOT NULL DEFAULT 0,   -- 0 = unlimited
  root_path TEXT NOT NULL DEFAULT '',         -- local working folder; agents run here
  node_id TEXT,                               -- node that owns the folder; NULL = this machine
  mode TEXT NOT NULL DEFAULT 'edit',          -- default permission mode for chats: plan | ask | edit | auto
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE project_members (
  project_id TEXT NOT NULL REFERENCES projects(id),
  agent_id TEXT NOT NULL REFERENCES agents(id),
  PRIMARY KEY(project_id, agent_id)
);
CREATE TABLE decisions (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id),
  title TEXT NOT NULL,
  body TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'proposed',    -- proposed | accepted | rejected | superseded
  decided_by TEXT NOT NULL DEFAULT '',        -- 'user' | agent id
  supersedes_id TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE obligations (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  result TEXT NOT NULL DEFAULT '',            -- expected result
  criteria TEXT NOT NULL DEFAULT '',          -- completion criteria
  owner_id TEXT REFERENCES agents(id),        -- responsible specialist
  project_id TEXT REFERENCES projects(id),
  parent_id TEXT REFERENCES obligations(id),
  requested_by TEXT NOT NULL DEFAULT 'user',  -- 'user' | agent id
  state TEXT NOT NULL DEFAULT 'accepted',
  next_step TEXT NOT NULL DEFAULT '',
  wait_condition TEXT,                        -- JSON: {"type":"time","at":ms} | {"type":"event","kind":"...","subject":"..."} | {"type":"obligation","id":"..."} | {"type":"user"}
  blocker TEXT NOT NULL DEFAULT '',
  outcome TEXT NOT NULL DEFAULT '',           -- what was actually delivered / why cancelled
  due_at INTEGER,
  budget_tokens INTEGER NOT NULL DEFAULT 0,
  spent_tokens INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX obligations_state ON obligations(state);
CREATE INDEX obligations_owner ON obligations(owner_id);
CREATE INDEX obligations_project ON obligations(project_id);
CREATE TABLE threads (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL DEFAULT 'chat',          -- lounge | chat
  project_id TEXT REFERENCES projects(id),
  agent_id TEXT REFERENCES agents(id),        -- addressee; NULL = routed by mention / topic
  title TEXT NOT NULL DEFAULT '',
  auto_title INTEGER NOT NULL DEFAULT 1,
  archived INTEGER NOT NULL DEFAULT 0,
  pinned INTEGER NOT NULL DEFAULT 0,
  model TEXT NOT NULL DEFAULT '',             -- 'claude:opus' | 'codex' | '' = agent / environment default
  effort TEXT NOT NULL DEFAULT '',
  mode TEXT NOT NULL DEFAULT '',              -- '' = project default
  cli_sessions TEXT NOT NULL DEFAULT '{}',    -- JSON {backend: cli session id} so a chat resumes after restarts
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX threads_project ON threads(project_id, archived, updated_at);
CREATE TABLE messages (
  id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL REFERENCES threads(id),
  author_kind TEXT NOT NULL,                  -- user | agent | system
  author_id TEXT,
  body TEXT NOT NULL DEFAULT '',
  props TEXT NOT NULL DEFAULT '{}',           -- steps, reasoning, work, ask, attachments, model ...
  status TEXT NOT NULL DEFAULT 'done',        -- queued | working | done | failed | cancelled
  session_id TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX messages_thread ON messages(thread_id, created_at);
CREATE TABLE memory (
  id TEXT PRIMARY KEY,
  level TEXT NOT NULL,                        -- personal | project | collective
  agent_id TEXT REFERENCES agents(id),        -- owner for personal
  project_id TEXT REFERENCES projects(id),
  kind TEXT NOT NULL,                         -- observation | hypothesis | verified | decision | preference | error
  body TEXT NOT NULL,
  source TEXT NOT NULL DEFAULT '',            -- origin: session id, url, file, 'user'
  scope_note TEXT NOT NULL DEFAULT '',        -- where it applies
  verification TEXT NOT NULL DEFAULT '',      -- basis of verification (not model confidence)
  status TEXT NOT NULL DEFAULT 'active',      -- active | superseded | archived
  supersedes_id TEXT,
  fresh_until INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX memory_scope ON memory(level, agent_id, project_id, status);
CREATE VIRTUAL TABLE memory_fts USING fts5(body, scope_note, content='memory', content_rowid='rowid', tokenize='unicode61 remove_diacritics 2');
CREATE TRIGGER memory_ai AFTER INSERT ON memory BEGIN
  INSERT INTO memory_fts(rowid, body, scope_note) VALUES (new.rowid, new.body, new.scope_note);
END;
CREATE TRIGGER memory_ad AFTER DELETE ON memory BEGIN
  INSERT INTO memory_fts(memory_fts, rowid, body, scope_note) VALUES ('delete', old.rowid, old.body, old.scope_note);
END;
CREATE TRIGGER memory_au AFTER UPDATE OF body, scope_note ON memory BEGIN
  INSERT INTO memory_fts(memory_fts, rowid, body, scope_note) VALUES ('delete', old.rowid, old.body, old.scope_note);
  INSERT INTO memory_fts(rowid, body, scope_note) VALUES (new.rowid, new.body, new.scope_note);
END;
CREATE TABLE artifacts (
  id TEXT PRIMARY KEY,
  project_id TEXT REFERENCES projects(id),
  kind TEXT NOT NULL,                         -- document | code | design | research | other
  title TEXT NOT NULL,
  body TEXT NOT NULL DEFAULT '',
  version INTEGER NOT NULL DEFAULT 1,
  created_by TEXT NOT NULL DEFAULT 'user',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE nodes (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  os TEXT NOT NULL DEFAULT '',
  kind TEXT NOT NULL DEFAULT 'remote',        -- local | remote
  token_hash TEXT NOT NULL DEFAULT '',
  capabilities TEXT NOT NULL DEFAULT '[]',    -- JSON array of backend descriptors
  last_seen INTEGER,
  created_at INTEGER NOT NULL
);
CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL REFERENCES agents(id),
  obligation_id TEXT REFERENCES obligations(id),
  thread_id TEXT REFERENCES threads(id),
  message_id TEXT,                            -- agent message the run streams into
  reason TEXT NOT NULL,                       -- why this run exists
  status TEXT NOT NULL DEFAULT 'queued',      -- queued | running | done | failed | cancelled
  backend TEXT NOT NULL DEFAULT '',
  node_id TEXT,
  prompt TEXT NOT NULL DEFAULT '',
  output TEXT NOT NULL DEFAULT '',
  summary TEXT NOT NULL DEFAULT '',
  error TEXT NOT NULL DEFAULT '',
  applied INTEGER NOT NULL DEFAULT 0,         -- report applied exactly once
  tokens_in INTEGER NOT NULL DEFAULT 0,
  tokens_out INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  started_at INTEGER,
  ended_at INTEGER
);
CREATE INDEX sessions_status ON sessions(status);
CREATE UNIQUE INDEX sessions_one_active_per_obligation
  ON sessions(obligation_id) WHERE obligation_id IS NOT NULL AND status IN ('queued','running');
CREATE TABLE approvals (
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL REFERENCES agents(id),
  session_id TEXT,
  obligation_id TEXT,
  action TEXT NOT NULL,
  detail TEXT NOT NULL,                       -- JSON payload to execute when approved
  status TEXT NOT NULL DEFAULT 'pending',     -- pending | approved | denied
  created_at INTEGER NOT NULL,
  decided_at INTEGER
);
CREATE TABLE events (
  id TEXT PRIMARY KEY,
  ts INTEGER NOT NULL,
  kind TEXT NOT NULL,
  subject_type TEXT NOT NULL,
  subject_id TEXT NOT NULL,
  actor TEXT NOT NULL DEFAULT 'system',
  payload TEXT NOT NULL DEFAULT '{}',
  consumed INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX events_subject ON events(subject_type, subject_id, ts);
CREATE INDEX events_unconsumed ON events(consumed, ts);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#,
    // 2 - MCP servers and who may use them
    r#"
CREATE TABLE mcp_servers (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,                  -- [a-z][a-z0-9_-]{1,30}; the agent sees tools as mcp__<name>__*
  kind TEXT NOT NULL DEFAULT 'stdio',         -- stdio | http
  command TEXT NOT NULL DEFAULT '',
  args TEXT NOT NULL DEFAULT '[]',            -- JSON array
  env TEXT NOT NULL DEFAULT '{}',             -- JSON object (secrets live here: never returned by the API)
  url TEXT NOT NULL DEFAULT '',
  headers TEXT NOT NULL DEFAULT '{}',         -- JSON object (secrets: never returned by the API)
  note TEXT NOT NULL DEFAULT '',
  enabled INTEGER NOT NULL DEFAULT 1,
  trusted INTEGER NOT NULL DEFAULT 0,         -- 1 = its tools run without asking
  shared INTEGER NOT NULL DEFAULT 0,          -- 1 = every specialist has it
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE agent_mcp (
  agent_id TEXT NOT NULL REFERENCES agents(id),
  server_id TEXT NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
  PRIMARY KEY (agent_id, server_id)
);
"#,
    // 3 - companies: clusters of projects; a specialist works across companies and keeps memory of each
    r#"
CREATE TABLE companies (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  brief TEXT NOT NULL DEFAULT '',             -- who they are, how they work, what matters to them
  color TEXT NOT NULL DEFAULT '#8e8e93',
  status TEXT NOT NULL DEFAULT 'active',      -- active | archived
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
ALTER TABLE projects ADD COLUMN company_id TEXT REFERENCES companies(id) ON DELETE SET NULL;
ALTER TABLE threads ADD COLUMN company_id TEXT REFERENCES companies(id) ON DELETE SET NULL;
-- memory: level 'company' (everyone in the company) and personal+company_id (this specialist, in this company only)
ALTER TABLE memory ADD COLUMN company_id TEXT REFERENCES companies(id) ON DELETE SET NULL;
CREATE INDEX memory_company ON memory(company_id, status);
-- decisions may belong to a company (shared by all its projects) instead of one project
CREATE TABLE decisions_new (
  id TEXT PRIMARY KEY,
  project_id TEXT REFERENCES projects(id),
  title TEXT NOT NULL,
  body TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'proposed',
  decided_by TEXT NOT NULL DEFAULT '',
  supersedes_id TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  company_id TEXT REFERENCES companies(id)
);
INSERT INTO decisions_new(id,project_id,title,body,status,decided_by,supersedes_id,created_at,updated_at)
  SELECT id,project_id,title,body,status,decided_by,supersedes_id,created_at,updated_at FROM decisions;
DROP TABLE decisions;
ALTER TABLE decisions_new RENAME TO decisions;
"#,
    // 4 - external API connections an agent can call through the node (secrets never leave it)
    r#"
CREATE TABLE integrations (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,                  -- how the agent names the service: [a-z][a-z0-9_-]{1,30}
  label TEXT NOT NULL DEFAULT '',
  base_url TEXT NOT NULL,
  auth_type TEXT NOT NULL DEFAULT 'bearer',   -- none | bearer | header | basic | query
  auth_name TEXT NOT NULL DEFAULT '',         -- header or query parameter name (header | query)
  secret TEXT NOT NULL DEFAULT '',            -- write-only: never returned by the API
  docs TEXT NOT NULL DEFAULT '',              -- what the API is for and how to call it (shown to the agent)
  write_mode TEXT NOT NULL DEFAULT 'approve', -- POST/PUT/PATCH/DELETE: deny | approve | auto
  enabled INTEGER NOT NULL DEFAULT 1,
  shared INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE agent_integrations (
  agent_id TEXT NOT NULL REFERENCES agents(id),
  integration_id TEXT NOT NULL REFERENCES integrations(id) ON DELETE CASCADE,
  PRIMARY KEY (agent_id, integration_id)
);
"#,
    // 5 - forum: a discussion among specialists with a topic, rounds and a stated outcome
    r#"
ALTER TABLE threads ADD COLUMN meta TEXT NOT NULL DEFAULT '{}';
"#,
    // 6 - a specialist's picture: the address of an uploaded image ('' = the letter on a colour)
    r#"
ALTER TABLE agents ADD COLUMN avatar TEXT NOT NULL DEFAULT '';
"#,
    // 7 - accesses: servers, databases and sites the person lets specialists use, one by one
    r#"
CREATE TABLE accesses (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,                  -- how the specialist names it: [a-z][a-z0-9_-]{1,30}
  label TEXT NOT NULL DEFAULT '',
  kind TEXT NOT NULL DEFAULT 'server',        -- server | database | site | service | other
  host TEXT NOT NULL DEFAULT '',              -- address: ssh user@host:port, https://..., a connection string without the password
  login TEXT NOT NULL DEFAULT '',
  secret TEXT NOT NULL DEFAULT '',            -- write-only: never returned by the API, never kept in chat history
  note TEXT NOT NULL DEFAULT '',              -- what it is and how to use it (shown to the specialist)
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE access_grants (
  access_id TEXT NOT NULL REFERENCES accesses(id) ON DELETE CASCADE,
  agent_id TEXT NOT NULL REFERENCES agents(id),
  mode TEXT NOT NULL,                         -- allow | ask   (no row: no access)
  PRIMARY KEY (access_id, agent_id)
);
CREATE TABLE access_permits (
  id TEXT PRIMARY KEY,
  access_id TEXT NOT NULL REFERENCES accesses(id) ON DELETE CASCADE,
  agent_id TEXT NOT NULL REFERENCES agents(id),
  expires_at INTEGER NOT NULL                 -- the person said yes to one use: valid for a short while, once
);
"#,
    // 8 - "allow always": what the person decided not to be asked about again
    r#"
CREATE TABLE permission_rules (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,                         -- path | cmd | tool
  value TEXT NOT NULL,                        -- path: a folder; cmd: the start of a simple command; tool: the tool's name
  label TEXT NOT NULL DEFAULT '',             -- what the person was told they were agreeing to
  created_at INTEGER NOT NULL,
  UNIQUE(kind, value)
);
"#,
    // 9 - routines: work a specialist does on a schedule (cron)
    r#"
CREATE TABLE routines (
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL REFERENCES agents(id),
  project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
  name TEXT NOT NULL,                         -- "Понедельник": what the person sees in the list
  cron TEXT NOT NULL,                         -- minute hour day-of-month month day-of-week, in the node's local time
  title TEXT NOT NULL DEFAULT '',             -- title of the task made each time (the date is added)
  prompt TEXT NOT NULL DEFAULT '',            -- what to do and what the result must be
  enabled INTEGER NOT NULL DEFAULT 1,
  source TEXT NOT NULL DEFAULT '',            -- '' = made by the person; 'skill:<name>' = declared by a skill
  last_run_at INTEGER,
  next_run_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(agent_id, name)
);
"#,
    // 10 - a specialist's gender: 'm' | 'f' | '' (not set). It decides how they speak about themselves.
    r#"
ALTER TABLE agents ADD COLUMN gender TEXT NOT NULL DEFAULT '';
"#,
    // 11 - notes between specialists: delivered while the recipient works (with their next Senate tool result) or at
    // their next turn, never lost to a call-chain limit; the sender can see when it was read.
    r#"
CREATE TABLE agent_notes (
  id TEXT PRIMARY KEY,
  thread_id TEXT,
  from_agent TEXT NOT NULL REFERENCES agents(id),
  to_agent TEXT NOT NULL REFERENCES agents(id),
  text TEXT NOT NULL,
  urgent INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  delivered_at INTEGER                         -- NULL = not seen by the recipient yet
);
CREATE INDEX agent_notes_to ON agent_notes(to_agent, delivered_at);
"#,
];

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let current: usize = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}
