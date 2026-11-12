//! SQLite-backed store. One connection behind a mutex: Senate is a single-user environment and
//! SQLite in WAL mode is plenty. Composite operations take the lock once so that a state change and
//! the event describing it are always written together.

use crate::error::{Error, Result};
use crate::model::*;
use crate::{new_id, now_ms, schema};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

fn en<T>(r: &Row, i: usize, parse: fn(&str) -> Option<T>, fallback: T) -> rusqlite::Result<T> {
    let s: String = r.get(i)?;
    Ok(parse(&s).unwrap_or(fallback))
}

fn json_or(s: &str, fallback: serde_json::Value) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or(fallback)
}

// ---------- row mappers ----------

const AGENT_COLS: &str = "id,slug,name,role,profile,accent,status,model_pref,created_at,updated_at,avatar,gender";
fn agent_from(r: &Row) -> rusqlite::Result<Agent> {
    Ok(Agent {
        id: r.get(0)?,
        slug: r.get(1)?,
        name: r.get(2)?,
        role: r.get(3)?,
        profile: r.get(4)?,
        accent: r.get(5)?,
        status: r.get(6)?,
        model_pref: r.get(7)?,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
        avatar: r.get(10)?,
        gender: r.get(11)?,
    })
}

const PROJECT_COLS: &str = "id,name,goal,criteria,constraints,status,budget_tokens,root_path,node_id,mode,created_at,updated_at,company_id";
fn project_from(r: &Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: r.get(0)?,
        name: r.get(1)?,
        goal: r.get(2)?,
        criteria: r.get(3)?,
        constraints: r.get(4)?,
        status: r.get(5)?,
        budget_tokens: r.get(6)?,
        root_path: r.get(7)?,
        node_id: r.get(8)?,
        mode: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
        company_id: r.get(12)?,
    })
}

const OBL_COLS: &str = "id,title,result,criteria,owner_id,project_id,parent_id,requested_by,state,next_step,wait_condition,blocker,outcome,due_at,budget_tokens,spent_tokens,created_at,updated_at";
fn obligation_from(r: &Row) -> rusqlite::Result<Obligation> {
    let wait: Option<String> = r.get(10)?;
    Ok(Obligation {
        id: r.get(0)?,
        title: r.get(1)?,
        result: r.get(2)?,
        criteria: r.get(3)?,
        owner_id: r.get(4)?,
        project_id: r.get(5)?,
        parent_id: r.get(6)?,
        requested_by: r.get(7)?,
        state: en(r, 8, ObligationState::parse, ObligationState::Accepted)?,
        next_step: r.get(9)?,
        wait_condition: wait.and_then(|w| serde_json::from_str(&w).ok()),
        blocker: r.get(11)?,
        outcome: r.get(12)?,
        due_at: r.get(13)?,
        budget_tokens: r.get(14)?,
        spent_tokens: r.get(15)?,
        created_at: r.get(16)?,
        updated_at: r.get(17)?,
    })
}

const DECISION_COLS: &str = "id,project_id,title,body,status,decided_by,supersedes_id,created_at,updated_at,company_id";
fn decision_from(r: &Row) -> rusqlite::Result<Decision> {
    Ok(Decision {
        id: r.get(0)?,
        project_id: r.get(1)?,
        company_id: r.get(9)?,
        title: r.get(2)?,
        body: r.get(3)?,
        status: en(r, 4, DecisionStatus::parse, DecisionStatus::Proposed)?,
        decided_by: r.get(5)?,
        supersedes_id: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

const MEMORY_COLS: &str = "m.id,m.level,m.agent_id,m.project_id,m.kind,m.body,m.source,m.scope_note,m.verification,m.status,m.supersedes_id,m.fresh_until,m.created_at,m.updated_at,m.company_id";
fn memory_from(r: &Row) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: r.get(0)?,
        level: en(r, 1, MemoryLevel::parse, MemoryLevel::Personal)?,
        agent_id: r.get(2)?,
        project_id: r.get(3)?,
        kind: en(r, 4, MemoryKind::parse, MemoryKind::Observation)?,
        body: r.get(5)?,
        source: r.get(6)?,
        scope_note: r.get(7)?,
        verification: r.get(8)?,
        status: en(r, 9, MemoryStatus::parse, MemoryStatus::Active)?,
        supersedes_id: r.get(10)?,
        fresh_until: r.get(11)?,
        created_at: r.get(12)?,
        updated_at: r.get(13)?,
        company_id: r.get(14)?,
    })
}

const SESSION_COLS: &str = "id,agent_id,obligation_id,thread_id,message_id,reason,status,backend,node_id,prompt,output,summary,error,applied,tokens_in,tokens_out,created_at,started_at,ended_at";
fn session_from(r: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        agent_id: r.get(1)?,
        obligation_id: r.get(2)?,
        thread_id: r.get(3)?,
        message_id: r.get(4)?,
        reason: r.get(5)?,
        status: en(r, 6, SessionStatus::parse, SessionStatus::Queued)?,
        backend: r.get(7)?,
        node_id: r.get(8)?,
        prompt: r.get(9)?,
        output: r.get(10)?,
        summary: r.get(11)?,
        error: r.get(12)?,
        applied: r.get::<_, i64>(13)? != 0,
        tokens_in: r.get(14)?,
        tokens_out: r.get(15)?,
        created_at: r.get(16)?,
        started_at: r.get(17)?,
        ended_at: r.get(18)?,
    })
}

const APPROVAL_COLS: &str = "id,agent_id,session_id,obligation_id,action,detail,status,created_at,decided_at";
fn approval_from(r: &Row) -> rusqlite::Result<Approval> {
    let detail: String = r.get(5)?;
    Ok(Approval {
        id: r.get(0)?,
        agent_id: r.get(1)?,
        session_id: r.get(2)?,
        obligation_id: r.get(3)?,
        action: r.get(4)?,
        detail: json_or(&detail, serde_json::Value::Null),
        status: en(r, 6, ApprovalStatus::parse, ApprovalStatus::Pending)?,
        created_at: r.get(7)?,
        decided_at: r.get(8)?,
    })
}

const EVENT_COLS: &str = "id,ts,kind,subject_type,subject_id,actor,payload";
fn event_from(r: &Row) -> rusqlite::Result<Event> {
    let payload: String = r.get(6)?;
    Ok(Event {
        id: r.get(0)?,
        ts: r.get(1)?,
        kind: r.get(2)?,
        subject_type: r.get(3)?,
        subject_id: r.get(4)?,
        actor: r.get(5)?,
        payload: json_or(&payload, serde_json::json!({})),
    })
}

pub(crate) fn emit_c(
    c: &Connection,
    kind: &str,
    subject_type: &str,
    subject_id: &str,
    actor: &str,
    payload: serde_json::Value,
) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO events(id,ts,kind,subject_type,subject_id,actor,payload) VALUES (?,?,?,?,?,?,?)",
        params![new_id(), now_ms(), kind, subject_type, subject_id, actor, payload.to_string()],
    )?;
    Ok(())
}

/// Turn free text into a safe FTS5 prefix query: `"foo"* OR "bar"*`.
fn fts_query(text: &str) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    let terms: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3)
        .map(|w| w.to_lowercase())
        .filter(|w| seen.insert(w.clone()))
        .take(16)
        .map(|w| format!("\"{}\"*", w))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" OR "))
    }
}

fn translit(ch: char) -> Option<&'static str> {
    Some(match ch {
        'а' => "a", 'б' => "b", 'в' => "v", 'г' => "g", 'д' => "d", 'е' | 'э' => "e", 'ё' => "yo", 'ж' => "zh",
        'з' => "z", 'и' => "i", 'й' => "y", 'к' => "k", 'л' => "l", 'м' => "m", 'н' => "n", 'о' => "o", 'п' => "p",
        'р' => "r", 'с' => "s", 'т' => "t", 'у' => "u", 'ф' => "f", 'х' => "h", 'ц' => "ts", 'ч' => "ch", 'ш' => "sh",
        'щ' => "sch", 'ъ' | 'ь' => "", 'ы' => "y", 'ю' => "yu", 'я' => "ya",
        _ => return None,
    })
}

