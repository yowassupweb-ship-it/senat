// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use senate_node::{default_data_dir, serve, Config, Handle};
use std::sync::Mutex;
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

struct Node(Mutex<Option<Handle>>);
/// Held so the runner loop stops with the app.
struct RunnerStop(#[allow(dead_code)] tokio::sync::watch::Sender<bool>);

/// `senate-desktop --remote https://host --token T` remembers a remote node and opens it instead of
/// starting a local one; `--local` goes back. The remote node serves the same UI.
fn remote_file() -> std::path::PathBuf {
    default_data_dir().join("remote.json")
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn read_remote() -> Option<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(remote_file()).ok()?).ok()?;
    Some((v["url"].as_str()?.trim_end_matches('/').to_string(), v["token"].as_str()?.to_string()))
}

fn remote_name() -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(remote_file()).ok()?).ok()?;
    v["name"].as_str().map(str::to_string)
}

/// Open a link in the default browser of THIS computer. The node may live on a server, where a browser
/// opened on request would be useless, so the window does it itself.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    let u = url.trim();
    if !(u.starts_with("http://") || u.starts_with("https://")) || u.len() > 2000 || u.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("можно открыть только http(s)-ссылку".into());
    }
    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", u]).creation_flags(0x0800_0000);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(u);
        c
    };
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(u);
        c
    };
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

fn main() {
    // Agent CLIs launch this same executable as their MCP bridge (`senate-desktop mcp`).
    if std::env::args().nth(1).as_deref() == Some("mcp") {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        rt.block_on(senate_node::mcp::run_stdio());
        return;
    }

    let args: Vec<String> = std::env::args().collect();
    if let Some(url) = flag(&args, "--remote") {
        let token = flag(&args, "--token").unwrap_or_default();
        let _ = std::fs::create_dir_all(default_data_dir());
        let name = flag(&args, "--name").unwrap_or_else(|| "Серверная нода".into());
        let _ = std::fs::write(remote_file(), serde_json::json!({"url": url, "token": token, "name": name}).to_string());
    }
    if args.iter().any(|a| a == "--local") {
        let _ = std::fs::remove_file(remote_file());
    }

    let app = tauri::Builder::default()
        // One instance per user: two nodes would fight over the database and the port.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .invoke_handler(tauri::generate_handler![open_url])
        .setup(|app| {
            let (url, token): (tauri::Url, String) = if let Some((u, t)) = read_remote() {
                (u.parse()?, t)
            } else {
                let mut cfg = Config::default();
                // Desktop uses its own port so it does not clash with a headless node on 7777.
                cfg.bind = "127.0.0.1:7790".parse().unwrap();
                let handle = tauri::async_runtime::block_on(serve(cfg)).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
                let out = (handle.url().parse()?, handle.token.clone());
                app.manage(Node(Mutex::new(Some(handle))));
                out
            };
            // Dev mode: show the Vite dev server (hot module reload) instead of the embedded UI; the page
            // still talks to this node, which is why its address and token are injected.
            let node_base = url.as_str().trim_end_matches('/').to_string();
            // Connected to a remote node: this app is also the place where "local" specialists do their work.
            if read_remote().is_some() {
                let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
                app.manage(RunnerStop(stop_tx));
                tauri::async_runtime::spawn(senate_node::runner::run_client(node_base.clone(), token.clone(), default_data_dir(), stop_rx));
            }
            let dev = std::env::var("SENATE_UI_DEV").ok().filter(|d| !d.is_empty());
            let label = if read_remote().is_some() { remote_name().unwrap_or_else(|| "Серверная нода".into()) } else { "Локальная нода".into() };
            let base_init = |chrome: bool| format!("window.__SENATE__ = {{ token: '{}', base: '{}', chrome: {}, name: {:?}, remote: {} }};", token, node_base, chrome, label, read_remote().is_some());
            // Dev: the Vite page (external, no native IPC) with the system frame. Otherwise: the UI bundled with
            // this app, in a frameless window whose header (logo, drag area, window buttons) is drawn by the UI.
            let (target, init, frame): (WebviewUrl, String, bool) = match dev {
                Some(d) => (WebviewUrl::External(d.parse()?), base_init(false), true),
                None => (WebviewUrl::App("index.html".into()), base_init(true), false),
            };
            let _ = url;
            WebviewWindowBuilder::new(app, "main", target)
                .decorations(frame)
                .title("Senate")
                .inner_size(1360.0, 860.0)
                .min_inner_size(900.0, 560.0)
                .initialization_script(&init)
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("не удалось создать приложение");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            // Stop the node cleanly so SQLite checkpoints its WAL.
            if let Some(h) = handle.try_state::<Node>().and_then(|n| n.0.lock().ok().and_then(|mut g| g.take())) {
                tauri::async_runtime::block_on(h.stop());
            }
        }
    });
}
