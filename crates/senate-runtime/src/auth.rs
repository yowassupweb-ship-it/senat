//! Agent accounts: who is logged in, which models they can use, and the login flow itself.
//! Follows the proven flow of Shar's node: run the CLI's own login command, pull the link (and the
//! device code) out of its output, show them to the user, then verify with the CLI's status command.
//! Credentials stay in the CLIs' own homes; Senate never reads or stores tokens .

use crate::cli;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, watch};

/// (id, label, how) — "login": subscription via the CLI's own browser login.
pub const PROVIDERS: &[(&str, &str, &str)] = &[
    ("claude", "Claude", "login"),
    ("codex", "GPT (Codex)", "login"),
    ("kimi", "Kimi", "login"),
];

const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

pub fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default()
}
pub fn kimi_home() -> PathBuf {
    std::env::var_os("KIMI_CODE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".kimi-code"))
}

#[derive(Debug, Clone, Default)]
pub struct Account {
    pub logged_in: bool,
    pub account: String,
}

#[derive(Debug, Clone, Default)]
pub struct LoginUpdate {
    /// starting | waiting | verifying | done | failed
    pub state: String,
    pub url: String,
    pub code: String,
    pub needs_code: bool,
    pub message: String,
    pub account: String,
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\x1b' => match it.peek() {
                Some('[') => {
                    it.next();
                    for n in it.by_ref() {
                        if n.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(']') => {
                    it.next();
                    for n in it.by_ref() {
                        if n == '\x07' {
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

pub fn find_url(text: &str) -> Option<String> {
    let i = text.find("https://")?;
    let rest = &text[i..];
    let end = rest.find(|c: char| c.is_whitespace() || "\"'<>".contains(c) || c == '\x1b').unwrap_or(rest.len());
    let url = rest[..end].trim_end_matches(|c| ").,;".contains(c));
    (url.len() > 12).then(|| url.to_string())
}

/// Device codes look like `ABCD-EFGH12`.
pub fn find_code(text: &str) -> Option<String> {
    text.split(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')).find(|t| {
        match t.split_once('-') {
            Some((a, b)) => (4..=5).contains(&a.len()) && (4..=6).contains(&b.len()) && !b.contains('-') && t.chars().any(|c| c.is_ascii_digit() || c.is_ascii_uppercase()),
            None => false,
        }
    })
    .map(String::from)
}

/// Run a short command, return (exit code, merged output).
pub async fn exec(path: &std::path::Path, args: &[&str], secs: u64) -> (i32, String) {
    let mut c = cli::command(path);
    c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    match tokio::time::timeout(Duration::from_secs(secs), c.output()).await {
        Ok(Ok(o)) => {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            (o.status.code().unwrap_or(-1), strip_ansi(&s))
        }
        Ok(Err(e)) => (-1, e.to_string()),
        Err(_) => (-2, "превышено время ожидания".into()),
    }
}

fn kimi_credentials() -> Vec<PathBuf> {
    std::fs::read_dir(kimi_home().join("credentials"))
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "json").unwrap_or(false)).collect())
        .unwrap_or_default()
}

pub async fn status(provider: &str) -> Account {
    match provider {
        "claude" => {
            let Some(p) = cli::resolve("claude") else { return Account::default() };
            let (code, out) = exec(&p, &["auth", "status"], 30).await;
            if let (Some(a), Some(b)) = (out.find('{'), out.rfind('}')) {
                if let Ok(v) = serde_json::from_str::<Value>(&out[a..=b]) {
                    let who = ["email", "orgName", "authMethod"].iter().find_map(|k| v[*k].as_str()).unwrap_or("");
                    return Account { logged_in: v["loggedIn"].as_bool().unwrap_or(false), account: who.into() };
                }
            }
            let l = out.to_lowercase();
            Account { logged_in: code == 0 && !l.contains("not logged in") && !l.contains("please log in"), account: String::new() }
        }
        "codex" => {
            let Some(p) = cli::resolve("codex") else { return Account::default() };
            let (code, out) = exec(&p, &["login", "status"], 30).await;
            let ok = code == 0 && !out.to_lowercase().contains("not logged in");
            Account { logged_in: ok, account: if ok { out.lines().next().unwrap_or("").chars().take(120).collect() } else { String::new() } }
        }
        // Kimi has no status command: the presence of its OAuth file is the signal (never read).
        "kimi" => Account { logged_in: cli::resolve("kimi").is_some() && !kimi_credentials().is_empty(), account: String::new() },
        _ => Account::default(),
    }
}

fn pair(id: &str, label: &str) -> Value {
    json!({"id": id, "label": label})
}

/// Models actually available to this account — asked from the provider, not invented.
pub async fn models(provider: &str) -> Vec<Value> {
    let fallback = || match provider {
        "claude" => vec![pair("opus", "Opus · последняя"), pair("sonnet", "Sonnet · последняя"), pair("haiku", "Haiku · последняя")],
        _ => vec![],
    };
    let http = reqwest::Client::builder().timeout(Duration::from_secs(20)).build().ok();
    match provider {
        "claude" => {
            let token = std::fs::read_to_string(home().join(".claude").join(".credentials.json"))
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .and_then(|v| v["claudeAiOauth"]["accessToken"].as_str().map(String::from));
            let (Some(token), Some(http)) = (token, http) else { return fallback() };
            let r = http
                .get("https://api.anthropic.com/v1/models?limit=100")
                .bearer_auth(token)
                .header("anthropic-version", "2023-06-01")
                .header("anthropic-beta", "oauth-2025-04-20")
                .send()
                .await;
            let Ok(r) = r else { return fallback() };
            let Ok(v) = r.json::<Value>().await else { return fallback() };
            let list: Vec<Value> = v["data"]
                .as_array()
                .map(|a| a.iter().filter_map(|m| Some(pair(m["id"].as_str()?, m["display_name"].as_str().unwrap_or(m["id"].as_str()?).trim_start_matches("Claude ")))).collect())
                .unwrap_or_default();
            if list.is_empty() {
                fallback()
            } else {
                let mut all = fallback();
                all.extend(list);
                all
            }
        }
        "codex" => {
            let Some(p) = cli::resolve("codex") else { return vec![] };
            let (_, out) = exec(&p, &["debug", "models"], 30).await;
            let Some(i) = out.find('{') else { return vec![] };
            serde_json::from_str::<Value>(&out[i..])
                .ok()
                .and_then(|v| {
                    v["models"].as_array().map(|a| {
                        a.iter()
                            .filter(|m| m["visibility"] == "list")
                            .filter_map(|m| Some(pair(m["slug"].as_str()?, m["display_name"].as_str().unwrap_or(m["slug"].as_str()?))))
                            .collect()
                    })
                })
                .unwrap_or_default()
        }
        "kimi" => crate::kimi::catalog().await.unwrap_or_default(),
        _ => vec![],
    }
}

/// npm package of each agent CLI, to learn whether the installed CLI is behind (a stale CLI shows a
/// stale model list).
pub fn npm_package(provider: &str) -> Option<&'static str> {
    Some(match provider {
        "claude" => "@anthropic-ai/claude-code",
        "codex" => "@openai/codex",
        "kimi" => "@moonshot-ai/kimi-code",
        _ => return None,
    })
}

pub async fn latest_version(provider: &str) -> Option<String> {
    let pkg = npm_package(provider)?;
    let http = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().ok()?;
    let v: Value = http.get(format!("https://registry.npmjs.org/{pkg}/latest")).send().await.ok()?.json().await.ok()?;
    v["version"].as_str().map(String::from)
}

/// First x.y.z found in a version string ("codex-cli 0.159.3", "2.1.292 (Claude Code)").
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    s.split(|c: char| !(c.is_ascii_digit() || c == '.')).find_map(|t| {
        let mut it = t.split('.');
        Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?))
    })
}

