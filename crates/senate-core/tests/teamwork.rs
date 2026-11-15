use senate_core::{Store, model::*, engine::TransitionArgs, tools::{self, ToolCtx}};
use serde_json::json;

fn setup() -> (Store, Agent, Agent, Project) {
    let s = Store::open_in_memory().unwrap();
    let a = s.create_agent(NewAgent { name: "Author".into(), ..Default::default() }).unwrap();
    let b = s.create_agent(NewAgent { name: "Reviewer".into(), ..Default::default() }).unwrap();
    let p = s.create_project(NewProject { name: "Project".into(), goal: "Ship".into(), constraints: "Keep compatibility".into(), ..Default::default() }).unwrap();
    (s, a, b, p)
}
fn task(s: &Store, a: &Agent, p: &Project, name: &str, deps: Vec<String>, reviewer: Option<String>) -> Obligation {
    s.create_obligation(NewObligation { title: name.into(), result: "Working result".into(), owner_id: Some(a.id.clone()), project_id: Some(p.id.clone()), dependencies: deps, reviewer_id: reviewer, ..Default::default() }).unwrap()
}
fn done(s: &Store, id: &str) {
    s.transition(id, ObligationState::Done, TransitionArgs { outcome: Some("Delivered".into()), ..Default::default() }, "user").unwrap();
}

