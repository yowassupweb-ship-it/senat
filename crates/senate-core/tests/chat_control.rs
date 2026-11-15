use senate_core::{Store, model::*, tools::{self,ToolCtx}};
use serde_json::json;
fn setup() -> (Store, Agent, Thread) {
    let s=Store::open_in_memory().unwrap();
    let a=s.create_agent(NewAgent { name:"Writer".into(),..Default::default() }).unwrap();
    let t=s.create_thread(NewThread { agent_id:Some(a.id.clone()),..Default::default() }).unwrap();
    (s,a,t)
}
#[test]
fn automatic_compaction_requires_measurement_and_deduplicates() {
    let (s,a,t)=setup();
    assert!(s.queue_chat_compact(&t.id,&a.id,"mock","",true).unwrap().is_none());
    let m=s.post_message(&t.id,"agent",Some(&a.id),"Result",json!({"work":{"context_tokens":81,"context_window":100}}),MessageStatus::Done,None).unwrap();
    let next=s.queue_chat_compact(&t.id,&a.id,"mock","",true).unwrap().unwrap();
    assert!(s.get_message(next.message_id.as_ref().unwrap()).unwrap().props["compact"]==true);
    assert!(s.queue_chat_compact(&t.id,&a.id,"mock","",true).unwrap().is_none());
    assert_eq!(s.get_message(&m.id).unwrap().body,"Result");
    assert_eq!(s.chat_context(&t.id).unwrap()["pending"],1);
}
#[test]
fn summary_resets_meter_without_deleting_history_and_auto_can_be_disabled() {
    let (s,a,t)=setup();
    s.post_message(&t.id,"agent",Some(&a.id),"Original",json!({"work":{"context_tokens":90,"context_window":100}}),MessageStatus::Done,None).unwrap();
    s.set_forum_meta(&t.id,&json!({"context_auto":false})).unwrap();
    assert!(s.queue_chat_compact(&t.id,&a.id,"mock","",true).unwrap().is_none());
    s.set_forum_meta(&t.id,&json!({"context_auto":true})).unwrap();
    s.set_compact_summary(&t.id,&a.id,"Goals and unfinished work",1).unwrap();
    let state=s.chat_context(&t.id).unwrap();
    assert_eq!(state["participants"][0]["compacted"],true);
    assert!(state["participants"][0]["tokens"].is_null());
    assert!(s.queue_chat_compact(&t.id,&a.id,"mock","",true).unwrap().is_none());
    assert_eq!(s.list_messages(&t.id,None,50).unwrap()[0].body,"Original");
    assert_eq!(Store::compact_summary(&s.get_thread(&t.id).unwrap(),&a.id).unwrap(),"Goals and unfinished work");
}
#[test]
fn assistant_requests_compaction_after_its_turn_and_can_read_history() {
    let (s,a,t)=setup();
    let m=s.post_message(&t.id,"agent",Some(&a.id),"Working",json!({}),MessageStatus::Working,None).unwrap();
    let run=s.create_session(&a.id,None,Some(&t.id),Some(&m.id),"test","mock","hello").unwrap();
    let ctx=ToolCtx { agent_id:a.id.clone(),thread_id:Some(t.id.clone()),session_id:Some(run.id),..Default::default() };
    let result=tools::call(&s,&ctx,"senate_chat_context",&json!({"action":"compact","instructions":"Keep pending tests"})).unwrap();
    assert_eq!(result["status"],"after_this_turn");
    assert_eq!(s.get_message(&m.id).unwrap().props["compact_requested"],"Keep pending tests");
    assert_eq!(s.chat_context(&t.id).unwrap()["pending"],0);
    let result=tools::call(&s,&ctx,"senate_chat_context",&json!({"action":"history"})).unwrap();
    assert_eq!(result["messages"][0]["body"],"Working");
    tools::call(&s,&ctx,"senate_chat_context",&json!({"action":"configure","auto":false})).unwrap();
    assert_eq!(s.chat_context(&t.id).unwrap()["auto"],false);
}
#[test]
fn history_cursor_does_not_skip_same_timestamp_messages() {
    let (s,_,t)=setup();
    for i in 0..60 { s.post_message(&t.id,"user",None,&format!("Message {i}"),json!({}),MessageStatus::Done,None).unwrap(); }
    let first=s.list_messages(&t.id,None,30).unwrap();
    let second=s.list_messages(&t.id,Some(&first[0].id),30).unwrap();
    assert_eq!(first.len()+second.len(),60);
    assert!(!second.iter().any(|m|first.iter().any(|x|x.id==m.id)));
}

