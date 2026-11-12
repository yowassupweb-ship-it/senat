// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod tunnel;

use senate_node::{default_data_dir, serve, Config, Handle};
use std::sync::Mutex;
use tauri::{Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder};

struct Node(Mutex<Option<Handle>>);
/// The tunnel to a server node, when the app owns one. Dropping it (at exit) stops the supervisor and `ssh`.
struct Link(Option<tunnel::Tunnel>);
/// A settings file that exists but cannot be used. Shown on the waiting screen instead of starting a local node.
struct Problem(Option<String>);
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

/// The remote settings: `Ok(None)` when there is no file (a local node is wanted), `Err` when there is one and it
/// cannot be used. That case must never quietly turn into a local node: the person would be working in another
/// database without knowing it. Windows tools like to put a byte-order mark in front of the text; it is ignored.
fn remote_json() -> Result<Option<serde_json::Value>, String> {
    let path = remote_file();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Не удаётся прочитать настройки подключения к серверу ({e}).")),
    };
    let v: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("Файл настроек подключения к серверу повреждён ({e}). Он лежит здесь: {}", path.display()))?;
    if v["url"].as_str().is_none() || v["token"].as_str().is_none() {
        return Err(format!("В настройках подключения к серверу нет адреса или токена: {}", path.display()));
    }
    Ok(Some(v))
}

fn read_remote() -> Option<(String, String)> {
    let v = remote_json().ok()??;
    Some((v["url"].as_str()?.trim_end_matches('/').to_string(), v["token"].as_str()?.to_string()))
}

/// `"ssh": {"user", "host", "key", "remote_port"}` in remote.json: the node is on a server and reached by tunnel.
fn read_ssh() -> Option<tunnel::Spec> {
    let v = remote_json().ok()??;
    let s = v.get("ssh")?;
    Some(tunnel::Spec {
        user: s["user"].as_str().unwrap_or("root").to_string(),
        host: s["host"].as_str()?.to_string(),
        key: std::path::PathBuf::from(s["key"].as_str()?),
        remote_port: s["remote_port"].as_u64().unwrap_or(17777) as u16,
    })
}

/// For the screen: is the node reachable, and if not, why. Without a tunnel there is nothing to report.
#[tauri::command]
fn link_state(link: State<'_, Link>, problem: State<'_, Problem>) -> tunnel::LinkState {
    if let Some(p) = &problem.0 {
        return tunnel::LinkState { up: false, detail: p.clone(), restarts: 0 };
    }
    match &link.0 {
        Some(t) => t.state.lock().map(|s| s.clone()).unwrap_or_default(),
        None => tunnel::LinkState { up: true, detail: String::new(), restarts: 0 },
    }
}