#[test]
fn waits_for_every_dependency_and_passes_their_results() {
    let (s,a,b,p) = setup();
    let x = task(&s,&a,&p,"Text",vec![],None);
    let y = task(&s,&b,&p,"Design",vec![],None);
    let z = task(&s,&a,&p,"Page",vec![x.id.clone(),y.id.clone()],None);
    s.transition(&z.id, ObligationState::Waiting, TransitionArgs { wait: Some(WaitCondition::Obligations { ids: z.dependencies.clone() }), ..Default::default() }, "user").unwrap();
    done(&s,&x.id);
    assert!(s.wake_due().unwrap().is_empty());
    assert!(s.transition(&z.id,ObligationState::Running,Default::default(),"user").is_err());
    done(&s,&y.id);
    assert_eq!(s.wake_due().unwrap(),vec![z.id.clone()]);
    let prompt = senate_core::workset::build_obligation_prompt(&s,&a,&s.get_obligation(&z.id).unwrap(),"test").unwrap();
    assert!(prompt.contains("Text") && prompt.contains("Design") && prompt.contains("Delivered"));
    assert_eq!(s.get_obligation(&z.id).unwrap().dependencies.len(),2);
}
#[test]
fn cancelled_prerequisite_blocks_and_can_be_explicitly_removed() {
    let (s,a,_,p) = setup();
    let x=task(&s,&a,&p,"X",vec![],None);
    let y=task(&s,&a,&p,"Y",vec![x.id.clone()],None);
    s.schedule("mock").unwrap();
    s.transition(&x.id,ObligationState::Cancelled,TransitionArgs { outcome:Some("Not needed".into()), ..Default::default() },"user").unwrap();
    s.wake_due().unwrap();
    assert_eq!(s.get_obligation(&y.id).unwrap().state,ObligationState::Blocked);
    s.configure_teamwork(&y.id,&[],None,"user").unwrap();
    s.transition(&y.id,ObligationState::Accepted,Default::default(),"user").unwrap();
    assert!(s.schedule("mock").unwrap().sessions.iter().any(|run| run.obligation_id.as_deref()==Some(&y.id)));
}
#[test]
fn rejects_cycles_missing_dependencies_and_self_review() {
    let (s,a,b,p)=setup();
    let x=task(&s,&a,&p,"X",vec![],Some(b.id.clone()));
    let y=task(&s,&a,&p,"Y",vec![x.id.clone()],None);
    assert!(s.configure_teamwork(&x.id,&[y.id.clone()],None,"user").is_err());
    assert!(s.configure_teamwork(&x.id,&["missing".into()],None,"user").is_err());
    assert!(s.configure_teamwork(&x.id,&[],Some(&a.id),"user").is_err());
    assert!(s.reassign_obligation(&x.id,&b.id,"user").is_err());
    assert!(s.get_obligation(&x.id).unwrap().dependencies.is_empty());
}
#[test]
fn reviewer_gets_one_run_and_author_cannot_accept_the_result() {
    let (s,a,b,p)=setup();
    let x=task(&s,&a,&p,"X",vec![],Some(b.id.clone()));
    let review=TransitionArgs { outcome:Some("Result".into()),evidence:Some("file.md; checks passed".into()),..Default::default() };
    assert!(s.transition(&x.id,ObligationState::Review,TransitionArgs { outcome:Some("Result".into()),..Default::default() },&a.id).is_err());
    s.transition(&x.id,ObligationState::Review,review,&a.id).unwrap();
    assert!(s.transition(&x.id,ObligationState::Done,Default::default(),&a.id).is_err());
    let runs=s.schedule("mock").unwrap().sessions;
    assert_eq!(runs.len(),1);
    assert_eq!(runs[0].agent_id,b.id);
    assert!(runs[0].prompt.contains("file.md"));
    assert!(s.schedule("mock").unwrap().sessions.is_empty());
    s.finish_session(&runs[0].id,SessionStatus::Done,"","","",0,0).unwrap();
    assert!(s.schedule("mock").unwrap().sessions.is_empty(),"must not loop when reviewer fails to decide");
    let ctx=ToolCtx {agent_id:b.id.clone(),project_id:Some(p.id.clone()),obligation_id:Some(x.id.clone()),..Default::default()};
    let result=tools::call(&s,&ctx,"senate_obligation_update",&json!({"state":"done","evidence":"Independently checked file.md"})).unwrap();
    assert_eq!(result["state"],"done");
}
#[test]
fn review_rejection_requires_actionable_notes_and_clears_old_evidence() {
    let (s,a,b,p)=setup();
    let x=task(&s,&a,&p,"X",vec![],Some(b.id.clone()));
    s.transition(&x.id,ObligationState::Review,TransitionArgs {outcome:Some("Result".into()),evidence:Some("Old evidence".into()),..Default::default()},&a.id).unwrap();
    assert!(s.transition(&x.id,ObligationState::Accepted,Default::default(),&b.id).is_err());
    let x=s.transition(&x.id,ObligationState::Accepted,TransitionArgs {note:"Fix empty input".into(),..Default::default()},&b.id).unwrap();
    assert!(x.evidence.is_empty() && x.outcome.is_empty());
    assert!(senate_core::workset::build_obligation_prompt(&s,&a,&x,"rework").unwrap().contains("Fix empty input"));
}
#[test]
fn stale_artifact_update_cannot_overwrite_colleague() {
    let (s,a,b,p)=setup();
    let doc=s.save_artifact(None,None,Some(&p.id),"document","Spec","v1",&a.id).unwrap();
    let changed=s.save_artifact(Some(&doc.id),Some(doc.version),Some(&p.id),"document","Spec","v2",&b.id).unwrap();
    assert!(s.save_artifact(Some(&doc.id),Some(doc.version),Some(&p.id),"document","Spec","stale",&a.id).is_err());
    assert!(s.save_artifact(Some(&doc.id),None,Some(&p.id),"document","Spec","unchecked",&a.id).is_err());
    assert_eq!(s.read_artifact(&doc.id,Some(&p.id)).unwrap().body,"v2");
    assert_eq!(changed.version,doc.version+1);
    assert!(s.read_artifact(&doc.id,None).is_err());
}
#[test]
fn colleague_receives_project_context_and_versioned_materials() {
    let (s,a,b,p)=setup();
    task(&s,&a,&p,"Main task",vec![],None);
    s.save_artifact(None,None,Some(&p.id),"document","Spec","body",&a.id).unwrap();
    let t=s.create_thread(NewThread {project_id:Some(p.id),agent_id:Some(a.id.clone()),..Default::default()}).unwrap();
    let call=s.call_colleague(&a,&t.id,&b.slug,"Please inspect",0,"mock").unwrap();
    let run=s.get_session(call["session_id"].as_str().unwrap()).unwrap();
    assert!(run.prompt.contains("Main task") && run.prompt.contains("Spec") && run.prompt.contains("Keep compatibility"));
}

