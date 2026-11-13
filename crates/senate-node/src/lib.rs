//! Senate node: the process that owns the data, runs agents and serves the API (and the web UI).
//! The desktop app embeds it; on a server it runs headless. Either way clients speak the same API.

mod accounts;
mod api;
mod executor;
mod external;
mod forum;
mod images;
mod permissions;
mod skills;
pub mod mcp;
pub mod mcp_http;
pub mod runner;
pub mod s3;
mod project_git;
mod reflect;
mod uploads;
mod preview;
mod usage;
mod workspace;

use senate_core::tools::ToolCtx;
use senate_core::Store;
use senate_runtime::Decision;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::{broadcast, oneshot, watch, Notify};

#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub bind: SocketAddr,
    /// API token; generated and persisted on first start when `None`.
    pub token: Option<String>,
    /// Backend used when neither chat, specialist nor project says otherwise. `None` = best available.
    pub default_backend: Option<String>,
    pub parallel: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config { data_dir: default_data_dir(), bind: "127.0.0.1:7777".parse().unwrap(), token: None, default_backend: None, parallel: 4 }
    }
}

pub fn default_data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("SENATE_HOME") {
        return PathBuf::from(p);
    }
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    base.unwrap_or_else(|| PathBuf::from(".")).join("Senate")
}

pub(crate) struct Pending {
    pub tx: oneshot::Sender<Decision>,
    pub tool_use_id: String,
    /// What "allow always" would remember for this question (None: it is always asked about).
    pub rule: Option<permissions::Candidate>,
    /// What "allow work in this folder" would remember (None: not about a folder, or dangerous).
    pub dir: Option<permissions::Candidate>,
}

pub struct App {
    pub store: Store,
    pub cfg: Config,
    pub token: String,
    pub node_id: String,
    pub port: RwLock<u16>,
    /// Server-sent events for connected clients (JSON strings).
    pub bus: broadcast::Sender<String>,
    pub(crate) pending: Mutex<HashMap<String, Pending>>,
    pub(crate) cancels: Mutex<HashMap<String, watch::Sender<bool>>>,
    /// Per-session tokens for the MCP bridge: a token opens exactly one session's tools.
    pub(crate) session_tokens: Mutex<HashMap<String, ToolCtx>>,
    pub(crate) wake: Notify,
    pub caps: RwLock<Vec<serde_json::Value>>,
    /// Agent CLIs found on this machine (login state lives in `accounts`).
    pub(crate) installed: RwLock<Vec<serde_json::Value>>,
    pub(crate) accounts: Mutex<HashMap<String, accounts::AccountState>>,
    pub(crate) logins: Mutex<HashMap<String, senate_runtime::auth::LoginHandle>>,
    pub(crate) limits: RwLock<HashMap<String, serde_json::Value>>,
    /// Latest published version of each agent CLI.
    pub(crate) latest: RwLock<HashMap<String, String>>,
    /// Forums with a live orchestrator.
    pub(crate) forums: Mutex<std::collections::HashSet<String>>,
    /// Specialists who work on a person's computer: the queue of their jobs.
    pub runner: runner::Hub,
    /// The computer this node runs on (the same id its app uses there), so a project folder kept here is read here.
    pub machine_id: String,
    /// Computers already checked for the folders of projects that name no computer.
    pub(crate) matched: Mutex<std::collections::HashSet<String>>,
    /// Opens `/preview/<port>/` pages (see preview.rs); new on every start.
    pub(crate) preview_key: String,
}

impl App {
    pub fn publish(&self, kind: &str, payload: serde_json::Value) {
        let _ = self.bus.send(serde_json::json!({"type": kind, "data": payload}).to_string());
    }
    /// A token that opens exactly one session's tools over the MCP bridge (also used by tests).
    pub fn mint_session_token(&self, ctx: ToolCtx) -> String {
        let t = format!("sst_{}", uuid::Uuid::new_v4().simple());
        self.session_tokens.lock().unwrap().insert(t.clone(), ctx);
        t
    }

    /// The specialist's main model is set once and kept. If it cannot run right now (not logged in,
    /// CLI gone) the work still happens on the default backend — and says so — instead of failing.
    pub fn resolve_backend(&self, wanted: &str) -> (String, Option<String>) {
        let provider = wanted.split(':').next().unwrap_or("");
        let caps = self.caps.read().unwrap();
        let ready = |p: &str| caps.iter().any(|c| c["backend"] == p && c["ready"] == true);
        if provider == "mock" || ready(provider) {
            return (wanted.to_string(), None);
        }
        drop(caps);
        let fallback = self.backend_default();
        let why = format!("Основная модель специалиста ({wanted}) сейчас недоступна — отвечает {fallback}. Проверьте вход в Настройках.");
        (fallback, Some(why))
    }