fn remote_name() -> Option<String> {
    let v = remote_json().ok()??;
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
    // `--ssh root@host --key <file> --token T [--remote-port 17777] [--name N]`: a node on a server that is reached
    // through a tunnel the app keeps alive by itself. The address below is only a placeholder: the real local port
    // is chosen at every start.
    if let Some(target) = flag(&args, "--ssh") {
        let (user, host) = target.split_once('@').map(|(u, h)| (u.to_string(), h.to_string())).unwrap_or(("root".into(), target.clone()));
        let key = flag(&args, "--key").unwrap_or_default();
        let _ = std::fs::create_dir_all(default_data_dir());
        let name = flag(&args, "--name").unwrap_or_else(|| "Серверная нода".into());
        let remote_port: u16 = flag(&args, "--remote-port").and_then(|p| p.parse().ok()).unwrap_or(17777);
        let token = flag(&args, "--token").unwrap_or_default();
        let _ = std::fs::write(
            remote_file(),
            serde_json::json!({"url": "http://127.0.0.1:7777", "token": token, "name": name, "ssh": {"user": user, "host": host, "key": key, "remote_port": remote_port}}).to_string(),
        );
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
        .invoke_handler(tauri::generate_handler![open_url, link_state])
        .setup(|app| {
            let mut tunnel_up = None;
            let problem = remote_json().err();
            let (url, token): (tauri::Url, String) = if problem.is_some() {
                // Settings exist but are broken: no local node in their place; the window explains and keeps waiting.
                ("http://127.0.0.1:1".parse()?, String::new())
            } else if let Some((u, t)) = read_remote() {
                if let Some(spec) = read_ssh() {
                    // The node is on a server: build the tunnel first, and give it a moment, so the window opens on a
                    // live link. If the server cannot be reached, the window still opens and says why while it retries.
                    let preferred = u.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(7777);
                    let link = tunnel::start(spec, preferred);
                    let port = link.port;
                    let _ = tauri::async_runtime::block_on(tunnel::wait_up(port, std::time::Duration::from_secs(12)));
                    tunnel_up = Some(link);
                    (format!("http://127.0.0.1:{port}").parse()?, t)
                } else {
                    (u.parse()?, t)
                }
            } else {
                let mut cfg = Config::default();
                // Desktop uses its own port so it does not clash with a headless node on 7777.
                cfg.bind = "127.0.0.1:7790".parse().unwrap();
                let handle = tauri::async_runtime::block_on(serve(cfg)).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
                let out = (handle.url().parse()?, handle.token.clone());
                app.manage(Node(Mutex::new(Some(handle))));
                out
            };
            let managed = tunnel_up.is_some() || problem.is_some();
            app.manage(Link(tunnel_up));
            app.manage(Problem(problem.clone()));
            // Dev mode: show the Vite dev server (hot module reload) instead of the embedded UI; the page
            // still talks to this node, which is why its address and token are injected.
            let node_base = url.as_str().trim_end_matches('/').to_string();
            // Connected to a remote node: this app is also the place where "local" specialists do their work.
            if read_remote().is_some() && problem.is_none() {
                let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
                app.manage(RunnerStop(stop_tx));
                tauri::async_runtime::spawn(senate_node::runner::run_client(node_base.clone(), token.clone(), default_data_dir(), stop_rx));
            }
            let dev = std::env::var("SENATE_UI_DEV").ok().filter(|d| !d.is_empty());
            let label = if read_remote().is_some() { remote_name().unwrap_or_else(|| "Серверная нода".into()) } else { "Локальная нода".into() };
            let base_init = |chrome: bool| format!("window.__SENATE__ = {{ token: '{}', base: '{}', chrome: {}, name: {:?}, remote: {}, link: {} }};", token, node_base, chrome, label, read_remote().is_some(), managed);
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

#[cfg(test)]
mod tests {
    use super::*;

    // the tests share one environment variable and one file: one at a time
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn with_file<T>(text: &[u8], f: impl FnOnce() -> T) -> T {
        let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        // remote_file() follows the data directory; point it at a scratch one for the test
        let dir = std::env::temp_dir().join(format!("senate-remote-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("SENATE_HOME", &dir);
        let p = remote_file();
        std::fs::write(&p, text).unwrap();
        let r = f();
        let _ = std::fs::remove_dir_all(&dir);
        r
    }

    #[test]
    fn a_byte_order_mark_does_not_break_the_settings() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(br#"{"url":"http://127.0.0.1:7777","token":"t","ssh":{"host":"h","key":"k"}}"#);
        with_file(&bytes, || {
            assert!(remote_json().unwrap().is_some());
            assert_eq!(read_remote().unwrap().1, "t");
            assert_eq!(read_ssh().unwrap().host, "h");
        });
    }

    #[test]
    fn unusable_settings_are_an_error_not_a_quiet_local_node() {
        with_file(b"{ not json", || assert!(remote_json().unwrap_err().contains("повреждён")));
        with_file(br#"{"url":"http://x"}"#, || assert!(remote_json().unwrap_err().contains("нет адреса или токена")));
    }
}