#[test]
fn shared_and_nested_folders_are_serialized_but_other_folders_run_in_parallel() {
    let (s,a,b,_) = setup();
    let p = s.create_project(NewProject {name:"Shared".into(),root_path:"C:/team/shared".into(),..Default::default()}).unwrap();
    let nested = s.create_project(NewProject {name:"Nested".into(),root_path:"c:\\TEAM\\shared\\src".into(),..Default::default()}).unwrap();
    let separate = s.create_project(NewProject {name:"Other".into(),root_path:"C:/team/other".into(),..Default::default()}).unwrap();
    let mut runs=vec![];
    for (project,agent) in [(&p,&a),(&nested,&b),(&separate,&b)] {
        let t=s.create_thread(NewThread {project_id:Some(project.id.clone()),..Default::default()}).unwrap();
        runs.push(s.create_session(&agent.id,None,Some(&t.id),None,"test","mock","").unwrap());
    }
    assert_eq!(s.claim_session("node",&|_|true).unwrap().unwrap().id,runs[0].id);
    assert_eq!(s.claim_session("node",&|_|true).unwrap().unwrap().id,runs[2].id);
    assert!(s.claim_session("node",&|_|true).unwrap().is_none());
    s.finish_session(&runs[0].id,SessionStatus::Done,"","","",0,0).unwrap();
    assert_eq!(s.claim_session("node",&|_|true).unwrap().unwrap().id,runs[1].id);
}
#[test]
fn only_recipient_can_acknowledge_an_urgent_note() {
    let (s,a,b,_)=setup();
    let id=s.add_note(None,&a.id,&b.id,"Stop conflicting edit",true).unwrap();
    assert!(s.acknowledge_note(&id,&a.id).is_err());
    assert_eq!(s.take_notes(&b.id).unwrap().len(),1);
    assert_eq!(s.take_notes(&b.id).unwrap().len(),1);
    assert!(s.sent_notes(&a.id,10).unwrap()[0]["acknowledged_at"].is_null());
    s.acknowledge_note(&id,&b.id).unwrap();
    assert!(s.take_notes(&b.id).unwrap().is_empty());
}

#[test]
fn migration_preserves_legacy_single_dependency_and_is_repeatable() {
    let db=rusqlite::Connection::open_in_memory().unwrap();
    for migration in &senate_core::schema::MIGRATIONS[..12] { db.execute_batch(migration).unwrap(); }
    db.pragma_update(None,"user_version",12).unwrap();
    db.execute("INSERT INTO obligations(id,title,state,wait_condition,created_at,updated_at) VALUES ('a','A','waiting',?,1,1)",[json!({"type":"obligation","id":"b"}).to_string()]).unwrap();
    senate_core::schema::migrate(&db).unwrap();
    senate_core::schema::migrate(&db).unwrap();
    let deps:String=db.query_row("SELECT dependencies FROM obligations WHERE id='a'",[],|r|r.get(0)).unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&deps).unwrap(),json!(["b"]));
}
#[test]
fn queued_task_cannot_run_after_dependencies_change() {
    let (s,a,_,p)=setup();
    let x=task(&s,&a,&p,"X",vec![],None);
    let y=task(&s,&a,&p,"Y",vec![],None);
    let runs=s.schedule("mock").unwrap().sessions;
    let old=runs.iter().find(|run|run.obligation_id.as_deref()==Some(&x.id)).unwrap();
    s.configure_teamwork(&x.id,&[y.id.clone()],None,"user").unwrap();
    let claimed=s.claim_session("node",&|_|true).unwrap().unwrap();
    assert_eq!(claimed.obligation_id.as_deref(),Some(y.id.as_str()));
    assert_eq!(s.get_session(&old.id).unwrap().status,SessionStatus::Cancelled);
}