    pub fn poke(&self) {
        self.wake.notify_one();
    }
    pub fn backend_default(&self) -> String {
        if let Some(b) = &self.cfg.default_backend {
            return b.clone();
        }
        if let Ok(Some(b)) = self.store.setting("default_backend") {
            return b;
        }
        let caps = self.caps.read().unwrap();
        caps.iter()
            .find(|c| c["ready"] == true && c["backend"] != "mock")
            .or_else(|| caps.iter().find(|c| c["backend"] == "mock"))
            .and_then(|c| c["backend"].as_str().map(String::from))
            .unwrap_or_else(|| "mock".into())
    }
    pub fn work_dir(&self) -> PathBuf {
        self.cfg.data_dir.join("run")
    }
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", *self.port.read().unwrap())
    }
}

pub struct Handle {
    pub app: Arc<App>,
    pub addr: SocketAddr,
    pub token: String,
    shutdown: Option<oneshot::Sender<()>>,
    join: tokio::task::JoinHandle<()>,
}

impl Handle {
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }
    pub async fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = self.join.await;
    }
}

fn load_or_create_token(dir: &std::path::Path) -> String {
    let f = dir.join("token");
    if let Ok(t) = std::fs::read_to_string(&f) {
        let t = t.trim().to_string();
        if t.len() >= 32 {
            return t;
        }
    }
    let t = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(&f, &t);
    t
}

/// Start the node: open the store, start the executor and the HTTP server.
/// If the preferred port is taken, the next free one is used (the desktop app asks the handle).
pub async fn serve(cfg: Config) -> Result<Handle, String> {
    std::fs::create_dir_all(&cfg.data_dir).map_err(|e| format!("папка данных: {e}"))?;
    let store = Store::open(cfg.data_dir.join("senate.db")).map_err(|e| e.to_string())?;
    store.seed_defaults().map_err(|e| e.to_string())?;
    let token = cfg.token.clone().unwrap_or_else(|| load_or_create_token(&cfg.data_dir));

    let node_id = match store.setting("node_id").map_err(|e| e.to_string())? {
        Some(i) => i,
        None => {
            let i = senate_core::new_id();
            store.set_setting("node_id", &i).map_err(|e| e.to_string())?;
            i
        }
    };

    let installed = senate_runtime::discover().await;
    let caps: Vec<serde_json::Value> = vec![serde_json::json!({"backend": "mock", "label": "Демо (без модели)", "version": "1", "ready": true, "models": [{"id": "", "label": "Демо"}]})];
    let host = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "Этот компьютер".into());
    store
        .upsert_node(&node_id, &host, std::env::consts::OS, "local", "", &serde_json::json!(caps))
        .map_err(|e| e.to_string())?;

    // Bind before anything else so a busy port fails fast and we can fall back.
    let mut bind = cfg.bind;
    let listener = loop {
        match tokio::net::TcpListener::bind(bind).await {
            Ok(l) => break l,
            Err(e) if bind.port() < cfg.bind.port().saturating_add(20) && e.kind() == std::io::ErrorKind::AddrInUse => {
                bind.set_port(bind.port() + 1);
            }
            Err(e) => return Err(format!("не удалось занять {bind}: {e}")),
        }
    };
    let addr = listener.local_addr().map_err(|e| e.to_string())?;

    let (bus, _) = broadcast::channel(512);
    let app = Arc::new(App {
        store,
        cfg: cfg.clone(),
        token: token.clone(),
        node_id,
        port: RwLock::new(addr.port()),
        bus,
        pending: Mutex::new(HashMap::new()),
        cancels: Mutex::new(HashMap::new()),
        session_tokens: Mutex::new(HashMap::new()),
        wake: Notify::new(),
        caps: RwLock::new(caps),
        installed: RwLock::new(installed),
        accounts: Mutex::new(HashMap::new()),
        logins: Mutex::new(HashMap::new()),
        limits: RwLock::new(HashMap::new()),
        latest: RwLock::new(HashMap::new()),
        forums: Mutex::new(Default::default()),
        runner: Default::default(),
        machine_id: workspace::machine_identity(&cfg.data_dir).0,
        matched: Mutex::new(Default::default()),
        preview_key: preview::new_key(),
    });
    accounts::rebuild_caps(&app);
    // Who is logged in is learned in the background: the window must open at once.
    {
        let a = app.clone();
        tokio::spawn(async move { accounts::refresh_all(&a).await });
    }
    accounts::spawn_refresher(&app);
    usage::spawn(&app);

    executor::recover(&app);
    forum::resume_all(&app);
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let exec_app = app.clone();
    let (exec_stop_tx, exec_stop_rx) = watch::channel(false);
    let exec = tokio::spawn(async move { executor::run_loop(exec_app, exec_stop_rx).await });

    let router = api::router(app.clone());
    let join = tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = stop_rx.await;
            })
            .await;
        let _ = exec_stop_tx.send(true);
        let _ = exec.await;
    });
    tracing::info!("Senate node слушает на http://{addr}");
    Ok(Handle { app, addr, token, shutdown: Some(stop_tx), join })
}
