//! Agent accounts on this node: login state, models, subscription limits, and the login flow the UI
//! drives (button, link and code, status) — the flow Shar's node has, with Senate's own plumbing.

use crate::App;
use senate_runtime::auth::{self, LoginHandle, LoginUpdate, OnLogin};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Default, Clone)]
pub(crate) struct AccountState {
    pub logged_in: bool,
    pub account: String,
    pub models: Vec<Value>,
    pub login: Option<Value>,
    pub checked_at: i64,
}

fn installed(app: &App, provider: &str) -> Option<Value> {
    app.installed.read().unwrap().iter().find(|c| c["backend"] == provider).cloned()
}

/// What the UI shows for every supported agent.
pub(crate) fn view(app: &App) -> Vec<Value> {
    let st = app.accounts.lock().unwrap();
    let limits = app.limits.read().unwrap();
    auth::PROVIDERS
        .iter()
        .map(|(id, label, _)| {
            let a = st.get(*id).cloned().unwrap_or_default();
            let inst = installed(app, id);
            let latest = app.latest.read().unwrap().get(*id).cloned().unwrap_or_default();
            let behind = match (auth::parse_version(inst.as_ref().and_then(|i| i["version"].as_str()).unwrap_or("")), auth::parse_version(&latest)) {
                (Some(cur), Some(new)) => cur < new,
                _ => false,
            };
            json!({
                "provider": id, "label": label,
                "installed": inst.is_some(),
                "version": inst.as_ref().and_then(|i| i["version"].as_str()).unwrap_or(""),
                "state": if a.login.as_ref().map(|l| matches!(l["state"].as_str(), Some("starting" | "waiting" | "verifying"))).unwrap_or(false) { "pending" } else if a.logged_in { "ready" } else { "none" },
                "account": a.account,
                "models": a.models,
                "login": a.login,
                "limits": limits.get(*id).cloned().unwrap_or(Value::Null),
                "checked_at": a.checked_at,
                "latest_version": latest,
                "update_available": behind,
            })
        })
        .collect()
}

pub(crate) fn publish(app: &App) {
    app.publish("accounts", json!(view(app)));
}

/// Backends offered in chats: installed AND logged in, with the account's own model list.
pub(crate) fn rebuild_caps(app: &App) {
    let st = app.accounts.lock().unwrap();
    let mut caps: Vec<Value> = vec![];
    for (id, label, _) in auth::PROVIDERS {
        let Some(inst) = installed(app, id) else { continue };
        let a = st.get(*id).cloned().unwrap_or_default();
        let mut models = vec![json!({"id": "", "label": "По умолчанию"})];
        models.extend(a.models.iter().cloned());
        caps.push(json!({"backend": id, "label": label, "version": inst["version"], "ready": a.logged_in, "account": a.account, "models": models}));
    }
    caps.push(json!({"backend": "mock", "label": "Демо (без модели)", "version": "1", "ready": true, "models": [{"id": "", "label": "Демо"}]}));
    drop(st);
    let _ = app.store.upsert_node(&app.node_id, "", std::env::consts::OS, "local", "", &json!(caps));
    *app.caps.write().unwrap() = caps;
}

pub(crate) async fn refresh(app: &Arc<App>, provider: &str) {
    if installed(app, provider).is_none() {
        app.accounts.lock().unwrap().remove(provider);
        rebuild_caps(app);
        return;
    }
    let acc = auth::status(provider).await;
    let models = if acc.logged_in { auth::models(provider).await } else { vec![] };
    {
        let mut st = app.accounts.lock().unwrap();
        let e = st.entry(provider.to_string()).or_default();
        e.logged_in = acc.logged_in;
        e.account = acc.account;
        e.models = models;
        e.checked_at = senate_core::now_ms();
        if acc.logged_in && e.login.as_ref().map(|l| l["state"] != "waiting" && l["state"] != "starting" && l["state"] != "verifying").unwrap_or(true) {
            e.login = None;
        }
    }
    rebuild_caps(app);
    publish(app);
}

pub(crate) async fn refresh_all(app: &Arc<App>) {
    let list: Vec<&str> = auth::PROVIDERS.iter().map(|(id, _, _)| *id).collect();
    futures_util::future::join_all(list.into_iter().map(|p| refresh(app, p))).await;
}

