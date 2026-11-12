// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use senate_node::{default_data_dir, serve, Config, Handle};
use std::sync::Mutex;
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

struct Node(Mutex<Option<Handle>>);

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
        let _ = std::fs::write(remote_file(), serde_json::json!({"url": url, "token": token}).to_string());
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
            let (open_url, init): (tauri::Url, String) = match std::env::var("SENATE_UI_DEV") {
                Ok(dev) if !dev.is_empty() => (dev.parse()?, format!("window.__SENATE__ = {{ token: '{}', base: '{}' }};", token, node_base)),
                _ => (url, format!("window.__SENATE__ = {{ token: '{}' }};", token)),
            };
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(open_url))
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
