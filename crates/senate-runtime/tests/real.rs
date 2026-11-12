//! Real-CLI checks. They use the logged-in agents of this machine, so they are ignored by default:
//!   cargo test -p senate-runtime --test real -- --ignored --nocapture

use senate_runtime::{run, Approver, Decision, Event, PermissionMode, PermissionRequest, RunSpec};
use std::future::Future;
use std::pin::Pin;

struct AllowAll;
impl Approver for AllowAll {
    fn ask(&self, req: PermissionRequest) -> Pin<Box<dyn Future<Output = Decision> + Send + '_>> {
        println!("  ASK {} - {}", req.tool, req.reason);
        Box::pin(async { Decision::Allow })
    }
}

async fn go(backend: &str, mode: PermissionMode) {
    let root = std::env::temp_dir().join(format!("senate-real-{}-{}", backend.replace(':', "_"), senate_core::new_id()));
    std::fs::create_dir_all(&root).unwrap();
    let spec = RunSpec {
        backend: backend.into(),
        effort: Some("low".into()),
        mode,
        cwd: root.clone(),
        system_prompt: "Ты — разработчик. Отвечай кратко.".into(),
        prompt: "Создай в текущей папке файл hello.txt со словом привет и ответь одним словом: готово.".into(),
        resume: None,
        mcp: None,
        extra_mcp: vec![],
        env: vec![],
        bare: false,
        work_dir: root.join(".run"),
        run_id: "t1".into(),
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (_c, crx) = tokio::sync::watch::channel(false);
    let drain = tokio::spawn(async move {
        let mut n = 0;
        while let Some(e) = rx.recv().await {
            if let Event::Ask(_) = e {
                n += 1
            }
        }
        n
    });
    let out = run(spec, &AllowAll, tx, crx).await;
    let asks = drain.await.unwrap();
    println!("[{backend}] text={:?} failure={:?} session={:?} steps={} asks={asks} tokens(in/out)={}/{}", out.text, out.failure, out.session_id, out.steps.len(), out.work.input_tokens, out.work.output_tokens);
    for s in &out.steps {
        println!("   step {} | {} | {} | {}", s.kind, s.name, s.hint, s.status);
    }
    assert!(out.failure.is_none(), "{:?}", out.failure);
    let file = std::fs::read_to_string(root.join("hello.txt")).unwrap_or_default();
    println!("   hello.txt = {file:?}");
    assert!(file.to_lowercase().contains("привет"), "file not created");
}

#[tokio::test]
#[ignore]
async fn codex_edits_files_in_the_project_folder() {
    go("codex", PermissionMode::Edit).await;
}

#[tokio::test]
#[ignore]
async fn kimi_edits_files_through_acp_permissions() {
    go("kimi", PermissionMode::Ask).await;
}

#[tokio::test]
#[ignore]
async fn kimi_model_catalog_comes_from_kimi() {
    let m = senate_runtime::kimi::catalog().await.expect("catalog");
    println!("{m:?}");
    assert!(!m.is_empty());
}

#[tokio::test]
#[ignore]
async fn model_lists_come_from_the_providers_and_the_account_is_known() {
    for p in ["claude", "codex", "kimi"] {
        let acc = senate_runtime::auth::status(p).await;
        let models = senate_runtime::auth::models(p).await;
        let latest = senate_runtime::auth::latest_version(p).await;
        println!("[{p}] logged_in={} models={} latest_cli={latest:?}", acc.logged_in, models.len());
        for m in models.iter().take(12) {
            println!("    {} | {}", m["id"], m["label"]);
        }
        assert!(acc.logged_in, "{p} is not logged in on this machine");
        assert!(!models.is_empty(), "{p}: empty model list");
    }
}
