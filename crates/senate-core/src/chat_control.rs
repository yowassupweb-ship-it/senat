//! Long-lived chat controls. Compaction never deletes messages.
use crate::{Store, error::{Error, Result}, model::*};
use serde_json::{json, Value};

impl Store {
    pub fn chat_context(&self, tid: &str) -> Result<Value> {
        let t = self.get_thread(tid)?;
        let ids: Vec<String> = self.lock().prepare("SELECT DISTINCT author_id FROM messages WHERE thread_id=? AND author_kind='agent' AND author_id IS NOT NULL")?
            .query_map([tid], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        let mut participants = vec![];
        for id in ids {
            let mid: Option<String> = self.lock().query_row("SELECT id FROM messages WHERE thread_id=? AND author_id=? AND json_type(props,'$.work.context_window') IS NOT NULL ORDER BY created_at DESC,rowid DESC LIMIT 1", rusqlite::params![tid,id], |r|r.get(0)).ok();
            let m = mid.and_then(|id| self.get_message(&id).ok());
            let at = t.meta["compact"][&id]["at"].as_i64().unwrap_or(0);
            let reset = m.as_ref().map_or(at > 0, |m| at >= m.updated_at);
            let work = m.as_ref().map(|m|m.props["work"].clone()).unwrap_or(Value::Null);
            let name = self.get_agent(&id).map(|a| a.name).unwrap_or(id.clone());
            participants.push(json!({"id":id,"name":name,"tokens":if reset {Value::Null} else {work["context_tokens"].clone()},"window":work["context_window"],"compacted":reset,"compacted_at":at}));
        }
        let pending: i64 = self.lock().query_row("SELECT COUNT(*) FROM messages WHERE thread_id=? AND status IN ('queued','working') AND json_extract(props,'$.compact')=1",[tid],|r|r.get(0))?;
        Ok(json!({"auto":t.meta["context_auto"].as_bool().unwrap_or(true),"threshold":80,"pending":pending,"participants":participants}))
    }

    pub fn queue_chat_compact(&self, tid: &str, agent_id: &str, backend: &str, extra: &str, automatic: bool) -> Result<Option<Session>> {
        let t = self.get_thread(tid)?;
        if t.kind == ThreadKind::Forum { return Err(Error::invalid("сжатие доступно в обычном чате")); }
        let state = self.chat_context(tid)?;
        if automatic {
            if state["auto"] != true { return Ok(None); }
            let row = state["participants"].as_array().and_then(|v|v.iter().find(|v|v["id"]==agent_id));
            let Some(row) = row else { return Ok(None) };
            let window = row["window"].as_f64().unwrap_or(0.0);
            if row["compacted"] == true || window <= 0.0 || row["tokens"].as_f64().unwrap_or(0.0) / window < 0.8 { return Ok(None); }
        }
        let exists: bool = self.lock().query_row("SELECT EXISTS(SELECT 1 FROM messages WHERE thread_id=? AND author_id=? AND status IN ('queued','working') AND json_extract(props,'$.compact')=1)",rusqlite::params![tid,agent_id],|r|r.get(0))?;
        if exists { return Ok(None); }
        let a = self.get_agent(agent_id)?;
        let backend = if a.model_pref.is_empty() { backend } else { &a.model_pref };
        let msg = self.post_message(tid,"agent",Some(agent_id),"",json!({"backend":backend,"compact":true,"automatic":automatic,"phase":"сжатие контекста в очереди"}),MessageStatus::Queued,None)?;
        let s = self.create_session(agent_id,None,Some(tid),Some(&msg.id),"сжатие контекста",backend,&crate::workset::compact_prompt(extra))?;
        Ok(Some(s))
    }
}

impl Store {
    pub fn workspace_locations(&self, project: &str) -> Result<Value> {
        let p = self.get_project(project)?;
        let mut rows: Vec<Value> = self.setting(&format!("workspace_locations:{project}"))?.and_then(|s|serde_json::from_str(&s).ok()).unwrap_or_default();
        if !p.root_path.is_empty() && !rows.iter().any(|r|r["machine"].as_str().unwrap_or("")==p.node_id.as_deref().unwrap_or("")) {
            rows.push(json!({"machine":p.node_id.unwrap_or_default(),"path":p.root_path}));
        }
        Ok(json!(rows))
    }
    pub fn remember_workspace(&self, project: &str, machine: &str, path: &str) -> Result<()> {
        let mut rows = self.workspace_locations(project)?.as_array().cloned().unwrap_or_default();
        rows.retain(|r|r["machine"]!=machine);
        rows.push(json!({"machine":machine,"path":path}));
        self.set_setting(&format!("workspace_locations:{project}"),&json!(rows).to_string())
    }
}