pub async fn logout(provider: &str) {
    match provider {
        "claude" => {
            if let Some(p) = cli::resolve("claude") {
                exec(&p, &["auth", "logout"], 30).await;
            }
        }
        "codex" => {
            if let Some(p) = cli::resolve("codex") {
                exec(&p, &["logout"], 30).await;
            }
        }
        // Kimi offers /logout only interactively: remove its OAuth files.
        "kimi" => {
            for f in kimi_credentials() {
                let _ = std::fs::remove_file(f);
            }
        }
        _ => {}
    }
}

/// Handle of a running login: feed it the code the page shows (Claude) or cancel it.
pub struct LoginHandle {
    code_tx: mpsc::UnboundedSender<String>,
    cancel: watch::Sender<bool>,
}

impl LoginHandle {
    pub fn send_code(&self, code: &str) {
        let _ = self.code_tx.send(code.trim().to_string());
    }
    pub fn cancel(&self) {
        let _ = self.cancel.send(true);
    }
}

fn login_args(provider: &str) -> Option<&'static [&'static str]> {
    Some(match provider {
        "claude" => &["auth", "login"],
        "codex" => &["login", "--device-auth"],
        "kimi" => &["login"],
        _ => return None,
    })
}

pub type OnLogin = Arc<dyn Fn(LoginUpdate) + Send + Sync>;