fn slugify(name: &str) -> String {
    let mut s = String::new();
    for ch in name.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch);
        } else if let Some(t) = translit(ch) {
            s.push_str(t);
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        format!("agent-{}", &new_id()[20..])
    } else {
        s
    }
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Store> {
        if let Some(dir) = path.as_ref().parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        schema::migrate(&conn)?;
        let store = Store { conn: Arc::new(Mutex::new(conn)) };
        store.lounge()?;
        Ok(store)
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---------- settings ----------
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .lock()
            .query_row("SELECT value FROM settings WHERE key=?", [key], |r| r.get(0))
            .optional()?)
    }
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.lock().execute(
            "INSERT INTO settings(key,value) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ---------- events ----------
    pub fn emit(&self, kind: &str, subject_type: &str, subject_id: &str, actor: &str, payload: serde_json::Value) -> Result<()> {
        emit_c(&self.lock(), kind, subject_type, subject_id, actor, payload)?;
        Ok(())
    }

    /// Journal for one subject (or the latest events overall), newest first.
    pub fn events(&self, subject_type: Option<&str>, subject_id: Option<&str>, limit: i64) -> Result<Vec<Event>> {
        let c = self.lock();
        let (sql, args): (String, Vec<String>) = match (subject_type, subject_id) {
            (Some(t), Some(i)) => (
                format!("SELECT {EVENT_COLS} FROM events WHERE subject_type=? AND subject_id=? ORDER BY ts DESC, id DESC LIMIT {limit}"),
                vec![t.into(), i.into()],
            ),
            _ => (format!("SELECT {EVENT_COLS} FROM events ORDER BY ts DESC, id DESC LIMIT {limit}"), vec![]),
        };
        let mut st = c.prepare(&sql)?;
        let rows = st.query_map(params_from_iter(args.iter()), event_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Events not yet handled by the scheduler, oldest first.
    pub fn take_unconsumed_events(&self, limit: i64) -> Result<Vec<Event>> {
        let c = self.lock();
        let rows = {
            let mut st = c.prepare(&format!("SELECT {EVENT_COLS} FROM events WHERE consumed=0 ORDER BY ts, id LIMIT {limit}"))?;
            let v = st.query_map([], event_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
            v
        };
        for e in &rows {
            c.execute("UPDATE events SET consumed=1 WHERE id=?", [&e.id])?;
        }
        Ok(rows)
    }

    // ---------- agents ----------
    pub fn create_agent(&self, n: NewAgent) -> Result<Agent> {
        let name = n.name.trim();
        if name.is_empty() {
            return Err(Error::invalid("у специалиста должно быть имя"));
        }
        let c = self.lock();
        let base = slugify(name);
        let mut slug = base.clone();
        let mut i = 2;
        while c.query_row("SELECT 1 FROM agents WHERE slug=?", [&slug], |_| Ok(())).optional()?.is_some() {
            slug = format!("{base}-{i}");
            i += 1;
        }
        let id = new_id();
        let now = now_ms();
        let accent = if n.accent.is_empty() { "#8e8e93".to_string() } else { n.accent };
        let role = if n.role.is_empty() { "custom".to_string() } else { n.role };
        c.execute(
            "INSERT INTO agents(id,slug,name,role,profile,accent,model_pref,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?)",
            params![id, slug, name, role, n.profile, accent, n.model_pref, now, now],
        )?;
        c.execute(
            "INSERT INTO agent_profile_versions(id,agent_id,profile,note,created_at) VALUES (?,?,?,?,?)",
            params![new_id(), id, n.profile, "создание", now],
        )?;
        emit_c(&c, "agent.created", "agent", &id, "user", serde_json::json!({"name": name}))?;
        let a = c.query_row(&format!("SELECT {AGENT_COLS} FROM agents WHERE id=?"), [&id], agent_from)?;
        Ok(a)
    }

    pub fn get_agent(&self, id_or_slug: &str) -> Result<Agent> {
        self.lock()
            .query_row(
                &format!("SELECT {AGENT_COLS} FROM agents WHERE id=?1 OR slug=?1"),
                [id_or_slug],
                agent_from,
            )
            .optional()?
            .ok_or_else(|| Error::not_found(format!("специалист {id_or_slug}")))
    }

    pub fn list_agents(&self, include_retired: bool) -> Result<Vec<Agent>> {
        let c = self.lock();
        let sql = format!(
            "SELECT {AGENT_COLS} FROM agents {} ORDER BY created_at",
            if include_retired { "" } else { "WHERE status='active'" }
        );
        let mut st = c.prepare(&sql)?;
        let v = st.query_map([], agent_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    /// Significant profile changes are recorded explicitly (Senate.md §5).
    pub fn update_profile(&self, agent_id: &str, profile: &str, note: &str, actor: &str) -> Result<Agent> {
        let c = self.lock();
        let now = now_ms();
        let n = c.execute("UPDATE agents SET profile=?, updated_at=? WHERE id=?", params![profile, now, agent_id])?;
        if n == 0 {
            return Err(Error::not_found(format!("специалист {agent_id}")));
        }
        c.execute(
            "INSERT INTO agent_profile_versions(id,agent_id,profile,note,created_at) VALUES (?,?,?,?,?)",
            params![new_id(), agent_id, profile, note, now],
        )?;
        emit_c(&c, "agent.profile_changed", "agent", agent_id, actor, serde_json::json!({"note": note}))?;
        Ok(c.query_row(&format!("SELECT {AGENT_COLS} FROM agents WHERE id=?"), [agent_id], agent_from)?)
    }

    pub fn update_agent_basics(&self, agent_id: &str, name: Option<&str>, accent: Option<&str>, model_pref: Option<&str>) -> Result<Agent> {
        let c = self.lock();
        let now = now_ms();
        if let Some(v) = name {
            c.execute("UPDATE agents SET name=?, updated_at=? WHERE id=?", params![v, now, agent_id])?;
        }
        if let Some(v) = accent {
            c.execute("UPDATE agents SET accent=?, updated_at=? WHERE id=?", params![v, now, agent_id])?;
        }
        if let Some(v) = model_pref {
            c.execute("UPDATE agents SET model_pref=?, updated_at=? WHERE id=?", params![v, now, agent_id])?;
        }
        c.query_row(&format!("SELECT {AGENT_COLS} FROM agents WHERE id=?"), [agent_id], agent_from)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("специалист {agent_id}")))
    }

    /// The specialist's username (`@slug`), chosen by hand. Latin letters, digits, `-` and `_`, 2–32 long; unique.
    /// Returns the specialist and the username they had, so the caller can move what is stored under the old one.
    pub fn set_agent_slug(&self, agent_id: &str, wanted: &str) -> Result<(Agent, String)> {
        let slug = wanted.trim().trim_start_matches('@').to_lowercase();
        if slug.len() < 2 || slug.len() > 32 || !slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
            return Err(Error::invalid("username: 2–32 символа, латинские буквы, цифры, «-» и «_»"));
        }
        let c = self.lock();
        let old: String = c
            .query_row("SELECT slug FROM agents WHERE id=?", [agent_id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| Error::not_found(format!("специалист {agent_id}")))?;
        if old != slug {
            let taken: Option<String> = c.query_row("SELECT id FROM agents WHERE slug=? OR id=?", params![slug, slug], |r| r.get(0)).optional()?;
            if taken.is_some() {
                return Err(Error::invalid(format!("username @{slug} уже занят")));
            }
            c.execute("UPDATE agents SET slug=?, updated_at=? WHERE id=?", params![slug, now_ms(), agent_id])?;
        }
        let ag = c.query_row(&format!("SELECT {AGENT_COLS} FROM agents WHERE id=?"), [agent_id], agent_from)?;
        Ok((ag, old))
    }

    /// "m", "f" or "" (not set).
    pub fn set_agent_gender(&self, agent_id: &str, gender: &str) -> Result<Agent> {
        let g = match gender.trim() {
            "m" | "f" | "" => gender.trim(),
            _ => return Err(Error::invalid("пол: m, f или пусто")),
        };
        let c = self.lock();
        let n = c.execute("UPDATE agents SET gender=?, updated_at=? WHERE id=?", params![g, now_ms(), agent_id])?;
        if n == 0 {
            return Err(Error::not_found(format!("специалист {agent_id}")));
        }
        Ok(c.query_row(&format!("SELECT {AGENT_COLS} FROM agents WHERE id=?"), [agent_id], agent_from)?)
    }

    /// The specialist's picture: the address of an uploaded image, or "" to go back to the letter.
    pub fn set_agent_avatar(&self, agent_id: &str, avatar: &str) -> Result<Agent> {
        let c = self.lock();
        let n = c.execute("UPDATE agents SET avatar=?, updated_at=? WHERE id=?", params![avatar, now_ms(), agent_id])?;
        if n == 0 {
            return Err(Error::not_found(format!("специалист {agent_id}")));
        }
        Ok(c.query_row(&format!("SELECT {AGENT_COLS} FROM agents WHERE id=?"), [agent_id], agent_from)?)
    }

    /// Retiring is only allowed once open obligations were handed over or cancelled explicitly.
    pub fn retire_agent(&self, agent_id: &str) -> Result<()> {
        let c = self.lock();
        let open: i64 = c.query_row(
            "SELECT COUNT(*) FROM obligations WHERE owner_id=? AND state NOT IN ('done','cancelled')",
            [agent_id],
            |r| r.get(0),
        )?;
        if open > 0 {
            return Err(Error::invalid(format!(
                "у специалиста {open} незавершённых задач — передайте или отмените их"
            )));
        }
        c.execute("UPDATE agents SET status='retired', updated_at=? WHERE id=?", params![now_ms(), agent_id])?;
        emit_c(&c, "agent.retired", "agent", agent_id, "user", serde_json::json!({}))?;
        Ok(())
    }

    pub fn profile_versions(&self, agent_id: &str) -> Result<Vec<serde_json::Value>> {
        let c = self.lock();
        let mut st = c.prepare("SELECT id,profile,note,created_at FROM agent_profile_versions WHERE agent_id=? ORDER BY created_at DESC")?;
        let v = st
            .query_map([agent_id], |r| {
                Ok(serde_json::json!({"id": r.get::<_,String>(0)?, "profile": r.get::<_,String>(1)?, "note": r.get::<_,String>(2)?, "created_at": r.get::<_,i64>(3)?}))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    // ---------- grants ----------
    pub fn set_grant(&self, agent_id: Option<&str>, project_id: Option<&str>, action: &str, mode: GrantMode) -> Result<()> {
        if !crate::model::action::ALL.contains(&action) {
            return Err(Error::invalid(format!("неизвестное действие: {action}")));
        }
        let c = self.lock();
        c.execute(
            "DELETE FROM grants WHERE agent_id IS ?1 AND project_id IS ?2 AND action=?3",
            params![agent_id, project_id, action],
        )?;
        c.execute(
            "INSERT INTO grants(id,agent_id,project_id,action,mode) VALUES (?,?,?,?,?)",
            params![new_id(), agent_id, project_id, action, mode.as_str()],
        )?;
        emit_c(&c, "grant.changed", "grant", action, "user", serde_json::json!({"agent": agent_id, "project": project_id, "mode": mode.as_str()}))?;
        Ok(())
    }

    pub fn list_grants(&self) -> Result<Vec<Grant>> {
        let c = self.lock();
        let mut st = c.prepare("SELECT id,agent_id,project_id,action,mode FROM grants ORDER BY action")?;
        let v = st
            .query_map([], |r| {
                Ok(Grant {
                    id: r.get(0)?,
                    agent_id: r.get(1)?,
                    project_id: r.get(2)?,
                    action: r.get(3)?,
                    mode: en(r, 4, GrantMode::parse, GrantMode::Deny)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    /// Most specific grant wins: (agent, project) > (agent) > (project) > (global). Without any grant
    /// the built-in default applies (see `engine::default_mode`).
    pub fn resolve_mode(&self, agent_id: &str, project_id: Option<&str>, action: &str) -> Result<GrantMode> {
        let c = self.lock();
        let mode: Option<String> = c
            .query_row(
                "SELECT mode FROM grants
                 WHERE action=?1 AND (agent_id IS NULL OR agent_id=?2) AND (project_id IS NULL OR project_id=?3)
                 ORDER BY (agent_id IS NOT NULL)*2 + (project_id IS NOT NULL) DESC LIMIT 1",
                params![action, agent_id, project_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(mode
            .and_then(|m| GrantMode::parse(&m))
            .unwrap_or_else(|| crate::engine::default_mode(action)))
    }

    // ---------- projects ----------
    pub fn create_project(&self, n: NewProject) -> Result<Project> {
        if n.name.trim().is_empty() {
            return Err(Error::invalid("у проекта должно быть название"));
        }
        let c = self.lock();
        let id = new_id();
        let now = now_ms();
        c.execute(
            "INSERT INTO projects(id,name,goal,criteria,constraints,root_path,node_id,company_id,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?)",
            params![id, n.name.trim(), n.goal, n.criteria, n.constraints, n.root_path.trim(), n.node_id, n.company_id.filter(|c| !c.is_empty()), now, now],
        )?;
        for m in &n.members {
            c.execute("INSERT OR IGNORE INTO project_members(project_id,agent_id) VALUES (?,?)", params![id, m])?;
        }
        emit_c(&c, "project.created", "project", &id, "user", serde_json::json!({"name": n.name}))?;
        Ok(c.query_row(&format!("SELECT {PROJECT_COLS} FROM projects WHERE id=?"), [&id], project_from)?)
    }

    pub fn get_project(&self, id: &str) -> Result<Project> {
        self.lock()
            .query_row(&format!("SELECT {PROJECT_COLS} FROM projects WHERE id=?"), [id], project_from)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("проект {id}")))
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let c = self.lock();
        let mut st = c.prepare(&format!("SELECT {PROJECT_COLS} FROM projects WHERE status!='archived' ORDER BY updated_at DESC"))?;
        let v = st.query_map([], project_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn update_project(&self, id: &str, p: ProjectPatch) -> Result<Project> {
        let c = self.lock();
        let now = now_ms();
        let mut changed = vec![];
        for (col, val) in [
            ("name", &p.name),
            ("goal", &p.goal),
            ("criteria", &p.criteria),
            ("constraints", &p.constraints),
            ("status", &p.status),
            ("root_path", &p.root_path),
            ("mode", &p.mode),
        ] {
            if let Some(v) = val {
                c.execute(&format!("UPDATE projects SET {col}=?, updated_at=? WHERE id=?"), params![v, now, id])?;
                changed.push(col);
            }
        }
        if let Some(cid) = &p.company_id {
            c.execute("UPDATE projects SET company_id=?, updated_at=? WHERE id=?", params![if cid.is_empty() { None } else { Some(cid) }, now, id])?;
            emit_c(&c, "project.company_changed", "project", id, "user", serde_json::json!({"company": cid}))?;
        }
        if changed.iter().any(|c| ["goal", "criteria", "constraints"].contains(c)) {
            // A changed requirement must reach whoever works on it (Senate.md section 8).
            emit_c(&c, "project.requirements_changed", "project", id, "user", serde_json::json!({"fields": changed}))?;
        }
        c.query_row(&format!("SELECT {PROJECT_COLS} FROM projects WHERE id=?"), [id], project_from)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("проект {id}")))
    }

    pub fn project_members(&self, project_id: &str) -> Result<Vec<Agent>> {
        let c = self.lock();
        let mut st = c.prepare(&format!(
            "SELECT {} FROM agents a JOIN project_members p ON p.agent_id=a.id WHERE p.project_id=? AND a.status='active' ORDER BY a.created_at",
            AGENT_COLS.split(',').map(|c| format!("a.{c}")).collect::<Vec<_>>().join(",")
        ))?;
        let v = st.query_map([project_id], agent_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn set_project_members(&self, project_id: &str, agent_ids: &[String]) -> Result<()> {
        let c = self.lock();
        c.execute("DELETE FROM project_members WHERE project_id=?", [project_id])?;
        for a in agent_ids {
            c.execute("INSERT OR IGNORE INTO project_members(project_id,agent_id) VALUES (?,?)", params![project_id, a])?;
        }
        Ok(())
    }

    pub fn agent_projects(&self, agent_id: &str) -> Result<Vec<Project>> {
        let c = self.lock();
        let mut st = c.prepare(&format!(
            "SELECT {} FROM projects p JOIN project_members m ON m.project_id=p.id WHERE m.agent_id=? AND p.status!='archived' ORDER BY p.updated_at DESC",
            PROJECT_COLS.split(',').map(|c| format!("p.{c}")).collect::<Vec<_>>().join(",")
        ))?;
        let v = st.query_map([agent_id], project_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    // ---------- decisions ----------
    pub fn propose_decision(&self, project_id: &str, title: &str, body: &str, by: &str, supersedes: Option<&str>) -> Result<Decision> {
        self.propose_decision_in(Some(project_id), None, title, body, by, supersedes)
    }

    /// A decision belongs to one project or to a whole company (shared by all its projects).
    pub fn propose_decision_in(&self, project_id: Option<&str>, company_id: Option<&str>, title: &str, body: &str, by: &str, supersedes: Option<&str>) -> Result<Decision> {
        if title.trim().is_empty() {
            return Err(Error::invalid("у решения должна быть формулировка"));
        }
        if project_id.is_none() && company_id.is_none() {
            return Err(Error::invalid("решение принимается в проекте или в компании"));
        }
        let c = self.lock();
        let id = new_id();
        let now = now_ms();
        c.execute(
            "INSERT INTO decisions(id,project_id,company_id,title,body,decided_by,supersedes_id,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?)",
            params![id, project_id, company_id, title.trim(), body, by, supersedes, now, now],
        )?;
        emit_c(&c, "decision.proposed", "decision", &id, by, serde_json::json!({"project": project_id, "company": company_id, "title": title}))?;
        Ok(c.query_row(&format!("SELECT {DECISION_COLS} FROM decisions WHERE id=?"), [&id], decision_from)?)
    }

    /// Status change is recorded separately from the discussion itself (Senate.md §8).
    pub fn set_decision_status(&self, id: &str, status: DecisionStatus, by: &str) -> Result<Decision> {
        let c = self.lock();
        let cur = c
            .query_row(&format!("SELECT {DECISION_COLS} FROM decisions WHERE id=?"), [id], decision_from)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("решение {id}")))?;
        let now = now_ms();
        c.execute("UPDATE decisions SET status=?, decided_by=?, updated_at=? WHERE id=?", params![status.as_str(), by, now, id])?;
        if status == DecisionStatus::Accepted {
            if let Some(old) = &cur.supersedes_id {
                c.execute("UPDATE decisions SET status='superseded', updated_at=? WHERE id=?", params![now, old])?;
                emit_c(&c, "decision.superseded", "decision", old, by, serde_json::json!({"by": id}))?;
            }
        }
        emit_c(&c, &format!("decision.{}", status.as_str()), "decision", id, by, serde_json::json!({"project": cur.project_id, "title": cur.title}))?;
        Ok(c.query_row(&format!("SELECT {DECISION_COLS} FROM decisions WHERE id=?"), [id], decision_from)?)
    }

    pub fn list_company_decisions(&self, company_id: &str) -> Result<Vec<Decision>> {
        let c = self.lock();
        let mut st = c.prepare(&format!("SELECT {DECISION_COLS} FROM decisions WHERE company_id=? ORDER BY created_at DESC"))?;
        let v = st.query_map([company_id], decision_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn list_decisions(&self, project_id: &str) -> Result<Vec<Decision>> {
        let c = self.lock();
        let mut st = c.prepare(&format!("SELECT {DECISION_COLS} FROM decisions WHERE project_id=? ORDER BY created_at DESC"))?;
        let v = st.query_map([project_id], decision_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    // ---------- obligations (raw access; rules live in engine.rs) ----------
    pub fn create_obligation(&self, n: NewObligation) -> Result<Obligation> {
        if n.title.trim().is_empty() {
            return Err(Error::invalid("у задача должно быть название"));
        }
        let c = self.lock();
        let id = new_id();
        let now = now_ms();
        let by = if n.requested_by.is_empty() { "user".to_string() } else { n.requested_by.clone() };
        c.execute(
            "INSERT INTO obligations(id,title,result,criteria,owner_id,project_id,parent_id,requested_by,next_step,due_at,budget_tokens,created_at,updated_at)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![id, n.title.trim(), n.result, n.criteria, n.owner_id, n.project_id, n.parent_id, by, n.next_step, n.due_at, n.budget_tokens, now, now],
        )?;
        emit_c(&c, "obligation.created", "obligation", &id, &by, serde_json::json!({"title": n.title, "owner": n.owner_id, "project": n.project_id}))?;
        Ok(c.query_row(&format!("SELECT {OBL_COLS} FROM obligations WHERE id=?"), [&id], obligation_from)?)
    }

    pub fn get_obligation(&self, id: &str) -> Result<Obligation> {
        self.lock()
            .query_row(&format!("SELECT {OBL_COLS} FROM obligations WHERE id=?"), [id], obligation_from)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("задачо {id}")))
    }

    pub fn list_obligations(&self, owner: Option<&str>, project: Option<&str>, open_only: bool) -> Result<Vec<Obligation>> {
        let c = self.lock();
        let mut sql = format!("SELECT {OBL_COLS} FROM obligations WHERE 1=1");
        let mut args: Vec<String> = vec![];
        if let Some(o) = owner {
            sql += " AND owner_id=?";
            args.push(o.into());
        }
        if let Some(p) = project {
            sql += " AND project_id=?";
            args.push(p.into());
        }
        if open_only {
            sql += " AND state NOT IN ('done','cancelled')";
        }
        sql += " ORDER BY CASE state WHEN 'blocked' THEN 0 WHEN 'review' THEN 1 WHEN 'running' THEN 2 WHEN 'accepted' THEN 3 WHEN 'waiting' THEN 4 ELSE 5 END, updated_at DESC";
        let mut st = c.prepare(&sql)?;
        let v = st.query_map(params_from_iter(args.iter()), obligation_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn update_obligation_fields(&self, id: &str, title: Option<&str>, result: Option<&str>, criteria: Option<&str>, next_step: Option<&str>, due_at: Option<Option<i64>>, actor: &str) -> Result<Obligation> {
        let c = self.lock();
        let now = now_ms();
        for (col, val) in [("title", title), ("result", result), ("criteria", criteria), ("next_step", next_step)] {
            if let Some(v) = val {
                c.execute(&format!("UPDATE obligations SET {col}=?, updated_at=? WHERE id=?"), params![v, now, id])?;
            }
        }
        if let Some(d) = due_at {
            c.execute("UPDATE obligations SET due_at=?, updated_at=? WHERE id=?", params![d, now, id])?;
        }
        emit_c(&c, "obligation.edited", "obligation", id, actor, serde_json::json!({}))?;
        c.query_row(&format!("SELECT {OBL_COLS} FROM obligations WHERE id=?"), [id], obligation_from)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("задачо {id}")))
    }

    /// Hand the responsibility to another specialist; the previous owner is kept in the journal.
    pub fn reassign_obligation(&self, id: &str, new_owner: &str, actor: &str) -> Result<Obligation> {
        let c = self.lock();
        let cur = c
            .query_row(&format!("SELECT {OBL_COLS} FROM obligations WHERE id=?"), [id], obligation_from)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("задачо {id}")))?;
        if cur.state.is_terminal() {
            return Err(Error::Transition("завершённую задачу нельзя передать".into()));
        }
        c.execute("UPDATE obligations SET owner_id=?, updated_at=? WHERE id=?", params![new_owner, now_ms(), id])?;
        emit_c(&c, "obligation.reassigned", "obligation", id, actor, serde_json::json!({"from": cur.owner_id, "to": new_owner}))?;
        Ok(c.query_row(&format!("SELECT {OBL_COLS} FROM obligations WHERE id=?"), [id], obligation_from)?)
    }

    pub(crate) fn write_obligation_state(
        &self,
        id: &str,
        state: ObligationState,
        wait: Option<&WaitCondition>,
        blocker: &str,
        outcome: Option<&str>,
        next_step: Option<&str>,
        actor: &str,
        note: &str,
    ) -> Result<Obligation> {
        let c = self.lock();
        let prev: String = c
            .query_row("SELECT state FROM obligations WHERE id=?", [id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| Error::not_found(format!("задачо {id}")))?;
        let wait_json = wait.map(|w| serde_json::to_string(w).unwrap_or_default());
        c.execute(
            "UPDATE obligations SET state=?, wait_condition=?, blocker=?, updated_at=? WHERE id=?",
            params![state.as_str(), wait_json, blocker, now_ms(), id],
        )?;
        if let Some(o) = outcome {
            c.execute("UPDATE obligations SET outcome=? WHERE id=?", params![o, id])?;
        }
        if let Some(n) = next_step {
            c.execute("UPDATE obligations SET next_step=? WHERE id=?", params![n, id])?;
        }
        emit_c(
            &c,
            &format!("obligation.{}", state.as_str()),
            "obligation",
            id,
            actor,
            serde_json::json!({"from": prev, "to": state.as_str(), "note": note}),
        )?;
        Ok(c.query_row(&format!("SELECT {OBL_COLS} FROM obligations WHERE id=?"), [id], obligation_from)?)
    }

    pub fn add_spent_tokens(&self, obligation_id: &str, tokens: i64) -> Result<()> {
        self.lock().execute("UPDATE obligations SET spent_tokens=spent_tokens+? WHERE id=?", params![tokens, obligation_id])?;
        Ok(())
    }

    // ---------- threads & messages ----------
    pub fn lounge(&self) -> Result<Thread> {
        let c = self.lock();
        if let Some(t) = c.query_row(&format!("SELECT {THREAD_COLS} FROM threads WHERE kind='lounge'"), [], thread_from).optional()? {
            return Ok(t);
        }
        let id = new_id();
        let now = now_ms();
        c.execute(
            "INSERT INTO threads(id,kind,title,auto_title,created_at,updated_at) VALUES (?, 'lounge', 'Общее обсуждение', 0, ?, ?)",
            params![id, now, now],
        )?;
        Ok(c.query_row(&format!("SELECT {THREAD_COLS} FROM threads WHERE id=?"), [&id], thread_from)?)
    }

    pub fn create_thread(&self, n: NewThread) -> Result<Thread> {
        let c = self.lock();
        let id = new_id();
        let now = now_ms();
        let auto = n.title.trim().is_empty();
        c.execute(
            "INSERT INTO threads(id,kind,project_id,company_id,agent_id,title,auto_title,created_at,updated_at) VALUES (?, 'chat', ?, ?, ?, ?, ?, ?, ?)",
            params![id, n.project_id, n.company_id.filter(|c| !c.is_empty()), n.agent_id, n.title.trim(), auto as i64, now, now],
        )?;
        Ok(c.query_row(&format!("SELECT {THREAD_COLS} FROM threads WHERE id=?"), [&id], thread_from)?)
    }

    pub fn get_thread(&self, id: &str) -> Result<Thread> {
        self.lock()
            .query_row(&format!("SELECT {THREAD_COLS} FROM threads WHERE id=?"), [id], thread_from)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("чат {id}")))
    }

    pub fn list_threads(&self, project_id: Option<&str>, archived: bool) -> Result<Vec<Thread>> {
        let c = self.lock();
        let mut st = c.prepare(&format!(
            "SELECT {THREAD_COLS} FROM threads WHERE archived=?1 AND (?2 IS NULL OR project_id=?2)
             ORDER BY (kind='lounge') DESC, pinned DESC, updated_at DESC"
        ))?;
        let v = st.query_map(params![archived as i64, project_id], thread_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn patch_thread(&self, id: &str, p: ThreadPatch) -> Result<Thread> {
        let c = self.lock();
        let now = now_ms();
        if let Some(v) = &p.title {
            c.execute("UPDATE threads SET title=?, auto_title=0, updated_at=? WHERE id=?", params![v.trim(), now, id])?;
        }
        for (col, val) in [("model", &p.model), ("effort", &p.effort), ("mode", &p.mode)] {
            if let Some(v) = val {
                c.execute(&format!("UPDATE threads SET {col}=? WHERE id=?"), params![v, id])?;
            }
        }
        if let Some(v) = p.archived {
            c.execute("UPDATE threads SET archived=? WHERE id=?", params![v as i64, id])?;
        }
        if let Some(v) = p.pinned {
            c.execute("UPDATE threads SET pinned=? WHERE id=?", params![v as i64, id])?;
        }
        if let Some(v) = &p.agent_id {
            c.execute("UPDATE threads SET agent_id=? WHERE id=?", params![if v.is_empty() { None } else { Some(v) }, id])?;
        }
        // A chat is filed under a company or a project; an empty string takes it out again.
        if let Some(v) = &p.company_id {
            if !v.is_empty() && c.query_row("SELECT 1 FROM companies WHERE id=?", [v], |_| Ok(())).optional()?.is_none() {
                return Err(Error::not_found(format!("компания {v}")));
            }
            c.execute("UPDATE threads SET company_id=? WHERE id=?", params![if v.is_empty() { None } else { Some(v) }, id])?;
        }
        if let Some(v) = &p.project_id {
            if !v.is_empty() && c.query_row("SELECT 1 FROM projects WHERE id=?", [v], |_| Ok(())).optional()?.is_none() {
                return Err(Error::not_found(format!("проект {v}")));
            }
            c.execute("UPDATE threads SET project_id=? WHERE id=?", params![if v.is_empty() { None } else { Some(v) }, id])?;
        }
        c.query_row(&format!("SELECT {THREAD_COLS} FROM threads WHERE id=?"), [id], thread_from)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("чат {id}")))
    }

    /// Give a chat a title from its first message, unless the user named it.
    pub fn auto_title(&self, id: &str, text: &str) -> Result<()> {
        let title: String = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(60).collect();
        if title.is_empty() {
            return Ok(());
        }
        self.lock().execute("UPDATE threads SET title=? WHERE id=? AND auto_title=1 AND title=''", params![title, id])?;
        Ok(())
    }

    pub fn thread_cli_session(&self, id: &str, backend: &str) -> Result<Option<String>> {
        let t = self.get_thread(id)?;
        Ok(t.cli_sessions.get(backend).and_then(|v| v.as_str()).map(String::from))
    }

    /// The CLI session a specialist continues in this chat. One specialist, one session: two specialists never
    /// share one (a CLI lets a single process write to a session, and their identities and histories would mix).
    /// Chats from before kept one session per provider: it stays with the specialist the chat is pinned to.
    pub fn cli_session_for(&self, thread: &Thread, provider: &str, agent_id: &str) -> Option<String> {
        let get = |k: &str| thread.cli_sessions.get(k).and_then(|v| v.as_str()).map(String::from);
        get(&format!("{provider}:{agent_id}")).or_else(|| if thread.agent_id.as_deref() == Some(agent_id) { get(provider) } else { None })
    }

    /// Forget the CLI session a specialist continues in this chat: the next turn starts a fresh one.
    pub fn clear_cli_session_for(&self, thread_id: &str, provider: &str, agent_id: &str) -> Result<()> {
        let t = self.get_thread(thread_id)?;
        let mut m = t.cli_sessions.as_object().cloned().unwrap_or_default();
        m.remove(&format!("{provider}:{agent_id}"));
        if t.agent_id.as_deref() == Some(agent_id) {
            m.remove(provider);
        }
        self.lock().execute("UPDATE threads SET cli_sessions=? WHERE id=?", params![serde_json::Value::Object(m).to_string(), thread_id])?;
        Ok(())
    }

    /// The summary of the earlier part of a chat that a specialist keeps after `/compact`.
    pub fn compact_summary(thread: &Thread, agent_id: &str) -> Option<String> {
        thread.meta["compact"][agent_id]["summary"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
    }

    /// Keep a specialist's summary of this chat (replacing the one before it).
    pub fn set_compact_summary(&self, thread_id: &str, agent_id: &str, summary: &str, messages: usize) -> Result<()> {
        let t = self.get_thread(thread_id)?;
        let mut all = t.meta.get("compact").and_then(|v| v.as_object().cloned()).unwrap_or_default();
        all.insert(agent_id.into(), serde_json::json!({"summary": summary, "at": now_ms(), "messages": messages}));
        self.set_forum_meta(thread_id, &serde_json::json!({"compact": serde_json::Value::Object(all)}))?;
        Ok(())
    }

    pub fn set_cli_session_for(&self, thread_id: &str, provider: &str, agent_id: &str, session: &str) -> Result<()> {
        self.set_thread_cli_session(thread_id, &format!("{provider}:{agent_id}"), session)
    }

    pub fn set_thread_cli_session(&self, id: &str, backend: &str, session: &str) -> Result<()> {
        let t = self.get_thread(id)?;
        let mut m = t.cli_sessions.as_object().cloned().unwrap_or_default();
        m.insert(backend.into(), serde_json::Value::String(session.into()));
        self.lock().execute("UPDATE threads SET cli_sessions=? WHERE id=?", params![serde_json::Value::Object(m).to_string(), id])?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn post_message(
        &self,
        thread_id: &str,
        author_kind: &str,
        author_id: Option<&str>,
        body: &str,
        props: serde_json::Value,
        status: MessageStatus,
        session_id: Option<&str>,
    ) -> Result<Message> {
        if body.trim().is_empty() && author_kind == "user" && props.get("attachments").is_none() {
            return Err(Error::invalid("пустое сообщение"));
        }
        let c = self.lock();
        let id = new_id();
        let now = now_ms();
        c.execute(
            "INSERT INTO messages(id,thread_id,author_kind,author_id,body,props,status,session_id,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?)",
            params![id, thread_id, author_kind, author_id, body, props.to_string(), status.as_str(), session_id, now, now],
        )?;
        c.execute("UPDATE threads SET updated_at=? WHERE id=?", params![now, thread_id])?;
        emit_c(&c, "message.posted", "thread", thread_id, author_id.unwrap_or(author_kind), serde_json::json!({"message": id, "author_kind": author_kind}))?;
        Ok(c.query_row(&format!("SELECT {MESSAGE_COLS} FROM messages WHERE id=?"), [&id], message_from)?)
    }

    pub fn get_message(&self, id: &str) -> Result<Message> {
        self.lock()
            .query_row(&format!("SELECT {MESSAGE_COLS} FROM messages WHERE id=?"), [id], message_from)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("сообщение {id}")))
    }

    /// Streaming update of an agent message. `props_merge` is shallow-merged into existing props
    /// (a null value removes the key).
    pub fn update_message(&self, id: &str, body: Option<&str>, props_merge: Option<&serde_json::Value>, status: Option<MessageStatus>) -> Result<Message> {
        let c = self.lock();
        let now = now_ms();
        if let Some(b) = body {
            c.execute("UPDATE messages SET body=?, updated_at=? WHERE id=?", params![b, now, id])?;
        }
        if let Some(patch) = props_merge {
            let cur: String = c
                .query_row("SELECT props FROM messages WHERE id=?", [id], |r| r.get(0))
                .optional()?
                .ok_or_else(|| Error::not_found(format!("сообщение {id}")))?;
            let mut v: serde_json::Value = json_or(&cur, serde_json::json!({}));
            if let (Some(dst), Some(src)) = (v.as_object_mut(), patch.as_object()) {
                for (k, val) in src {
                    if val.is_null() {
                        dst.remove(k);
                    } else {
                        dst.insert(k.clone(), val.clone());
                    }
                }
            }
            c.execute("UPDATE messages SET props=?, updated_at=? WHERE id=?", params![v.to_string(), now, id])?;
        }
        if let Some(st) = status {
            c.execute("UPDATE messages SET status=?, updated_at=? WHERE id=?", params![st.as_str(), now, id])?;
        }
        Ok(c.query_row(&format!("SELECT {MESSAGE_COLS} FROM messages WHERE id=?"), [id], message_from)?)
    }

    /// Newest `limit` messages (optionally those before a given message), oldest first.
    pub fn list_messages(&self, thread_id: &str, before: Option<&str>, limit: i64) -> Result<Vec<Message>> {
        let c = self.lock();
        let mut st = c.prepare(&format!(
            "SELECT {MESSAGE_COLS} FROM (
               SELECT * FROM messages WHERE thread_id=?1
                 AND (?2 IS NULL OR created_at < (SELECT created_at FROM messages WHERE id=?2))
               ORDER BY created_at DESC, id DESC LIMIT ?3
             ) AS messages ORDER BY created_at, id"
        ))?;
        let v = st.query_map(params![thread_id, before, limit], message_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn thread_last_message(&self, thread_id: &str) -> Result<Option<Message>> {
        Ok(self
            .lock()
            .query_row(
                &format!("SELECT {MESSAGE_COLS} FROM messages WHERE thread_id=? ORDER BY created_at DESC, id DESC LIMIT 1"),
                [thread_id],
                message_from,
            )
            .optional()?)
    }

    // ---------- memory ----------
    pub fn add_memory(&self, n: NewMemory) -> Result<Memory> {
        if n.body.trim().is_empty() {
            return Err(Error::invalid("пустая запись памяти"));
        }
        match n.level {
            MemoryLevel::Personal if n.agent_id.is_none() => return Err(Error::invalid("личная память требует специалиста")),
            MemoryLevel::Project if n.project_id.is_none() => return Err(Error::invalid("проектная память требует проект")),
            MemoryLevel::Company if n.company_id.is_none() => return Err(Error::invalid("память компании требует компанию")),
            _ => {}
        }
        if n.kind == MemoryKind::Verified && n.verification.trim().is_empty() {
            return Err(Error::invalid("проверенное знание требует основание проверки (уверенность модели — не основание)"));
        }
        let c = self.lock();
        let id = new_id();
        let now = now_ms();
        c.execute(
            "INSERT INTO memory(id,level,agent_id,project_id,company_id,kind,body,source,scope_note,verification,fresh_until,created_at,updated_at)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![id, n.level.as_str(), n.agent_id, n.project_id, n.company_id, n.kind.as_str(), n.body.trim(), n.source, n.scope_note, n.verification, n.fresh_until, now, now],
        )?;
        emit_c(&c, "memory.added", "memory", &id, n.agent_id.as_deref().unwrap_or("user"), serde_json::json!({"level": n.level.as_str(), "kind": n.kind.as_str()}))?;
        Ok(self.memory_by_id_c(&c, &id)?)
    }

    fn memory_by_id_c(&self, c: &Connection, id: &str) -> rusqlite::Result<Memory> {
        c.query_row(&format!("SELECT {MEMORY_COLS} FROM memory m WHERE m.id=?"), [id], memory_from)
    }

    pub fn get_memory(&self, id: &str) -> Result<Memory> {
        self.memory_by_id_c(&self.lock(), id).optional()?.ok_or_else(|| Error::not_found(format!("запись памяти {id}")))
    }

    /// Correct an entry: the old one stops being used as current knowledge and is linked to the new
    /// one; open work in the same project receives a reason to re-check (Senate.md §15).
    pub fn supersede_memory(&self, old_id: &str, new_body: &str, source: &str, actor: &str) -> Result<Memory> {
        let c = self.lock();
        let old = self.memory_by_id_c(&c, old_id).optional()?.ok_or_else(|| Error::not_found(format!("запись памяти {old_id}")))?;
        if old.status != MemoryStatus::Active {
            return Err(Error::invalid("запись уже заменена или архивирована"));
        }
        let id = new_id();
        let now = now_ms();
        let kind = if old.kind == MemoryKind::Verified { MemoryKind::Hypothesis } else { old.kind };
        c.execute(
            "INSERT INTO memory(id,level,agent_id,project_id,company_id,kind,body,source,scope_note,supersedes_id,created_at,updated_at)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
            params![id, old.level.as_str(), old.agent_id, old.project_id, old.company_id, kind.as_str(), new_body.trim(), source, old.scope_note, old_id, now, now],
        )?;
        c.execute("UPDATE memory SET status='superseded', updated_at=? WHERE id=?", params![now, old_id])?;
        emit_c(&c, "memory.corrected", "memory", old_id, actor, serde_json::json!({"replaced_by": id, "project": old.project_id}))?;
        Ok(self.memory_by_id_c(&c, &id)?)
    }

    /// Observation → hypothesis → verified. Verification needs a stated basis.
    pub fn verify_memory(&self, id: &str, basis: &str, actor: &str) -> Result<Memory> {
        if basis.trim().is_empty() {
            return Err(Error::invalid("укажите основание проверки: данные, воспроизводимый результат, независимая оценка или решение человека"));
        }
        let c = self.lock();
        let n = c.execute(
            "UPDATE memory SET kind='verified', verification=?, updated_at=? WHERE id=? AND status='active'",
            params![basis.trim(), now_ms(), id],
        )?;
        if n == 0 {
            return Err(Error::not_found(format!("активная запись памяти {id}")));
        }
        emit_c(&c, "memory.verified", "memory", id, actor, serde_json::json!({"basis": basis}))?;
        Ok(self.memory_by_id_c(&c, id)?)
    }

    /// Collective memory fills selectively: only verified, sourced knowledge gets promoted.
    pub fn promote_memory(&self, id: &str, actor: &str) -> Result<Memory> {
        let c = self.lock();
        let m = self.memory_by_id_c(&c, id).optional()?.ok_or_else(|| Error::not_found(format!("запись памяти {id}")))?;
        if m.kind != MemoryKind::Verified {
            return Err(Error::invalid("в коллективную память переходит только проверенное знание"));
        }
        if m.source.trim().is_empty() {
            return Err(Error::invalid("у знания должен быть источник"));
        }
        c.execute("UPDATE memory SET level='collective', updated_at=? WHERE id=?", params![now_ms(), id])?;
        emit_c(&c, "memory.promoted", "memory", id, actor, serde_json::json!({"from": m.level.as_str()}))?;
        Ok(self.memory_by_id_c(&c, id)?)
    }

    pub fn archive_memory(&self, id: &str, actor: &str) -> Result<()> {
        let c = self.lock();
        c.execute("UPDATE memory SET status='archived', updated_at=? WHERE id=?", params![now_ms(), id])?;
        emit_c(&c, "memory.archived", "memory", id, actor, serde_json::json!({}))?;
        Ok(())
    }

    /// Entries visible to this specialist in this project, most relevant first. Superseded and
    /// archived entries are never returned. Without a query: verified first, then newest.
    pub fn search_memory(&self, query: &str, agent_id: Option<&str>, project_id: Option<&str>, limit: i64) -> Result<Vec<Memory>> {
        self.search_memory_in(query, agent_id, project_id, None, limit)
    }

    pub fn search_memory_in(&self, query: &str, agent_id: Option<&str>, project_id: Option<&str>, company_hint: Option<&str>, limit: i64) -> Result<Vec<Memory>> {
        let c = self.lock();
        // Visible: shared knowledge; this project's; this company's; the specialist's own general experience;
        // and the specialist's own experience *in this company* (never in another one).
        let company: Option<String> = match project_id {
            Some(p) => c.query_row("SELECT company_id FROM projects WHERE id=?", [p], |r| r.get(0)).optional()?.flatten(),
            None => None,
        };
        let company = company.or(company_hint.map(String::from));
        let scope = "m.status='active' AND (m.level='collective' OR (m.level='project' AND m.project_id IS ?2) OR (m.level='company' AND m.company_id IS ?6) OR (m.level='personal' AND m.agent_id IS ?1 AND (m.company_id IS NULL OR m.company_id IS ?6)))";
        let now = now_ms();
        let mut out: Vec<Memory> = vec![];
        if let Some(q) = fts_query(query) {
            let sql = format!(
                "SELECT {MEMORY_COLS} FROM memory_fts f JOIN memory m ON m.rowid=f.rowid
                 WHERE memory_fts MATCH ?3 AND {scope} AND (m.fresh_until IS NULL OR m.fresh_until>?4)
                 ORDER BY bm25(memory_fts), (m.kind='verified') DESC LIMIT ?5"
            );
            let mut st = c.prepare(&sql)?;
            out = st.query_map(params![agent_id, project_id, q, now, limit, company], memory_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        }
        if (out.len() as i64) < limit {
            let sql = format!(
                "SELECT {MEMORY_COLS} FROM memory m WHERE {scope} AND (m.fresh_until IS NULL OR m.fresh_until>?3)
                 ORDER BY (m.kind='verified') DESC, (m.level='project') DESC, (m.level='company') DESC, m.updated_at DESC LIMIT ?4"
            );
            let mut st = c.prepare(&sql)?;
            let more = st.query_map(params![agent_id, project_id, now, limit, 0i64, company], memory_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
            for m in more {
                if (out.len() as i64) >= limit {
                    break;
                }
                if !out.iter().any(|x| x.id == m.id) {
                    out.push(m);
                }
            }
        }
        Ok(out)
    }

    pub fn list_memory_in(&self, level: Option<MemoryLevel>, agent_id: Option<&str>, project_id: Option<&str>, company_id: Option<&str>, include_inactive: bool, limit: i64) -> Result<Vec<Memory>> {
        let c = self.lock();
        let mut sql = format!("SELECT {MEMORY_COLS} FROM memory m WHERE 1=1");
        let mut args: Vec<String> = vec![];
        if !include_inactive {
            sql += " AND m.status='active'";
        }
        if let Some(l) = level {
            sql += " AND m.level=?";
            args.push(l.as_str().into());
        }
        if let Some(a) = agent_id {
            sql += " AND m.agent_id=?";
            args.push(a.into());
        }
        if let Some(p) = project_id {
            sql += " AND m.project_id=?";
            args.push(p.into());
        }
        if let Some(co) = company_id {
            sql += " AND m.company_id=?";
            args.push(co.into());
        }
        sql += &format!(" ORDER BY m.updated_at DESC LIMIT {limit}");
        let mut st = c.prepare(&sql)?;
        let v = st.query_map(params_from_iter(args.iter()), memory_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    // ---------- MCP servers ----------
    fn mcp_from(c: &Connection, r: &Row) -> rusqlite::Result<McpServer> {
        let id: String = r.get(0)?;
        let args: String = r.get(4)?;
        let env: String = r.get(5)?;
        let headers: String = r.get(7)?;
        let agents: Vec<String> = {
            let mut st = c.prepare("SELECT agent_id FROM agent_mcp WHERE server_id=?")?;
            let v = st.query_map([&id], |x| x.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
            v
        };
        Ok(McpServer {
            id,
            name: r.get(1)?,
            kind: r.get(2)?,
            command: r.get(3)?,
            args: serde_json::from_str(&args).unwrap_or_default(),
            env: serde_json::from_str(&env).unwrap_or_default(),
            url: r.get(6)?,
            headers: serde_json::from_str(&headers).unwrap_or_default(),
            note: r.get(8)?,
            enabled: r.get::<_, i64>(9)? != 0,
            trusted: r.get::<_, i64>(10)? != 0,
            shared: r.get::<_, i64>(11)? != 0,
            agents,
            created_at: r.get(12)?,
            updated_at: r.get(13)?,
        })
    }

    pub fn list_mcp(&self) -> Result<Vec<McpServer>> {
        let c = self.lock();
        let mut st = c.prepare("SELECT id,name,kind,command,args,env,url,headers,note,enabled,trusted,shared,created_at,updated_at FROM mcp_servers ORDER BY name")?;
        let rows = st.query_map([], |r| Self::mcp_from(&c, r))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_mcp(&self, id: &str) -> Result<McpServer> {
        let c = self.lock();
        c.query_row(
            "SELECT id,name,kind,command,args,env,url,headers,note,enabled,trusted,shared,created_at,updated_at FROM mcp_servers WHERE id=?1 OR name=?1",
            [id],
            |r| Self::mcp_from(&c, r),
        )
        .optional()?
        .ok_or_else(|| Error::not_found(format!("MCP-сервер {id}")))
    }

    fn validate_mcp(m: &McpServer) -> Result<()> {
        let ok_name = m.name.len() >= 2
            && m.name.len() <= 31
            && m.name.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false)
            && m.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
        if !ok_name {
            return Err(Error::invalid("имя сервера: латиница в нижнем регистре, цифры, - и _, от 2 до 31 знака"));
        }
        if m.name == "senate" {
            return Err(Error::invalid("имя «senate» занято встроенными инструментами"));
        }
        match m.kind.as_str() {
            "stdio" if m.command.trim().is_empty() => Err(Error::invalid("укажите команду запуска")),
            "http" if !(m.url.starts_with("http://") || m.url.starts_with("https://")) => Err(Error::invalid("укажите адрес сервера (http:// или https://)")),
            "stdio" | "http" => Ok(()),
            _ => Err(Error::invalid("тип сервера: stdio или http")),
        }
    }

    pub fn save_mcp(&self, id: Option<&str>, i: McpInput) -> Result<McpServer> {
        let mut m = match id {
            Some(id) => self.get_mcp(id)?,
            None => McpServer { kind: "stdio".into(), enabled: true, ..Default::default() },
        };
        if let Some(v) = i.name {
            m.name = v.trim().to_string();
        }
        if let Some(v) = i.kind {
            m.kind = v;
        }
        if let Some(v) = i.command {
            m.command = v.trim().to_string();
        }
        if let Some(v) = i.args {
            m.args = v.into_iter().filter(|a| !a.is_empty()).collect();
        }
        if let Some(v) = i.url {
            m.url = v.trim().to_string();
        }
        if let Some(n) = i.env {
            for (k, v) in n {
                if !k.trim().is_empty() && !(v.is_empty() && m.env.contains_key(&k)) {
                    m.env.insert(k.trim().to_string(), v);
                }
            }
        }
        for k in i.env_remove.unwrap_or_default() {
            m.env.remove(&k);
        }
        if let Some(n) = i.headers {
            for (k, v) in n {
                if !k.trim().is_empty() && !(v.is_empty() && m.headers.contains_key(&k)) {
                    m.headers.insert(k.trim().to_string(), v);
                }
            }
        }
        for k in i.headers_remove.unwrap_or_default() {
            m.headers.remove(&k);
        }
        if let Some(v) = i.note {
            m.note = v;
        }
        if let Some(v) = i.enabled {
            m.enabled = v;
        }
        if let Some(v) = i.trusted {
            m.trusted = v;
        }
        if let Some(v) = i.shared {
            m.shared = v;
        }
        Self::validate_mcp(&m)?;
        let c = self.lock();
        let now = now_ms();
        let sid = match id {
            Some(_) => {
                c.execute(
                    "UPDATE mcp_servers SET name=?,kind=?,command=?,args=?,env=?,url=?,headers=?,note=?,enabled=?,trusted=?,shared=?,updated_at=? WHERE id=?",
                    params![m.name, m.kind, m.command, serde_json::to_string(&m.args)?, serde_json::to_string(&m.env)?, m.url, serde_json::to_string(&m.headers)?, m.note, m.enabled as i64, m.trusted as i64, m.shared as i64, now, m.id],
                )?;
                m.id.clone()
            }
            None => {
                let nid = new_id();
                c.execute(
                    "INSERT INTO mcp_servers(id,name,kind,command,args,env,url,headers,note,enabled,trusted,shared,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                    params![nid, m.name, m.kind, m.command, serde_json::to_string(&m.args)?, serde_json::to_string(&m.env)?, m.url, serde_json::to_string(&m.headers)?, m.note, m.enabled as i64, m.trusted as i64, m.shared as i64, now, now],
                )
                .map_err(|e| match e {
                    rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => Error::invalid("сервер с таким именем уже есть"),
                    other => Error::Db(other),
                })?;
                nid
            }
        };
        if let Some(agents) = i.agents {
            c.execute("DELETE FROM agent_mcp WHERE server_id=?", [&sid])?;
            for a in agents {
                c.execute("INSERT OR IGNORE INTO agent_mcp(agent_id,server_id) VALUES (?,?)", params![a, sid])?;
            }
        }
        emit_c(&c, if id.is_some() { "mcp.updated" } else { "mcp.added" }, "mcp", &sid, "user", serde_json::json!({"name": m.name}))?;
        drop(c);
        self.get_mcp(&sid)
    }

    pub fn delete_mcp(&self, id: &str) -> Result<()> {
        let c = self.lock();
        c.execute("DELETE FROM mcp_servers WHERE id=?", [id])?;
        emit_c(&c, "mcp.removed", "mcp", id, "user", serde_json::json!({}))?;
        Ok(())
    }

    /// Servers a given specialist actually gets: enabled ones that are shared or assigned to them.
    pub fn mcp_for_agent(&self, agent_id: &str) -> Result<Vec<McpServer>> {
        Ok(self.list_mcp()?.into_iter().filter(|m| m.enabled && (m.shared || m.agents.iter().any(|a| a == agent_id))).collect())
    }

    // ---------- companies ----------
    pub fn create_company(&self, n: NewCompany) -> Result<Company> {
        let name = n.name.trim();
        if name.is_empty() {
            return Err(Error::invalid("у компании должно быть название"));
        }
        let c = self.lock();
        let id = new_id();
        let now = now_ms();
        let color = if n.color.is_empty() { "#5e5ce6".to_string() } else { n.color };
        c.execute(
            "INSERT INTO companies(id,name,brief,color,created_at,updated_at) VALUES (?,?,?,?,?,?)",
            params![id, name, n.brief, color, now, now],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => Error::invalid("компания с таким названием уже есть"),
            other => Error::Db(other),
        })?;
        emit_c(&c, "company.created", "company", &id, "user", serde_json::json!({"name": name}))?;
        Ok(c.query_row(&format!("SELECT {COMPANY_COLS} FROM companies WHERE id=?"), [&id], company_from)?)
    }

    pub fn get_company(&self, id: &str) -> Result<Company> {
        self.lock()
            .query_row(&format!("SELECT {COMPANY_COLS} FROM companies WHERE id=?"), [id], company_from)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("компания {id}")))
    }

    pub fn list_companies(&self) -> Result<Vec<Company>> {
        let c = self.lock();
        let mut st = c.prepare(&format!("SELECT {COMPANY_COLS} FROM companies WHERE status!='archived' ORDER BY lower(name)"))?;
        let v = st.query_map([], company_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn update_company(&self, id: &str, name: Option<&str>, brief: Option<&str>, color: Option<&str>, status: Option<&str>) -> Result<Company> {
        let c = self.lock();
        let now = now_ms();
        for (col, val) in [("name", name), ("brief", brief), ("color", color), ("status", status)] {
            if let Some(v) = val {
                c.execute(&format!("UPDATE companies SET {col}=?, updated_at=? WHERE id=?"), params![v.trim(), now, id])?;
            }
        }
        // A changed brief reaches whoever works there: it is part of every working set.
        emit_c(&c, "company.updated", "company", id, "user", serde_json::json!({}))?;
        c.query_row(&format!("SELECT {COMPANY_COLS} FROM companies WHERE id=?"), [id], company_from)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("компания {id}")))
    }

    pub fn company_of_project(&self, project_id: &str) -> Result<Option<String>> {
        Ok(self.lock().query_row("SELECT company_id FROM projects WHERE id=?", [project_id], |r| r.get(0)).optional()?.flatten())
    }

    pub fn list_memory(&self, level: Option<MemoryLevel>, agent_id: Option<&str>, project_id: Option<&str>, include_inactive: bool, limit: i64) -> Result<Vec<Memory>> {
        self.list_memory_in(level, agent_id, project_id, None, include_inactive, limit)
    }

    // ---------- routines: work on a schedule ----------
    /// A run missed by more than this (the app was off) is skipped, not made up: a Monday report on Wednesday is noise.
    const ROUTINE_GRACE_MS: i64 = 12 * 60 * 60 * 1000;
    const ROUTINE_COLS: &'static str = "id,agent_id,project_id,name,cron,title,prompt,enabled,source,last_run_at,next_run_at,created_at,updated_at";

    fn routine_from(r: &Row) -> rusqlite::Result<Routine> {
        Ok(Routine {
            id: r.get(0)?,
            agent_id: r.get(1)?,
            project_id: r.get(2)?,
            name: r.get(3)?,
            cron: r.get(4)?,
            title: r.get(5)?,
            prompt: r.get(6)?,
            enabled: r.get::<_, i64>(7)? != 0,
            source: r.get(8)?,
            last_run_at: r.get(9)?,
            next_run_at: r.get(10)?,
            created_at: r.get(11)?,
            updated_at: r.get(12)?,
        })
    }

    pub fn list_routines(&self, agent_id: Option<&str>) -> Result<Vec<Routine>> {
        let c = self.lock();
        let sql = format!("SELECT {} FROM routines {} ORDER BY agent_id, name", Self::ROUTINE_COLS, if agent_id.is_some() { "WHERE agent_id=?1" } else { "" });
        let mut st = c.prepare(&sql)?;
        let rows = if let Some(a) = agent_id { st.query_map([a], Self::routine_from)?.collect::<rusqlite::Result<Vec<_>>>()? } else { st.query_map([], Self::routine_from)?.collect::<rusqlite::Result<Vec<_>>>()? };
        Ok(rows)
    }

    pub fn get_routine(&self, id: &str) -> Result<Routine> {
        self.lock()
            .query_row(&format!("SELECT {} FROM routines WHERE id=?", Self::ROUTINE_COLS), [id], Self::routine_from)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("регулярная задача {id}")))
    }

    /// Create or change a routine. `offset_secs` is the node's local UTC offset: the cron is read in local time.
    pub fn save_routine(&self, id: Option<&str>, i: RoutineInput, offset_secs: i64) -> Result<Routine> {
        let existing = match id {
            Some(id) => Some(self.get_routine(id)?),
            None => None,
        };
        let mut m = existing.clone().unwrap_or(Routine {
            id: String::new(), agent_id: String::new(), project_id: None, name: String::new(), cron: String::new(), title: String::new(),
            prompt: String::new(), enabled: true, source: String::new(), last_run_at: None, next_run_at: None, created_at: 0, updated_at: 0,
        });
        if let Some(v) = i.agent_id {
            m.agent_id = self.get_agent(&v)?.id;
        }
        if let Some(v) = i.project_id {
            m.project_id = if v.is_empty() { None } else { Some(self.get_project(&v)?.id) };
        }
        if let Some(v) = i.name {
            m.name = v.trim().to_string();
        }
        if let Some(v) = i.cron {
            m.cron = v.trim().to_string();
        }
        if let Some(v) = i.title {
            m.title = v.trim().to_string();
        }
        if let Some(v) = i.prompt {
            m.prompt = v;
        }
        if let Some(v) = i.enabled {
            m.enabled = v;
        }
        if let Some(v) = i.source {
            m.source = v;
        }
        if m.agent_id.is_empty() {
            return Err(Error::invalid("укажите специалиста"));
        }
        if m.name.is_empty() {
            return Err(Error::invalid("назовите регулярную задачу"));
        }
        if m.title.is_empty() {
            m.title = m.name.clone();
        }
        let cron = crate::cron::Cron::parse(&m.cron)?;
        let now = now_ms();
        let cron_changed = existing.as_ref().map(|e| e.cron != m.cron).unwrap_or(true);
        let turned_on = existing.as_ref().map(|e| !e.enabled && m.enabled).unwrap_or(false);
        if m.enabled && (cron_changed || turned_on || m.next_run_at.is_none()) {
            m.next_run_at = cron.next_after(now, offset_secs);
        }
        if !m.enabled {
            m.next_run_at = None;
        }
        let c = self.lock();
        let sid = match &existing {
            Some(e) => {
                c.execute(
                    "UPDATE routines SET agent_id=?,project_id=?,name=?,cron=?,title=?,prompt=?,enabled=?,source=?,next_run_at=?,updated_at=? WHERE id=?",
                    params![m.agent_id, m.project_id, m.name, m.cron, m.title, m.prompt, m.enabled as i64, m.source, m.next_run_at, now, e.id],
                )
                .map_err(Self::routine_constraint)?;
                e.id.clone()
            }
            None => {
                let nid = new_id();
                c.execute(
                    "INSERT INTO routines(id,agent_id,project_id,name,cron,title,prompt,enabled,source,last_run_at,next_run_at,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,NULL,?,?,?)",
                    params![nid, m.agent_id, m.project_id, m.name, m.cron, m.title, m.prompt, m.enabled as i64, m.source, m.next_run_at, now, now],
                )
                .map_err(Self::routine_constraint)?;
                nid
            }
        };
        emit_c(&c, if existing.is_some() { "routine.updated" } else { "routine.added" }, "routine", &sid, "user", serde_json::json!({"name": m.name, "cron": m.cron}))?;
        drop(c);
        self.get_routine(&sid)
    }

    fn routine_constraint(e: rusqlite::Error) -> Error {
        match e {
            rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => Error::invalid("у этого специалиста уже есть регулярная задача с таким названием"),
            other => Error::Db(other),
        }
    }

    /// For routines declared by a skill: make it if it is not there yet, and never touch it afterwards, so the
    /// person's changes (another time, switched off) survive a restart.
    pub fn ensure_routine(&self, agent_id: &str, name: &str, cron: &str, title: &str, prompt: &str, source: &str, offset_secs: i64) -> Result<(Routine, bool)> {
        let found: Option<String> = self.lock().query_row("SELECT id FROM routines WHERE agent_id=? AND name=?", params![agent_id, name], |r| r.get(0)).optional()?;
        if let Some(id) = found {
            return Ok((self.get_routine(&id)?, false));
        }
        let r = self.save_routine(
            None,
            RoutineInput { agent_id: Some(agent_id.into()), name: Some(name.into()), cron: Some(cron.into()), title: Some(title.into()), prompt: Some(prompt.into()), source: Some(source.into()), ..Default::default() },
            offset_secs,
        )?;
        Ok((r, true))
    }

    pub fn delete_routine(&self, id: &str) -> Result<()> {
        let c = self.lock();
        c.execute("DELETE FROM routines WHERE id=?", [id])?;
        emit_c(&c, "routine.removed", "routine", id, "user", serde_json::json!({}))?;
        Ok(())
    }

    /// Make the task for a routine now. The schedule is untouched.
    pub fn run_routine(&self, id: &str, now: i64, offset_secs: i64) -> Result<Obligation> {
        let r = self.get_routine(id)?;
        let agent = self.get_agent(&r.agent_id)?;
        if agent.status != "active" {
            return Err(Error::invalid("специалист не работает"));
        }
        let title = format!("{} · {}", r.title, crate::cron::local_date(now, offset_secs));
        let o = self.create_obligation(NewObligation {
            title,
            result: if r.prompt.trim().is_empty() { r.title.clone() } else { r.prompt.clone() },
            criteria: "Итог записан в отчёте задачи: что сделано, что найдено, что требует решения человека.".into(),
            owner_id: Some(agent.id),
            project_id: r.project_id.clone(),
            requested_by: format!("routine:{}", r.id),
            ..Default::default()
        })?;
        let c = self.lock();
        emit_c(&c, "routine.fired", "routine", &r.id, "system", serde_json::json!({"obligation": o.id}))?;
        Ok(o)
    }

    /// Called by the node every couple of seconds: make the tasks that are due, and set the next time.
    pub fn fire_due_routines(&self, now: i64, offset_secs: i64) -> Result<Vec<(Routine, Obligation)>> {
        let due: Vec<Routine> = self.list_routines(None)?.into_iter().filter(|r| r.enabled && r.next_run_at.map(|t| t <= now).unwrap_or(false)).collect();
        let mut fired = vec![];
        for r in due {
            let late = now - r.next_run_at.unwrap_or(now);
            let next = crate::cron::Cron::parse(&r.cron).ok().and_then(|c| c.next_after(now, offset_secs));
            // the previous run is still open: a second one on top of it helps nobody
            let still_open: i64 = self.lock().query_row(
                "SELECT COUNT(*) FROM obligations WHERE requested_by=? AND state NOT IN ('done','cancelled')",
                [format!("routine:{}", r.id)],
                |x| x.get(0),
            )?;
            let skip = if late > Self::ROUTINE_GRACE_MS {
                Some("пропущена: приложение было выключено в это время")
            } else if still_open > 0 {
                Some("пропущена: прошлый запуск ещё не завершён")
            } else {
                None
            };
            match skip {
                None => {
                    if let Ok(o) = self.run_routine(&r.id, now, offset_secs) {
                        fired.push((r.clone(), o));
                    }
                    self.lock().execute("UPDATE routines SET last_run_at=?, next_run_at=? WHERE id=?", params![now, next, r.id])?;
                }
                Some(why) => {
                    let c = self.lock();
                    c.execute("UPDATE routines SET next_run_at=? WHERE id=?", params![next, r.id])?;
                    emit_c(&c, "routine.skipped", "routine", &r.id, "system", serde_json::json!({"why": why}))?;
                }
            }
        }
        Ok(fired)
    }

    /// What the calendar shows between two moments: what the schedules will do, what they already did, and the
    /// tasks that have a deadline in that time.
    pub fn calendar(&self, from_ms: i64, to_ms: i64, offset_secs: i64) -> Result<serde_json::Value> {
        let now = now_ms();
        let mut items: Vec<serde_json::Value> = vec![];
        // the plan: only what is still ahead (the past is what really happened, below)
        for r in self.list_routines(None)?.into_iter().filter(|r| r.enabled) {
            let Ok(cron) = crate::cron::Cron::parse(&r.cron) else { continue };
            for at in cron.occurrences(from_ms.max(now), to_ms, offset_secs, 400) {
                items.push(serde_json::json!({"kind": "planned", "at": at, "routine_id": r.id, "agent_id": r.agent_id, "title": r.title, "name": r.name, "project_id": r.project_id}));
            }
        }
        {
            let c = self.lock();
            // what the schedules made
            let mut st = c.prepare("SELECT id,title,owner_id,state,created_at,requested_by,project_id FROM obligations WHERE requested_by LIKE 'routine:%' AND created_at>=?1 AND created_at<?2 ORDER BY created_at")?;
            let rows = st
                .query_map(params![from_ms, to_ms], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, String>(3)?, r.get::<_, i64>(4)?, r.get::<_, String>(5)?, r.get::<_, Option<String>>(6)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (id, title, owner, state, at, by, project) in rows {
                items.push(serde_json::json!({"kind": "run", "at": at, "obligation_id": id, "routine_id": by.trim_start_matches("routine:"), "agent_id": owner, "title": title, "state": state, "project_id": project}));
            }
            // tasks with a deadline
            let mut st = c.prepare("SELECT id,title,owner_id,state,due_at,project_id FROM obligations WHERE due_at>=?1 AND due_at<?2 AND state NOT IN ('done','cancelled') ORDER BY due_at")?;
            let rows = st
                .query_map(params![from_ms, to_ms], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, String>(3)?, r.get::<_, i64>(4)?, r.get::<_, Option<String>>(5)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (id, title, owner, state, at, project) in rows {
                items.push(serde_json::json!({"kind": "due", "at": at, "obligation_id": id, "agent_id": owner, "title": title, "state": state, "project_id": project}));
            }
        }
        items.sort_by_key(|v| v["at"].as_i64().unwrap_or(0));
        Ok(serde_json::json!({"from": from_ms, "to": to_ms, "now": now, "offset": offset_secs, "items": items}))
    }

    // ---------- "allow always" ----------
    pub fn list_permission_rules(&self) -> Result<Vec<PermissionRule>> {
        let c = self.lock();
        let mut st = c.prepare("SELECT id,kind,value,label,created_at FROM permission_rules ORDER BY created_at DESC")?;
        let rows = st
            .query_map([], |r| Ok(PermissionRule { id: r.get(0)?, kind: r.get(1)?, value: r.get(2)?, label: r.get(3)?, created_at: r.get(4)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Remember a rule; asking for the same one twice is not an error.
    pub fn add_permission_rule(&self, kind: &str, value: &str, label: &str) -> Result<PermissionRule> {
        if !["path", "cmd", "tool"].contains(&kind) || value.trim().is_empty() {
            return Err(Error::invalid("неизвестное правило разрешений"));
        }
        let c = self.lock();
        c.execute(
            "INSERT OR IGNORE INTO permission_rules(id,kind,value,label,created_at) VALUES (?,?,?,?,?)",
            params![new_id(), kind, value, label, now_ms()],
        )?;
        emit_c(&c, "permission.always", "permission", kind, "user", serde_json::json!({"value": value, "label": label}))?;
        Ok(c.query_row(
            "SELECT id,kind,value,label,created_at FROM permission_rules WHERE kind=? AND value=?",
            params![kind, value],
            |r| Ok(PermissionRule { id: r.get(0)?, kind: r.get(1)?, value: r.get(2)?, label: r.get(3)?, created_at: r.get(4)? }),
        )?)
    }

    pub fn delete_permission_rule(&self, id: &str) -> Result<()> {
        let c = self.lock();
        c.execute("DELETE FROM permission_rules WHERE id=?", [id])?;
        emit_c(&c, "permission.revoked", "permission", id, "user", serde_json::json!({}))?;
        Ok(())
    }

    // ---------- accesses: what the person lets specialists use ----------
    const ACCESS_COLS: &'static str = "id,name,label,kind,host,login,secret,note,enabled,created_at,updated_at";
    /// How long one "yes" to a single use stays good.
    const ACCESS_PERMIT_MS: i64 = 15 * 60 * 1000;

    fn access_from(c: &Connection, r: &Row) -> rusqlite::Result<Access> {
        let id: String = r.get(0)?;
        let grants: std::collections::BTreeMap<String, String> = {
            let mut st = c.prepare("SELECT agent_id, mode FROM access_grants WHERE access_id=?")?;
            let v = st.query_map([&id], |x| Ok((x.get::<_, String>(0)?, x.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            v.into_iter().collect()
        };
        Ok(Access {
            id,
            name: r.get(1)?,
            label: r.get(2)?,
            kind: r.get(3)?,
            host: r.get(4)?,
            login: r.get(5)?,
            secret: r.get(6)?,
            note: r.get(7)?,
            enabled: r.get::<_, i64>(8)? != 0,
            grants,
            created_at: r.get(9)?,
            updated_at: r.get(10)?,
        })
    }

    pub fn list_accesses(&self) -> Result<Vec<Access>> {
        let c = self.lock();
        let mut st = c.prepare(&format!("SELECT {} FROM accesses ORDER BY name", Self::ACCESS_COLS))?;
        let rows = st.query_map([], |r| Self::access_from(&c, r))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_access(&self, id_or_name: &str) -> Result<Access> {
        let c = self.lock();
        c.query_row(&format!("SELECT {} FROM accesses WHERE id=?1 OR name=?1", Self::ACCESS_COLS), [id_or_name], |r| Self::access_from(&c, r))
            .optional()?
            .ok_or_else(|| Error::not_found(format!("доступ {id_or_name}")))
    }

    pub fn save_access(&self, id: Option<&str>, i: AccessInput) -> Result<Access> {
        let mut m = match id {
            Some(id) => self.get_access(id)?,
            None => Access { kind: "server".into(), enabled: true, ..Default::default() },
        };
        if let Some(v) = i.name {
            m.name = v.trim().to_string();
        }
        if let Some(v) = i.label {
            m.label = v.trim().to_string();
        }
        if let Some(v) = i.kind {
            m.kind = v;
        }
        if let Some(v) = i.host {
            m.host = v.trim().to_string();
        }
        if let Some(v) = i.login {
            m.login = v.trim().to_string();
        }
        if let Some(v) = i.secret {
            if !v.is_empty() {
                m.secret = v;
            }
        }
        if i.clear_secret == Some(true) {
            m.secret.clear();
        }
        if let Some(v) = i.note {
            m.note = v;
        }
        if let Some(v) = i.enabled {
            m.enabled = v;
        }
        let ok_name = m.name.len() >= 2 && m.name.len() <= 31 && m.name.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false) && m.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
        if !ok_name {
            return Err(Error::invalid("имя доступа: латиница в нижнем регистре, цифры, - и _, от 2 до 31 знака"));
        }
        if !["server", "database", "site", "service", "other"].contains(&m.kind.as_str()) {
            return Err(Error::invalid("неизвестный вид доступа"));
        }
        if m.host.is_empty() {
            return Err(Error::invalid("укажите адрес: сервер, базу или сайт"));
        }
        if let Some(g) = &i.grants {
            for (agent, mode) in g {
                if !["allow", "ask", "deny"].contains(&mode.as_str()) {
                    return Err(Error::invalid(format!("неизвестный режим доступа «{mode}» (для {agent})")));
                }
                self.get_agent(agent)?;
            }
        }
        let c = self.lock();
        let now = now_ms();
        let sid = match id {
            Some(_) => {
                c.execute(
                    "UPDATE accesses SET name=?,label=?,kind=?,host=?,login=?,secret=?,note=?,enabled=?,updated_at=? WHERE id=?",
                    params![m.name, m.label, m.kind, m.host, m.login, m.secret, m.note, m.enabled as i64, now, m.id],
                )?;
                m.id.clone()
            }
            None => {
                let nid = new_id();
                c.execute(
                    "INSERT INTO accesses(id,name,label,kind,host,login,secret,note,enabled,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?)",
                    params![nid, m.name, m.label, m.kind, m.host, m.login, m.secret, m.note, m.enabled as i64, now, now],
                )
                .map_err(|e| match e {
                    rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => Error::invalid("доступ с таким именем уже есть"),
                    other => Error::Db(other),
                })?;
                nid
            }
        };
        if let Some(g) = i.grants {
            for (agent, mode) in g {
                if mode == "deny" {
                    c.execute("DELETE FROM access_grants WHERE access_id=? AND agent_id=?", params![sid, agent])?;
                } else {
                    c.execute(
                        "INSERT INTO access_grants(access_id,agent_id,mode) VALUES (?,?,?) ON CONFLICT(access_id,agent_id) DO UPDATE SET mode=excluded.mode",
                        params![sid, agent, mode],
                    )?;
                }
                emit_c(&c, "access.granted", "access", &sid, "user", serde_json::json!({"agent": agent, "mode": mode}))?;
            }
        }
        emit_c(&c, if id.is_some() { "access.updated" } else { "access.added" }, "access", &sid, "user", serde_json::json!({"name": m.name}))?;
        drop(c);
        self.get_access(&sid)
    }

    pub fn delete_access(&self, id: &str) -> Result<()> {
        let c = self.lock();
        c.execute("DELETE FROM accesses WHERE id=?", [id])?;
        emit_c(&c, "access.removed", "access", id, "user", serde_json::json!({}))?;
        Ok(())
    }

    /// The person said yes to one use by this specialist: good once, for a quarter of an hour.
    pub fn add_access_permit(&self, access_id: &str, agent_id: &str) -> Result<()> {
        self.lock().execute(
            "INSERT INTO access_permits(id,access_id,agent_id,expires_at) VALUES (?,?,?,?)",
            params![new_id(), access_id, agent_id, now_ms() + Self::ACCESS_PERMIT_MS],
        )?;
        Ok(())
    }

    /// Spend a permit if there is a live one.
    pub fn take_access_permit(&self, access_id: &str, agent_id: &str) -> Result<bool> {
        let c = self.lock();
        c.execute("DELETE FROM access_permits WHERE expires_at < ?", [now_ms()])?;
        let id: Option<String> = c
            .query_row("SELECT id FROM access_permits WHERE access_id=? AND agent_id=? ORDER BY expires_at LIMIT 1", params![access_id, agent_id], |r| r.get(0))
            .optional()?;
        match id {
            Some(id) => {
                c.execute("DELETE FROM access_permits WHERE id=?", [id])?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Every use is written down: who, when, what for. The secret itself never is.
    pub fn note_access_use(&self, access_id: &str, agent_id: &str, reason: &str, how: &str) -> Result<()> {
        let c = self.lock();
        emit_c(&c, "access.used", "access", access_id, agent_id, serde_json::json!({"reason": reason.chars().take(300).collect::<String>(), "how": how}))?;
        Ok(())
    }

    pub fn access_log(&self, access_id: &str, limit: i64) -> Result<Vec<Event>> {
        self.events(Some("access"), Some(access_id), limit)
    }

    // ---------- API integrations ----------
    fn integration_from(c: &Connection, r: &Row) -> rusqlite::Result<Integration> {
        let id: String = r.get(0)?;
        let agents: Vec<String> = {
            let mut st = c.prepare("SELECT agent_id FROM agent_integrations WHERE integration_id=?")?;
            let v = st.query_map([&id], |x| x.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
            v
        };
        Ok(Integration {
            id,
            name: r.get(1)?,
            label: r.get(2)?,
            base_url: r.get(3)?,
            auth_type: r.get(4)?,
            auth_name: r.get(5)?,
            secret: r.get(6)?,
            docs: r.get(7)?,
            write_mode: r.get(8)?,
            enabled: r.get::<_, i64>(9)? != 0,
            shared: r.get::<_, i64>(10)? != 0,
            agents,
            created_at: r.get(11)?,
            updated_at: r.get(12)?,
        })
    }

    pub fn list_integrations(&self) -> Result<Vec<Integration>> {
        let c = self.lock();
        let mut st = c.prepare("SELECT id,name,label,base_url,auth_type,auth_name,secret,docs,write_mode,enabled,shared,created_at,updated_at FROM integrations ORDER BY name")?;
        let rows = st.query_map([], |r| Self::integration_from(&c, r))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_integration(&self, id: &str) -> Result<Integration> {
        let c = self.lock();
        c.query_row(
            "SELECT id,name,label,base_url,auth_type,auth_name,secret,docs,write_mode,enabled,shared,created_at,updated_at FROM integrations WHERE id=?1 OR name=?1",
            [id],
            |r| Self::integration_from(&c, r),
        )
        .optional()?
        .ok_or_else(|| Error::not_found(format!("подключение {id}")))
    }

    pub fn save_integration(&self, id: Option<&str>, i: IntegrationInput) -> Result<Integration> {
        let mut m = match id {
            Some(id) => self.get_integration(id)?,
            None => Integration { auth_type: "bearer".into(), write_mode: "approve".into(), enabled: true, ..Default::default() },
        };
        if let Some(v) = i.name {
            m.name = v.trim().to_string();
        }
        if let Some(v) = i.label {
            m.label = v.trim().to_string();
        }
        if let Some(v) = i.base_url {
            m.base_url = v.trim().trim_end_matches('/').to_string();
        }
        if let Some(v) = i.auth_type {
            m.auth_type = v;
        }
        if let Some(v) = i.auth_name {
            m.auth_name = v.trim().to_string();
        }
        if let Some(v) = i.secret {
            if !v.is_empty() {
                m.secret = v;
            }
        }
        if i.clear_secret == Some(true) {
            m.secret.clear();
        }
        if let Some(v) = i.docs {
            m.docs = v;
        }
        if let Some(v) = i.write_mode {
            m.write_mode = v;
        }
        if let Some(v) = i.enabled {
            m.enabled = v;
        }
        if let Some(v) = i.shared {
            m.shared = v;
        }
        let ok_name = m.name.len() >= 2 && m.name.len() <= 31 && m.name.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false) && m.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
        if !ok_name {
            return Err(Error::invalid("имя подключения: латиница в нижнем регистре, цифры, - и _, от 2 до 31 знака"));
        }
        if !(m.base_url.starts_with("https://") || m.base_url.starts_with("http://")) {
            return Err(Error::invalid("адрес API должен начинаться с https:// (или http://)"));
        }
        if !["none", "bearer", "header", "basic", "query"].contains(&m.auth_type.as_str()) {
            return Err(Error::invalid("неизвестный способ авторизации"));
        }
        if matches!(m.auth_type.as_str(), "header" | "query") && m.auth_name.is_empty() {
            return Err(Error::invalid("укажите имя заголовка или параметра для ключа"));
        }
        if !["deny", "approve", "auto"].contains(&m.write_mode.as_str()) {
            return Err(Error::invalid("неизвестный режим записи"));
        }
        let c = self.lock();
        let now = now_ms();
        let sid = match id {
            Some(_) => {
                c.execute(
                    "UPDATE integrations SET name=?,label=?,base_url=?,auth_type=?,auth_name=?,secret=?,docs=?,write_mode=?,enabled=?,shared=?,updated_at=? WHERE id=?",
                    params![m.name, m.label, m.base_url, m.auth_type, m.auth_name, m.secret, m.docs, m.write_mode, m.enabled as i64, m.shared as i64, now, m.id],
                )?;
                m.id.clone()
            }
            None => {
                let nid = new_id();
                c.execute(
                    "INSERT INTO integrations(id,name,label,base_url,auth_type,auth_name,secret,docs,write_mode,enabled,shared,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
                    params![nid, m.name, m.label, m.base_url, m.auth_type, m.auth_name, m.secret, m.docs, m.write_mode, m.enabled as i64, m.shared as i64, now, now],
                )
                .map_err(|e| match e {
                    rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => Error::invalid("подключение с таким именем уже есть"),
                    other => Error::Db(other),
                })?;
                nid
            }
        };
        if let Some(agents) = i.agents {
            c.execute("DELETE FROM agent_integrations WHERE integration_id=?", [&sid])?;
            for a in agents {
                c.execute("INSERT OR IGNORE INTO agent_integrations(agent_id,integration_id) VALUES (?,?)", params![a, sid])?;
            }
        }
        emit_c(&c, if id.is_some() { "integration.updated" } else { "integration.added" }, "integration", &sid, "user", serde_json::json!({"name": m.name}))?;
        drop(c);
        self.get_integration(&sid)
    }

    pub fn delete_integration(&self, id: &str) -> Result<()> {
        let c = self.lock();
        c.execute("DELETE FROM integrations WHERE id=?", [id])?;
        emit_c(&c, "integration.removed", "integration", id, "user", serde_json::json!({}))?;
        Ok(())
    }

    pub fn integrations_for_agent(&self, agent_id: &str) -> Result<Vec<Integration>> {
        Ok(self.list_integrations()?.into_iter().filter(|m| m.enabled && (m.shared || m.agents.iter().any(|a| a == agent_id))).collect())
    }

    /// "Equipment" of one specialist: replace the set of MCP servers / API connections assigned to them.
    pub fn set_agent_mcp(&self, agent_id: &str, server_ids: &[String]) -> Result<()> {
        let c = self.lock();
        c.execute("DELETE FROM agent_mcp WHERE agent_id=?", [agent_id])?;
        for s in server_ids {
            c.execute("INSERT OR IGNORE INTO agent_mcp(agent_id,server_id) VALUES (?,?)", params![agent_id, s])?;
        }
        emit_c(&c, "agent.equipment_changed", "agent", agent_id, "user", serde_json::json!({"kind": "mcp"}))?;
        Ok(())
    }

    pub fn set_agent_integrations(&self, agent_id: &str, ids: &[String]) -> Result<()> {
        let c = self.lock();
        c.execute("DELETE FROM agent_integrations WHERE agent_id=?", [agent_id])?;
        for s in ids {
            c.execute("INSERT OR IGNORE INTO agent_integrations(agent_id,integration_id) VALUES (?,?)", params![agent_id, s])?;
        }
        emit_c(&c, "agent.equipment_changed", "agent", agent_id, "user", serde_json::json!({"kind": "api"}))?;
        Ok(())
    }

    /// Move a chat into a project / company (used when the user agrees to set one up for ongoing work).
    pub fn move_thread(&self, thread_id: &str, project_id: Option<&str>, company_id: Option<&str>) -> Result<()> {
        let c = self.lock();
        c.execute("UPDATE threads SET project_id=?, company_id=?, updated_at=? WHERE id=?", params![project_id, company_id, now_ms(), thread_id])?;
        emit_c(&c, "thread.moved", "thread", thread_id, "user", serde_json::json!({"project": project_id, "company": company_id}))?;
        Ok(())
    }

    /// A company by its exact name in any case. SQLite's lower() folds ASCII only, so «вокруг света» never met
    /// «Вокруг света» there: names are compared here instead.
    pub fn company_by_name(&self, name: &str) -> Result<Option<Company>> {
        let want = fold_name(name);
        Ok(if want.is_empty() { None } else { self.list_companies()?.into_iter().find(|c| fold_name(&c.name) == want) })
    }

    /// A company a person referred to: its exact name, else the only one whose name contains the words
    /// («вокруг» → «Вокруг света»). For finding, never for creating.
    pub fn company_like(&self, name: &str) -> Result<Option<Company>> {
        if let Some(c) = self.company_by_name(name)? {
            return Ok(Some(c));
        }
        let want = fold_name(name);
        if want.is_empty() {
            return Ok(None);
        }
        let partial: Vec<Company> = self.list_companies()?.into_iter().filter(|c| fold_name(&c.name).contains(&want)).collect();
        Ok(if partial.len() == 1 { partial.into_iter().next() } else { None })
    }

    // ---------- import ----------
    /// Bring a chat over from another Senate database (a local one that ran while the server was out of reach).
    /// Additive only: an existing chat or message is never overwritten, ids are kept so a second run changes
    /// nothing, and a reference to a project, company or specialist this database does not know is dropped
    /// rather than invented. Returns (chat created, messages added).
    pub fn import_thread(&self, t: &serde_json::Value, msgs: &[serde_json::Value]) -> Result<(bool, usize)> {
        let s = |v: &serde_json::Value, k: &str| v[k].as_str().map(String::from);
        let i = |v: &serde_json::Value, k: &str| v[k].as_i64();
        let id = s(t, "id").filter(|x| !x.is_empty()).ok_or_else(|| Error::invalid("у чата нет id"))?;
        let mut c = self.lock();
        let known = |c: &Connection, table: &str, id: &Option<String>| -> rusqlite::Result<Option<String>> {
            match id {
                Some(x) if !x.is_empty() => Ok(c.query_row(&format!("SELECT id FROM {table} WHERE id=?"), [x], |r| r.get(0)).optional()?),
                _ => Ok(None),
            }
        };
        let tx = c.transaction()?;
        let now = now_ms();
        let project = known(&tx, "projects", &s(t, "project_id"))?;
        let company = known(&tx, "companies", &s(t, "company_id"))?;
        let agent = known(&tx, "agents", &s(t, "agent_id"))?;
        let kind = s(t, "kind").filter(|k| k == "chat" || k == "forum").unwrap_or_else(|| "chat".into());
        let created = tx.execute(
            "INSERT OR IGNORE INTO threads(id,kind,project_id,agent_id,title,auto_title,archived,pinned,model,effort,mode,cli_sessions,created_at,updated_at,company_id,meta)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,'{}',?,?,?,?)",
            params![
                id, kind, project, agent, s(t, "title").unwrap_or_default(), i(t, "auto_title").unwrap_or(0), i(t, "archived").unwrap_or(0),
                i(t, "pinned").unwrap_or(0), s(t, "model").unwrap_or_default(), s(t, "effort").unwrap_or_default(), s(t, "mode").unwrap_or_default(),
                i(t, "created_at").unwrap_or(now), i(t, "updated_at").unwrap_or(now), company, s(t, "meta").unwrap_or_else(|| "{}".into())
            ],
        )? == 1;
        let mut added = 0;
        for m in msgs {
            let Some(mid) = s(m, "id").filter(|x| !x.is_empty()) else { continue };
            // a message that was mid-run when the other database stopped is not running here
            let status = match s(m, "status").as_deref() {
                Some("queued") | Some("working") => "cancelled".to_string(),
                Some(x) => x.to_string(),
                None => "done".into(),
            };
            let author = if s(m, "author_kind").as_deref() == Some("agent") { known(&tx, "agents", &s(m, "author_id"))? } else { s(m, "author_id") };
            added += tx.execute(
                "INSERT OR IGNORE INTO messages(id,thread_id,author_kind,author_id,body,props,status,session_id,created_at,updated_at) VALUES (?,?,?,?,?,?,?,NULL,?,?)",
                params![
                    mid, id, s(m, "author_kind").unwrap_or_else(|| "user".into()), author, s(m, "body").unwrap_or_default(),
                    s(m, "props").unwrap_or_else(|| "{}".into()), status, i(m, "created_at").unwrap_or(now), i(m, "updated_at").unwrap_or(now)
                ],
            )?;
        }
        if added > 0 {
            tx.execute("UPDATE threads SET updated_at=max(updated_at, (SELECT max(created_at) FROM messages WHERE thread_id=?)) WHERE id=?", params![id, id])?;
        }
        emit_c(&tx, "thread.imported", "thread", &id, "user", serde_json::json!({"created": created, "messages": added}))?;
        tx.commit()?;
        Ok((created, added))
    }

    // ---------- forum ----------
    /// A forum has a subject, an expected outcome, a bounded number of rounds and an end condition
    /// (Senate.md section 13): a discussion that cannot go on forever.
    pub fn create_forum(&self, n: NewForum) -> Result<Thread> {
        let topic = n.topic.trim();
        if topic.is_empty() {
            return Err(Error::invalid("у форума должен быть вопрос"));
        }
        if n.participants.len() < 2 {
            return Err(Error::invalid("для обсуждения нужно минимум двое участников"));
        }
        for p in &n.participants {
            self.get_agent(p)?;
        }
        let outcome = match n.outcome.as_str() {
            "decision" | "analysis" => n.outcome.clone(),
            _ => "plan".to_string(),
        };
        let rounds = if n.rounds <= 0 { 2 } else { n.rounds.min(5) };
        let leader = n.leader_id.clone().filter(|l| n.participants.contains(l)).unwrap_or_else(|| n.participants[0].clone());
        let meta = serde_json::json!({
            "topic": topic, "outcome": outcome, "participants": n.participants, "rounds": rounds, "round": 0,
            "status": "open", "leader": leader, "plan": null, "plan_applied": false,
        });
        let c = self.lock();
        let id = new_id();
        let now = now_ms();
        let title: String = topic.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(70).collect();
        c.execute(
            "INSERT INTO threads(id,kind,project_id,company_id,title,auto_title,mode,meta,created_at,updated_at) VALUES (?, 'forum', ?, ?, ?, 0, 'plan', ?, ?, ?)",
            params![id, n.project_id, n.company_id.filter(|x| !x.is_empty()), title, meta.to_string(), now, now],
        )?;
        emit_c(&c, "forum.opened", "thread", &id, "user", serde_json::json!({"topic": topic}))?;
        Ok(c.query_row(&format!("SELECT {THREAD_COLS} FROM threads WHERE id=?"), [&id], thread_from)?)
    }

    pub fn forum_meta(&self, thread_id: &str) -> Result<serde_json::Value> {
        Ok(self.get_thread(thread_id)?.meta)
    }

    /// Shallow-merge into the forum state; null removes a key.
    pub fn set_forum_meta(&self, thread_id: &str, patch: &serde_json::Value) -> Result<serde_json::Value> {
        let c = self.lock();
        let cur: String = c
            .query_row("SELECT meta FROM threads WHERE id=?", [thread_id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| Error::not_found(format!("форум {thread_id}")))?;
        let mut v: serde_json::Value = json_or(&cur, serde_json::json!({}));
        if let (Some(dst), Some(src)) = (v.as_object_mut(), patch.as_object()) {
            for (k, val) in src {
                if val.is_null() {
                    dst.remove(k);
                } else {
                    dst.insert(k.clone(), val.clone());
                }
            }
        }
        c.execute("UPDATE threads SET meta=?, updated_at=? WHERE id=?", params![v.to_string(), now_ms(), thread_id])?;
        Ok(v)
    }

    // ---------- context recognition ----------
    /// Companies and projects named in a piece of text. A person who writes "design system of Vokrug
    /// Sveta" has told the environment everything it needs: it must find them itself. Names are matched
    /// by word stems (declensions: "Дегмы" = "Дегма") and generic names are never matched.
    pub fn mentions(&self, text: &str) -> Result<Vec<Mention>> {
        const GENERIC: &[&str] = &["общее", "общий", "проект", "компания", "чат", "тест"];
        let lower = text.to_lowercase();
        let words: Vec<String> = lower.split(|c: char| !(c.is_alphanumeric() || c == '-')).filter(|w| !w.is_empty()).map(String::from).collect();
        let stem = |w: &str| -> String {
            let n = w.chars().count();
            let keep = if n <= 4 { n } else { (n - 2).max(4) };
            w.chars().take(keep).collect()
        };
        let matches = |name: &str| -> bool {
            let name_l = name.to_lowercase();
            if GENERIC.contains(&name_l.trim()) || name_l.chars().count() < 3 {
                return false;
            }
            let parts: Vec<&str> = name_l.split(|c: char| !(c.is_alphanumeric() || c == '-')).filter(|w| !w.is_empty()).collect();
            if parts.is_empty() {
                return false;
            }
            // every word of the name must appear (by stem), in order
            let mut at = 0usize;
            for part in parts {
                let st = stem(part);
                match words[at..].iter().position(|w| w.starts_with(&st) && (w.chars().count() as i64 - part.chars().count() as i64).abs() <= 3) {
                    Some(i) => at += i + 1,
                    None => return false,
                }
            }
            true
        };
        let mut out = vec![];
        for c in self.list_companies()? {
            if matches(&c.name) {
                out.push(Mention { kind: "company", id: c.id.clone(), name: c.name.clone(), company_id: Some(c.id) });
            }
        }
        for p in self.list_projects()? {
            if matches(&p.name) {
                out.push(Mention { kind: "project", id: p.id.clone(), name: p.name.clone(), company_id: p.company_id.clone() });
            }
        }
        Ok(out)
    }

    // ---------- character sheet ----------
    /// Real numbers behind a specialist's "character sheet": what they have done, how reliable they
    /// are, how much they know about each company. Nothing here is invented.
    pub fn agent_stats(&self, agent_id: &str) -> Result<serde_json::Value> {
        let c = self.lock();
        let mut by_state = serde_json::Map::new();
        {
            let mut st = c.prepare("SELECT state, COUNT(*) FROM obligations WHERE owner_id=? GROUP BY state")?;
            for r in st.query_map([agent_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
                let (k, v) = r?;
                by_state.insert(k, v.into());
            }
        }
        let n = |k: &str| by_state.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
        let (done, cancelled, blocked) = (n("done"), n("cancelled"), n("blocked"));
        let mut mem = serde_json::Map::new();
        {
            let mut st = c.prepare("SELECT kind, COUNT(*) FROM memory WHERE agent_id=? AND status='active' AND level='personal' GROUP BY kind")?;
            for r in st.query_map([agent_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
                let (k, v) = r?;
                mem.insert(k, v.into());
            }
        }
        let mg = |k: &str| mem.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
        let (sessions, tokens): (i64, i64) = c.query_row(
            "SELECT COUNT(*), COALESCE(SUM(tokens_in+tokens_out),0) FROM sessions WHERE agent_id=? AND status='done'",
            [agent_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let corrections: i64 = c.query_row("SELECT COUNT(*) FROM memory WHERE agent_id=? AND supersedes_id IS NOT NULL", [agent_id], |r| r.get(0))?;
        let mut companies = vec![];
        {
            let mut st = c.prepare("SELECT id,name,color FROM companies WHERE status!='archived' ORDER BY lower(name)")?;
            let cs = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            for (id, name, color) in cs {
                let own: i64 = c.query_row("SELECT COUNT(*) FROM memory WHERE agent_id=?1 AND company_id=?2 AND status='active'", params![agent_id, id], |r| r.get(0))?;
                let shared: i64 = c.query_row("SELECT COUNT(*) FROM memory WHERE level='company' AND company_id=?1 AND status='active'", [&id], |r| r.get(0))?;
                let decisions: i64 = c.query_row("SELECT COUNT(*) FROM decisions WHERE company_id=?1 AND status='accepted'", [&id], |r| r.get(0))?;
                let done_here: i64 = c.query_row(
                    "SELECT COUNT(*) FROM obligations o JOIN projects p ON p.id=o.project_id WHERE o.owner_id=?1 AND p.company_id=?2 AND o.state='done'",
                    params![agent_id, id],
                    |r| r.get(0),
                )?;
                let projects: i64 = c.query_row(
                    "SELECT COUNT(*) FROM projects p JOIN project_members m ON m.project_id=p.id WHERE m.agent_id=?1 AND p.company_id=?2",
                    params![agent_id, id],
                    |r| r.get(0),
                )?;
                if own + done_here + projects > 0 {
                    companies.push(serde_json::json!({"id": id, "name": name, "color": color, "own_memory": own, "shared_memory": shared, "decisions": decisions, "done": done_here, "projects": projects}));
                }
            }
        }
        // XP only counts verifiable things: finished work, verified knowledge, corrections of own mistakes.
        let xp = done * 10 + mg("verified") * 4 + corrections * 2 + mg("observation") + mg("hypothesis") + sessions;
        let level = ((xp as f64 / 6.0).sqrt().floor() as i64) + 1;
        let next_at = (level * level) * 6;
        let closed = done + cancelled + blocked;
        let reliability = if closed > 0 { Some((done as f64 / closed as f64 * 100.0).round() as i64) } else { None };
        Ok(serde_json::json!({
            "level": level, "xp": xp, "next_level_xp": next_at,
            "obligations": by_state, "reliability": reliability,
            "memory": mem, "corrections": corrections, "sessions": sessions, "tokens": tokens,
            "companies": companies,
        }))
    }

    /// Everything learned in one session: what the agent saved itself and what the environment extracted.
    pub fn memory_of_session(&self, session_id: &str) -> Result<Vec<Memory>> {
        let c = self.lock();
        let mut st = c.prepare(&format!("SELECT {MEMORY_COLS} FROM memory m WHERE (m.source=?1 OR m.source=?2) AND m.status='active' ORDER BY m.created_at"))?;
        let v = st.query_map(params![session_id, format!("auto:{session_id}")], memory_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    // ---------- artifacts ----------
    pub fn save_artifact(&self, id: Option<&str>, project_id: Option<&str>, kind: &str, title: &str, body: &str, by: &str) -> Result<Artifact> {
        let c = self.lock();
        let now = now_ms();
        let art_id = match id {
            Some(existing) => {
                let n = c.execute("UPDATE artifacts SET title=?, body=?, version=version+1, updated_at=? WHERE id=?", params![title, body, now, existing])?;
                if n == 0 {
                    return Err(Error::not_found(format!("артефакт {existing}")));
                }
                existing.to_string()
            }
            None => {
                let nid = new_id();
                c.execute(
                    "INSERT INTO artifacts(id,project_id,kind,title,body,created_by,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?)",
                    params![nid, project_id, kind, title, body, by, now, now],
                )?;
                nid
            }
        };
        emit_c(&c, "artifact.saved", "artifact", &art_id, by, serde_json::json!({"project": project_id, "title": title}))?;
        Ok(c.query_row(
            "SELECT id,project_id,kind,title,body,version,created_by,created_at,updated_at FROM artifacts WHERE id=?",
            [&art_id],
            artifact_from,
        )?)
    }

    pub fn list_artifacts(&self, project_id: Option<&str>) -> Result<Vec<Artifact>> {
        let c = self.lock();
        let mut st = c.prepare(
            "SELECT id,project_id,kind,title,body,version,created_by,created_at,updated_at FROM artifacts
             WHERE (?1 IS NULL OR project_id=?1) ORDER BY updated_at DESC",
        )?;
        let v = st.query_map([project_id], artifact_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    // ---------- sessions ----------
    #[allow(clippy::too_many_arguments)]
    pub fn create_session(&self, agent_id: &str, obligation_id: Option<&str>, thread_id: Option<&str>, message_id: Option<&str>, reason: &str, backend: &str, prompt: &str) -> Result<Session> {
        let c = self.lock();
        let id = new_id();
        c.execute(
            "INSERT INTO sessions(id,agent_id,obligation_id,thread_id,message_id,reason,backend,prompt,created_at) VALUES (?,?,?,?,?,?,?,?,?)",
            params![id, agent_id, obligation_id, thread_id, message_id, reason, backend, prompt, now_ms()],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => {
                Error::invalid("по этой задаче уже идёт сессия")
            }
            other => Error::Db(other),
        })?;
        emit_c(&c, "session.queued", "session", &id, "system", serde_json::json!({"agent": agent_id, "obligation": obligation_id, "reason": reason}))?;
        Ok(c.query_row(&format!("SELECT {SESSION_COLS} FROM sessions WHERE id=?"), [&id], session_from)?)
    }

    pub fn get_session(&self, id: &str) -> Result<Session> {
        self.lock()
            .query_row(&format!("SELECT {SESSION_COLS} FROM sessions WHERE id=?"), [id], session_from)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("сессия {id}")))
    }

    pub fn list_sessions(&self, agent_id: Option<&str>, obligation_id: Option<&str>, status: Option<SessionStatus>, limit: i64) -> Result<Vec<Session>> {
        let c = self.lock();
        let mut sql = format!("SELECT {SESSION_COLS} FROM sessions WHERE 1=1");
        let mut args: Vec<String> = vec![];
        if let Some(a) = agent_id {
            sql += " AND agent_id=?";
            args.push(a.into());
        }
        if let Some(o) = obligation_id {
            sql += " AND obligation_id=?";
            args.push(o.into());
        }
        if let Some(s) = status {
            sql += " AND status=?";
            args.push(s.as_str().into());
        }
        sql += &format!(" ORDER BY created_at DESC LIMIT {limit}");
        let mut st = c.prepare(&sql)?;
        let v = st.query_map(params_from_iter(args.iter()), session_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    /// Atomically claim the oldest queued session for a node. Returns None if there is none.
    pub fn claim_session(&self, node_id: &str, accepts: &dyn Fn(&Session) -> bool) -> Result<Option<Session>> {
        let c = self.lock();
        let mut st = c.prepare(&format!("SELECT {SESSION_COLS} FROM sessions WHERE status='queued' ORDER BY created_at"))?;
        let queued = st.query_map([], session_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        drop(st);
        for s in queued {
            if !accepts(&s) {
                continue;
            }
            // One run at a time per specialist in a chat: they share one CLI session (resume), and
            // a second message to the same person waits its turn. Different specialists, and other
            // chats, run in parallel. `queued` is ordered by creation, so the wait is first-in first-out.
            if let Some(tid) = &s.thread_id {
                let busy: i64 = c.query_row(
                    "SELECT COUNT(*) FROM sessions WHERE status='running' AND thread_id=? AND agent_id=?",
                    params![tid, s.agent_id],
                    |r| r.get(0),
                )?;
                if busy > 0 {
                    continue;
                }
            }
            let n = c.execute(
                "UPDATE sessions SET status='running', node_id=?, started_at=? WHERE id=? AND status='queued'",
                params![node_id, now_ms(), s.id],
            )?;
            if n == 1 {
                emit_c(&c, "session.started", "session", &s.id, node_id, serde_json::json!({"agent": s.agent_id}))?;
                return Ok(Some(c.query_row(&format!("SELECT {SESSION_COLS} FROM sessions WHERE id=?"), [&s.id], session_from)?));
            }
        }
        Ok(None)
    }

    pub fn finish_session(&self, id: &str, status: SessionStatus, output: &str, summary: &str, error: &str, tokens_in: i64, tokens_out: i64) -> Result<Session> {
        let c = self.lock();
        c.execute(
            "UPDATE sessions SET status=?, output=?, summary=?, error=?, tokens_in=?, tokens_out=?, ended_at=? WHERE id=? AND status IN ('queued','running')",
            params![status.as_str(), output, summary, error, tokens_in, tokens_out, now_ms(), id],
        )?;
        emit_c(&c, &format!("session.{}", status.as_str()), "session", id, "system", serde_json::json!({"error": error}))?;
        Ok(c.query_row(&format!("SELECT {SESSION_COLS} FROM sessions WHERE id=?"), [id], session_from)?)
    }

    /// Mark the report as applied; returns false if it had already been (idempotency).
    pub fn mark_session_applied(&self, id: &str) -> Result<bool> {
        let n = self.lock().execute("UPDATE sessions SET applied=1 WHERE id=? AND applied=0", [id])?;
        Ok(n == 1)
    }

    /// After a crash: sessions left `running` have no live executor. Requeue them; the
    /// per-obligation work log tells the next session what was already done.
    pub fn requeue_orphaned_sessions(&self) -> Result<usize> {
        let c = self.lock();
        let ids: Vec<String> = {
            let mut st = c.prepare("SELECT id FROM sessions WHERE status='running'")?;
            let v = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
            v
        };
        for id in &ids {
            c.execute("UPDATE sessions SET status='queued', node_id=NULL, started_at=NULL WHERE id=?", [id])?;
            emit_c(&c, "session.requeued", "session", id, "system", serde_json::json!({"why": "исполнитель недоступен"}))?;
        }
        Ok(ids.len())
    }

    // ---------- approvals ----------
    pub fn create_approval(&self, agent_id: &str, session_id: Option<&str>, obligation_id: Option<&str>, action: &str, detail: &serde_json::Value) -> Result<Approval> {
        let c = self.lock();
        let id = new_id();
        c.execute(
            "INSERT INTO approvals(id,agent_id,session_id,obligation_id,action,detail,created_at) VALUES (?,?,?,?,?,?,?)",
            params![id, agent_id, session_id, obligation_id, action, detail.to_string(), now_ms()],
        )?;
        emit_c(&c, "approval.requested", "approval", &id, agent_id, serde_json::json!({"action": action}))?;
        Ok(c.query_row(&format!("SELECT {APPROVAL_COLS} FROM approvals WHERE id=?"), [&id], approval_from)?)
    }

    pub fn get_approval(&self, id: &str) -> Result<Approval> {
        self.lock()
            .query_row(&format!("SELECT {APPROVAL_COLS} FROM approvals WHERE id=?"), [id], approval_from)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("запрос согласования {id}")))
    }

    pub fn list_approvals(&self, pending_only: bool) -> Result<Vec<Approval>> {
        let c = self.lock();
        let sql = format!(
            "SELECT {APPROVAL_COLS} FROM approvals {} ORDER BY created_at DESC LIMIT 200",
            if pending_only { "WHERE status='pending'" } else { "" }
        );
        let mut st = c.prepare(&sql)?;
        let v = st.query_map([], approval_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn set_approval_status(&self, id: &str, status: ApprovalStatus) -> Result<()> {
        let c = self.lock();
        let n = c.execute(
            "UPDATE approvals SET status=?, decided_at=? WHERE id=? AND status='pending'",
            params![status.as_str(), now_ms(), id],
        )?;
        if n == 0 {
            return Err(Error::invalid("запрос уже обработан"));
        }
        emit_c(&c, &format!("approval.{}", status.as_str()), "approval", id, "user", serde_json::json!({}))?;
        Ok(())
    }

    // ---------- nodes ----------
    pub fn upsert_node(&self, id: &str, name: &str, os: &str, kind: &str, token_hash: &str, capabilities: &serde_json::Value) -> Result<Node> {
        let c = self.lock();
        let now = now_ms();
        c.execute(
            "INSERT INTO nodes(id,name,os,kind,token_hash,capabilities,last_seen,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?7)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, os=excluded.os, capabilities=excluded.capabilities, last_seen=excluded.last_seen",
            params![id, name, os, kind, token_hash, capabilities.to_string(), now],
        )?;
        Ok(c.query_row("SELECT id,name,os,kind,capabilities,last_seen,created_at FROM nodes WHERE id=?", [id], node_from)?)
    }

    pub fn touch_node(&self, id: &str, capabilities: Option<&serde_json::Value>) -> Result<()> {
        let c = self.lock();
        c.execute("UPDATE nodes SET last_seen=? WHERE id=?", params![now_ms(), id])?;
        if let Some(cap) = capabilities {
            c.execute("UPDATE nodes SET capabilities=? WHERE id=?", params![cap.to_string(), id])?;
        }
        Ok(())
    }

    pub fn node_by_token_hash(&self, hash: &str) -> Result<Option<Node>> {
        Ok(self
            .lock()
            .query_row("SELECT id,name,os,kind,capabilities,last_seen,created_at FROM nodes WHERE token_hash=? AND token_hash!=''", [hash], node_from)
            .optional()?)
    }

    pub fn list_nodes(&self) -> Result<Vec<Node>> {
        let c = self.lock();
        let mut st = c.prepare("SELECT id,name,os,kind,capabilities,last_seen,created_at FROM nodes ORDER BY kind, created_at")?;
        let v = st.query_map([], node_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn delete_node(&self, id: &str) -> Result<()> {
        self.lock().execute("DELETE FROM nodes WHERE id=? AND kind!='local'", [id])?;
        Ok(())
    }
}

const COMPANY_COLS: &str = "id,name,brief,color,status,created_at,updated_at";
fn company_from(r: &Row) -> rusqlite::Result<Company> {
    Ok(Company { id: r.get(0)?, name: r.get(1)?, brief: r.get(2)?, color: r.get(3)?, status: r.get(4)?, created_at: r.get(5)?, updated_at: r.get(6)? })
}

const THREAD_COLS: &str = "id,kind,project_id,agent_id,title,auto_title,archived,pinned,model,effort,mode,cli_sessions,created_at,updated_at,company_id,meta";
fn thread_from(r: &Row) -> rusqlite::Result<Thread> {
    let cli: String = r.get(11)?;
    Ok(Thread {
        id: r.get(0)?,
        kind: en(r, 1, ThreadKind::parse, ThreadKind::Chat)?,
        project_id: r.get(2)?,
        agent_id: r.get(3)?,
        title: r.get(4)?,
        auto_title: r.get::<_, i64>(5)? != 0,
        archived: r.get::<_, i64>(6)? != 0,
        pinned: r.get::<_, i64>(7)? != 0,
        model: r.get(8)?,
        effort: r.get(9)?,
        mode: r.get(10)?,
        cli_sessions: json_or(&cli, serde_json::json!({})),
        created_at: r.get(12)?,
        updated_at: r.get(13)?,
        company_id: r.get(14)?,
        meta: json_or(&r.get::<_, String>(15)?, serde_json::json!({})),
    })
}

const MESSAGE_COLS: &str = "id,thread_id,author_kind,author_id,body,props,status,session_id,created_at,updated_at";
fn message_from(r: &Row) -> rusqlite::Result<Message> {
    let props: String = r.get(5)?;
    Ok(Message {
        id: r.get(0)?,
        thread_id: r.get(1)?,
        author_kind: r.get(2)?,
        author_id: r.get(3)?,
        body: r.get(4)?,
        props: json_or(&props, serde_json::json!({})),
        status: en(r, 6, MessageStatus::parse, MessageStatus::Done)?,
        session_id: r.get(7)?,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
    })
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ThreadPatch {
    pub title: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub mode: Option<String>,
    pub archived: Option<bool>,
    pub pinned: Option<bool>,
    pub agent_id: Option<String>,
    /// Empty string takes the chat out of its company.
    pub company_id: Option<String>,
    /// Empty string takes the chat out of its project.
    pub project_id: Option<String>,
}

/// A name as people type it: any case, any script, ё as е, runs of spaces and quotes ignored.
pub fn fold_name(s: &str) -> String {
    s.to_lowercase()
        .replace('ё', "е")
        .chars()
        .filter(|c| !matches!(c, '«' | '»' | '"' | '\''))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ProjectPatch {
    pub name: Option<String>,
    pub goal: Option<String>,
    pub criteria: Option<String>,
    pub constraints: Option<String>,
    pub status: Option<String>,
    pub root_path: Option<String>,
    pub mode: Option<String>,
    /// Empty string detaches the project from its company.
    pub company_id: Option<String>,
}

fn artifact_from(r: &Row) -> rusqlite::Result<Artifact> {
    Ok(Artifact {
        id: r.get(0)?,
        project_id: r.get(1)?,
        kind: r.get(2)?,
        title: r.get(3)?,
        body: r.get(4)?,
        version: r.get(5)?,
        created_by: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

fn node_from(r: &Row) -> rusqlite::Result<Node> {
    let cap: String = r.get(4)?;
    Ok(Node {
        id: r.get(0)?,
        name: r.get(1)?,
        os: r.get(2)?,
        kind: r.get(3)?,
        capabilities: json_or(&cap, serde_json::json!([])),
        last_seen: r.get(5)?,
        created_at: r.get(6)?,
    })
}