pub(crate) fn start_login(app: &Arc<App>, provider: &str) -> Result<(), String> {
    if let Some(old) = app.logins.lock().unwrap().remove(provider) {
        old.cancel();
    }
    let a = app.clone();
    let p = provider.to_string();
    let on: OnLogin = Arc::new(move |u: LoginUpdate| {
        let login = json!({"state": u.state, "url": u.url, "code": u.code, "needs_code": u.needs_code, "message": u.message});
        {
            let mut st = a.accounts.lock().unwrap();
            let e = st.entry(p.clone()).or_default();
            // Keep the link and code once seen: later updates (verifying) must not erase them from the screen.
            let mut merged = login.clone();
            if let Some(prev) = &e.login {
                for k in ["url", "code"] {
                    if merged[k].as_str().unwrap_or("").is_empty() {
                        merged[k] = prev[k].clone();
                    }
                }
            }
            if !u.url.is_empty() {
                e.checked_at = senate_core::now_ms();
            }
            e.login = Some(merged);
        }
        if u.state == "done" || u.state == "failed" {
            a.logins.lock().unwrap().remove(&p);
            let (a2, p2) = (a.clone(), p.clone());
            tokio::spawn(async move {
                refresh(&a2, &p2).await;
                // A failed login stays visible until dismissed.
            });
        }
        publish(&a);
    });
    let handle: LoginHandle = auth::start_login(provider, on)?;
    app.logins.lock().unwrap().insert(provider.to_string(), handle);
    Ok(())
}

pub(crate) fn login_code(app: &App, provider: &str, code: &str) -> bool {
    match app.logins.lock().unwrap().get(provider) {
        Some(h) => {
            h.send_code(code);
            true
        }
        None => false,
    }
}

pub(crate) fn cancel_login(app: &App, provider: &str) {
    if let Some(h) = app.logins.lock().unwrap().remove(provider) {
        h.cancel();
    }
    if let Some(e) = app.accounts.lock().unwrap().get_mut(provider) {
        e.login = None;
    }
    publish(app);
}

pub(crate) async fn logout(app: &Arc<App>, provider: &str) {
    cancel_login(app, provider);
    auth::logout(provider).await;
    app.limits.write().unwrap().remove(provider);
    refresh(app, provider).await;
}

/// Open the current login link in the browser of the machine this node runs on (desktop use).
/// Only the link the CLI itself printed for the running login can be opened.
pub(crate) fn open_login_link(app: &App, provider: &str) -> Result<(), String> {
    let url = app.accounts.lock().unwrap().get(provider).and_then(|a| a.login.as_ref().and_then(|l| l["url"].as_str().map(String::from))).unwrap_or_default();
    if !url.starts_with("https://") {
        return Err("нет ссылки для входа".into());
    }
    open_url(&url)
}

/// Hand an already-vetted http(s) URL to the default browser of the machine this node runs on.
pub(crate) fn open_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("можно открыть только http(s)-ссылку".into());
    }
    let url = url.to_string();
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", &url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(&url);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(&url);
        c
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Subscription windows from a Claude run (five-hour, weekly).
pub(crate) fn note_limits(app: &App, provider: &str, windows: &Value) {
    let mut m = serde_json::Map::new();
    if let Some(o) = windows.as_object() {
        for (k, v) in o {
            m.insert(k.clone(), json!({"used": (v["utilization"].as_f64().unwrap_or(0.0) * 100.0).round(), "resets_at": v["resetsAt"]}));
        }
    }
    app.limits.write().unwrap().insert(provider.to_string(), json!({"windows": m, "at": senate_core::now_ms()}));
    publish(app);
}

/// Which CLIs have a newer release than the installed one.
pub(crate) async fn check_updates(app: &Arc<App>) {
    for (id, _, _) in auth::PROVIDERS {
        if installed(app, id).is_none() {
            continue;
        }
        if let Some(v) = auth::latest_version(id).await {
            app.latest.write().unwrap().insert(id.to_string(), v);
        }
    }
    publish(app);
}

/// Keep accounts, model lists and CLI versions fresh without anyone asking.
pub(crate) fn spawn_refresher(app: &Arc<App>) {
    let a = app.clone();
    tokio::spawn(async move {
        check_updates(&a).await;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
            refresh_all(&a).await;
            check_updates(&a).await;
        }
    });
}