pub fn start_login(provider: &str, on: OnLogin) -> Result<LoginHandle, String> {
    let args = login_args(provider).ok_or("для этого агента вход не поддержан")?;
    let exe = cli::resolve(provider).ok_or_else(|| format!("CLI «{provider}» не установлен на этой машине"))?;
    let (code_tx, mut code_rx) = mpsc::unbounded_channel::<String>();
    let (cancel_tx, mut cancel_rx) = watch::channel(false);
    let provider = provider.to_string();

    tokio::spawn(async move {
        let mut cmd = if cfg!(windows) {
            // npm shims run fine through plain pipes on Windows (as in Shar's node).
            let mut c = cli::command(&exe);
            c.args(args);
            c
        } else {
            // Unix: the login wants a terminal; `script` supplies a pseudo-terminal without extra crates.
            let line = std::iter::once(exe.to_string_lossy().into_owned()).chain(args.iter().map(|s| s.to_string())).map(|a| format!("'{}'", a.replace('\'', "'\\''"))).collect::<Vec<_>>().join(" ");
            let mut c = tokio::process::Command::new("script");
            if cfg!(target_os = "macos") {
                c.args(["-q", "/dev/null", "sh", "-c", &line]);
            } else {
                c.args(["-qfec", &line, "/dev/null"]);
            }
            c
        };
        // BROWSER=true: the CLI must not open a browser by itself; the UI offers the link.
        cmd.env("BROWSER", "true").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                on(LoginUpdate { state: "failed".into(), message: format!("Не удалось запустить вход: {e}"), ..Default::default() });
                return;
            }
        };
        on(LoginUpdate { state: "starting".into(), message: "Запускаю вход…".into(), ..Default::default() });
        let mut stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (chunk_tx, mut chunk_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        for mut r in [Box::new(stdout) as Box<dyn tokio::io::AsyncRead + Unpin + Send>, Box::new(stderr)] {
            let tx = chunk_tx.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                while let Ok(n) = r.read(&mut buf).await {
                    if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            });
        }
        drop(chunk_tx);

        let mut text = String::new();
        let (mut url, mut code) = (String::new(), String::new());
        let deadline = tokio::time::sleep(LOGIN_TIMEOUT);
        tokio::pin!(deadline);
        let mut cancelled = false;
        loop {
            tokio::select! {
                chunk = chunk_rx.recv() => match chunk {
                    None => break,
                    Some(b) => {
                        text.push_str(&strip_ansi(&String::from_utf8_lossy(&b)));
                        if text.len() > 8000 { let cut = text.len() - 6000; text = text[text.char_indices().map(|(i, _)| i).find(|i| *i >= cut).unwrap_or(0)..].to_string(); }
                        let u = find_url(&text).unwrap_or_default();
                        let c = if provider == "codex" || provider == "kimi" { find_code(&text).unwrap_or_default() } else { String::new() };
                        if (!u.is_empty() && u != url) || (!c.is_empty() && c != code) {
                            if !u.is_empty() { url = u; }
                            if !c.is_empty() { code = c; }
                            let needs = provider == "claude";
                            on(LoginUpdate {
                                state: "waiting".into(), url: url.clone(), code: code.clone(), needs_code: needs,
                                message: if needs { "Откройте ссылку, подтвердите вход и вставьте код со страницы".into() } else { "Откройте ссылку и введите код".into() },
                                ..Default::default()
                            });
                        }
                    }
                },
                Some(c) = code_rx.recv() => {
                    if let Some(w) = stdin.as_mut() { let _ = w.write_all(format!("{c}\r").as_bytes()).await; let _ = w.flush().await; }
                },
                _ = cancel_rx.changed() => { if *cancel_rx.borrow() { cancelled = true; break; } },
                _ = &mut deadline => {
                    let _ = child.kill().await;
                    on(LoginUpdate { state: "failed".into(), message: "Время входа вышло (10 минут) — нажмите «Войти» ещё раз".into(), ..Default::default() });
                    return;
                }
            }
        }
        let _ = child.kill().await;
        if cancelled {
            return;
        }
        on(LoginUpdate { state: "verifying".into(), message: "Проверяю вход…".into(), ..Default::default() });
        let acc = status(&provider).await;
        if acc.logged_in {
            on(LoginUpdate { state: "done".into(), account: acc.account, ..Default::default() });
        } else {
            on(LoginUpdate { state: "failed".into(), message: "Вход не завершён — нажмите «Войти» ещё раз и подтвердите его в браузере".into(), ..Default::default() });
        }
    });
    Ok(LoginHandle { code_tx, cancel: cancel_tx })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_compared_numerically() {
        assert_eq!(parse_version("2.1.292 (Claude Code)"), Some((2, 1, 292)));
        assert_eq!(parse_version("codex-cli 0.159.3"), Some((0, 159, 3)));
        assert!(parse_version("2.1.292") < parse_version("2.1.1000"));
        assert_eq!(parse_version("nothing"), None);
    }

    #[test]
    fn parses_urls_and_codes_from_cli_output() {
        let t = strip_ansi("\x1b[1mOpen\x1b[0m this URL: https://auth.openai.com/codex/device?x=1).\r\nEnter code: ABCD-12345\r\n");
        assert_eq!(find_url(&t).as_deref(), Some("https://auth.openai.com/codex/device?x=1"));
        assert_eq!(find_code(&t).as_deref(), Some("ABCD-12345"));
        assert_eq!(find_code("no code here: abc-def"), None);
        assert_eq!(find_url("nothing"), None);
    }
}
