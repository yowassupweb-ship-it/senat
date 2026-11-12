//! A CLI session still held by another process must not fail the turn.

use senate_runtime::{run, Approver, Decision, PermissionMode, PermissionRequest, RunSpec};
use std::future::Future;
use std::pin::Pin;

struct AllowAll;
impl Approver for AllowAll {
    fn ask(&self, _: PermissionRequest) -> Pin<Box<dyn Future<Output = Decision> + Send + '_>> {
        Box::pin(async { Decision::Allow })
    }
}

fn spec(resume: Option<&str>) -> RunSpec {
    let root = std::env::temp_dir().join(format!("senate-resume-{}", senate_core::new_id()));
    std::fs::create_dir_all(&root).unwrap();
    RunSpec {
        backend: "mock".into(),
        effort: None,
        mode: PermissionMode::Edit,
        cwd: root.clone(),
        system_prompt: "Ты — разработчик.".into(),
        prompt: "СЕССИЯ-ЗАНЯТА: ответь коротко".into(),
        resume: resume.map(String::from),
        mcp: None,
        extra_mcp: vec![],
        env: vec![],
        bare: false,
        work_dir: root.join(".run"),
        run_id: "r1".into(),
    }
}

#[tokio::test]
async fn a_held_session_is_retried_and_then_replaced_by_a_fresh_one() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let (_c, crx) = tokio::sync::watch::channel(false);
    let t = std::time::Instant::now();
    let out = run(spec(Some("held-session")), &AllowAll, tx, crx).await;
    assert!(out.failure.is_none(), "the turn goes on in a fresh session: {:?}", out.failure);
    assert!(!out.text.is_empty());
    assert!(t.elapsed() >= std::time::Duration::from_secs(4), "it waited before giving up the held session");
}

#[tokio::test]
async fn without_a_resume_nothing_is_retried() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let (_c, crx) = tokio::sync::watch::channel(false);
    let t = std::time::Instant::now();
    let out = run(spec(None), &AllowAll, tx, crx).await;
    assert!(out.failure.is_none());
    assert!(t.elapsed() < std::time::Duration::from_secs(4));
}