#[test]
fn compaction_precedes_queued_user_turns() {
    let (s,a,t)=setup();
    let m=s.post_message(&t.id,"agent",Some(&a.id),"",json!({}),MessageStatus::Queued,None).unwrap();
    let normal=s.create_session(&a.id,None,Some(&t.id),Some(&m.id),"user","mock","pending user request").unwrap();
    let compact=s.queue_chat_compact(&t.id,&a.id,"mock","",false).unwrap().unwrap();
    let claimed=s.claim_session("test",&|_|true).unwrap().unwrap();
    assert_eq!(claimed.id,compact.id);
    assert_eq!(s.get_session(&normal.id).unwrap().status,SessionStatus::Queued);
    assert!(s.claim_session("test",&|_|true).unwrap().is_none());
}

#[test]
fn concurrent_participants_keep_each_others_summaries_and_sessions() {
    let (s,_,t)=setup();
    let store=std::sync::Arc::new(s);
    let barrier=std::sync::Arc::new(std::sync::Barrier::new(12));
    let handles:Vec<_>=(0..12).map(|i| {
        let store=store.clone(); let barrier=barrier.clone(); let tid=t.id.clone();
        std::thread::spawn(move || {
            let id=format!("participant-{i}"); barrier.wait();
            store.set_cli_session_for(&tid,"codex",&id,"session").unwrap();
            store.set_compact_summary(&tid,&id,"summary",10).unwrap();
        })
    }).collect();
    for h in handles { h.join().unwrap(); }
    let t=store.get_thread(&t.id).unwrap();
    assert_eq!(t.meta["compact"].as_object().unwrap().len(),12);
    assert_eq!(t.cli_sessions.as_object().unwrap().len(),12);
    store.clear_cli_session_for(&t.id,"codex","participant-0").unwrap();
    assert_eq!(store.get_thread(&t.id).unwrap().cli_sessions.as_object().unwrap().len(),11);
}

#[test]
fn workspace_paths_are_scoped_by_machine_and_running_places_do_not_move() {
    use senate_core::store::ProjectPatch;
    let (s,a,_) = setup();
    let p=s.create_project(NewProject {name:"Locations".into(),root_path:"/old/project".into(),..Default::default()}).unwrap();
    s.remember_workspace(&p.id,"laptop","C:/work/project").unwrap();
    s.remember_workspace(&p.id,"desktop","D:/work/project").unwrap();
    s.remember_workspace(&p.id,"laptop","C:/new/project").unwrap();
    let locations=s.workspace_locations(&p.id).unwrap();
    assert_eq!(locations.as_array().unwrap().len(),3);
    assert_eq!(locations.as_array().unwrap().iter().find(|r|r["machine"]=="laptop").unwrap()["path"],"C:/new/project");
    let t=s.create_thread(NewThread {project_id:Some(p.id.clone()),..Default::default()}).unwrap();
    let m=s.post_message(&t.id,"agent",Some(&a.id),"",json!({}),MessageStatus::Queued,None).unwrap();
    s.create_session(&a.id,None,Some(&t.id),Some(&m.id),"user","mock","work").unwrap();
    let run=s.claim_session("test",&|_|true).unwrap().unwrap();
    let before=s.session_workplace(&run).unwrap();
    s.update_project(&p.id,ProjectPatch{root_path:Some("/different/project".into()),..Default::default()}).unwrap();
    assert_eq!(s.session_workplace(&run).unwrap(),before);
    assert_eq!(before["captured"],true);
}
