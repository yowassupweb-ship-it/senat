//! The link to a node that lives on a server and is not exposed to the internet.
//!
//! The node listens on the server's loopback only, so the app reaches it through an SSH tunnel. The app owns that
//! tunnel: it starts it, watches it, and starts it again whenever it dies, so nobody has to think about it.
//!
//! What it takes care of:
//!   * a free local port is chosen on every start (no clash with anything else on the computer);
//!   * `ssh` is started without a console window and never asks anything (`BatchMode`): a key or nothing;
//!   * a dead process is replaced after a short pause that grows while the server stays unreachable;
//!   * a tunnel that is alive but no longer carries traffic (the computer slept, the network changed) is found by
//!     asking the node for its health, and is torn down and rebuilt;
//!   * the reason for a failure is kept in plain words for the screen to show, instead of a generic "no connection".

use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::sync::watch;

/// Where and how to tunnel. Stored next to the node's token in `remote.json`.
#[derive(Clone, Debug)]
pub struct Spec {
    pub user: String,
    pub host: String,
    pub key: PathBuf,
    /// The node's port on the server (its loopback).
    pub remote_port: u16,
}

/// What the screen shows: is the node reachable right now, and if not, why.
#[derive(Clone, Debug, Serialize, Default)]
pub struct LinkState {
    pub up: bool,
    /// Human words, Russian: "Соединяюсь с сервером…", "Сервер не отвечает…".
    pub detail: String,
    /// How many times the tunnel had to be rebuilt since the app started.
    pub restarts: u32,
}

pub type Shared = Arc<Mutex<LinkState>>;

/// On Windows a child outlives a parent that is killed (Task Manager, a crash, a forced restart): the tunnel would
/// stay behind, holding its port, and the next start would have to take another one. A job object with
/// "kill on close" fixes that: the job's only handle is ours, so when this process ends, by any road, Windows closes
/// it and ends every process put into the job.
#[cfg(windows)]
mod job {
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    pub struct Job(HANDLE);
    // The handle is only ever used to add processes; Windows allows that from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub fn new() -> Option<Job> {
            unsafe {
                let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if h.is_null() {
                    return None;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    CloseHandle(h);
                    return None;
                }
                Some(Job(h))
            }
        }
        pub fn adopt(&self, process: HANDLE) -> bool {
            unsafe { AssignProcessToJobObject(self.0, process) != 0 }
        }
    }
    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

pub struct Tunnel {
    pub port: u16,
    pub state: Shared,
    /// Dropping this stops the supervisor and, through `kill_on_drop`, the `ssh` process.
    _stop: watch::Sender<bool>,
}

/// A local port that is free right now. The preferred one is taken when it can be, so the address stays the same
/// from day to day and only changes when something else is sitting on it.
pub fn pick_port(preferred: u16) -> u16 {
    for p in [preferred, 0] {
        if let Ok(l) = std::net::TcpListener::bind(("127.0.0.1", p)) {
            if let Ok(a) = l.local_addr() {
                return a.port();
            }
        }
    }
    preferred
}

/// Ask the node whether it is alive. A tunnel to a node that is down, or a half-dead tunnel, both answer "no".
pub async fn healthy(port: u16) -> bool {
    let work = async {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.ok()?;
        s.write_all(b"GET /api/health HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").await.ok()?;
        let mut buf = Vec::with_capacity(256);
        s.take(512).read_to_end(&mut buf).await.ok()?;
        let text = String::from_utf8_lossy(&buf);
        Some(text.starts_with("HTTP/1.1 200") || text.starts_with("HTTP/1.0 200"))
    };
    matches!(tokio::time::timeout(Duration::from_secs(4), work).await, Ok(Some(true)))
}

/// What `ssh` said, turned into something a person can act on.
pub fn explain(stderr: &str) -> String {
    let s = stderr.to_lowercase();
    let msg = if s.contains("permission denied") {
        "Сервер не принял ключ доступа. Нужно заново выдать ключ (его могли удалить на сервере)."
    } else if s.contains("host key verification failed") || s.contains("remote host identification has changed") {
        "Ключ сервера изменился: его могли переустановить. Проверьте сервер и сбросьте запись в known_hosts."
    } else if s.contains("could not resolve") || s.contains("name or service not known") {
        "Не удаётся найти сервер по имени. Проверьте интернет."
    } else if s.contains("timed out") || s.contains("no route") || s.contains("network is unreachable") || s.contains("connection refused") {
        "Сервер недоступен. Скорее всего, нет интернета; связь вернётся сама."
    } else if s.contains("identity file") && s.contains("no such file") || s.contains("no such file or directory") {
        "Не найден файл ключа доступа к серверу."
    } else if s.contains("not found") || s.contains("is not recognized") {
        "В Windows не найден ssh (компонент «Клиент OpenSSH»)."
    } else if s.contains("address already in use") || s.contains("cannot listen") || s.contains("bind") {
        "Локальный порт занят, выбираю другой."
    } else {
        ""
    };
    if msg.is_empty() {
        let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
        if last.is_empty() { "Соединяюсь с сервером…".into() } else { format!("Не удаётся соединиться: {}", last.chars().take(160).collect::<String>()) }
    } else {
        msg.into()
    }
}

fn set(state: &Shared, up: bool, detail: impl Into<String>) {
    if let Ok(mut s) = state.lock() {
        s.up = up;
        s.detail = detail.into();
    }
}

fn ssh_command(spec: &Spec, port: u16) -> Command {
    let mut c = Command::new("ssh");
    c.arg("-N")
        .arg("-L")
        .arg(format!("127.0.0.1:{port}:127.0.0.1:{}", spec.remote_port))
        .arg("-i")
        .arg(&spec.key)
        .args(["-o", "IdentitiesOnly=yes", "-o", "BatchMode=yes", "-o", "ExitOnForwardFailure=yes"])
        .args(["-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3", "-o", "ConnectTimeout=10"])
        .args(["-o", "StrictHostKeyChecking=yes", "-o", "LogLevel=ERROR"])
        .arg(format!("{}@{}", spec.user, spec.host))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    c
}

/// Start the tunnel and keep it alive for as long as the returned value lives.
pub fn start(spec: Spec, preferred_port: u16) -> Tunnel {
    let port = pick_port(preferred_port);
    let state: Shared = Arc::new(Mutex::new(LinkState { up: false, detail: "Соединяюсь с сервером…".into(), restarts: 0 }));
    let (stop_tx, mut stop_rx) = watch::channel(false);
    let st = state.clone();
    #[cfg(windows)]
    let job = job::Job::new();
    tauri::async_runtime::spawn(async move {
        let mut pause = Duration::from_secs(1);
        loop {
            if *stop_rx.borrow() {
                return;
            }
            let began = Instant::now();
            let mut child = match ssh_command(&spec, port).spawn() {
                Ok(c) => {
                    #[cfg(windows)]
                    if let (Some(j), Some(h)) = (&job, c.raw_handle()) {
                        j.adopt(h as _);
                    }
                    c
                }
                Err(e) => {
                    set(&st, false, explain(&e.to_string()));
                    tokio::select! { _ = tokio::time::sleep(Duration::from_secs(10)) => {}, _ = stop_rx.changed() => return }
                    continue;
                }
            };
            // Read what ssh says while it runs; the last words are the reason if it dies.
            let said = Arc::new(Mutex::new(String::new()));
            if let Some(mut err) = child.stderr.take() {
                let said = said.clone();
                tauri::async_runtime::spawn(async move {
                    let mut buf = [0u8; 512];
                    while let Ok(n) = err.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                        if let Ok(mut s) = said.lock() {
                            s.push_str(&String::from_utf8_lossy(&buf[..n]));
                            let len = s.len();
                            if len > 2000 {
                                *s = s[len - 1000..].to_string();
                            }
                        }
                    }
                });
            }

            let mut misses = 0u32;
            let mut ever_up = false;
            let mut tick = tokio::time::interval(Duration::from_secs(4));
            let outcome = loop {
                tokio::select! {
                    _ = stop_rx.changed() => { let _ = child.start_kill(); return; }
                    status = child.wait() => break format!("{:?}", status.ok()),
                    _ = tick.tick() => {
                        if healthy(port).await {
                            misses = 0;
                            ever_up = true;
                            set(&st, true, "На связи с сервером");
                        } else {
                            misses += 1;
                            if ever_up && misses == 1 {
                                set(&st, false, "Связь с сервером прервалась, восстанавливаю…");
                            }
                            // ssh is alive but carries nothing (sleep, network change) — or never got going: rebuild it.
                            if misses >= if ever_up { 3 } else { 8 } {
                                let _ = child.start_kill();
                                break "stale".to_string();
                            }
                        }
                    }
                }
            };
            let _ = child.wait().await;
            let why = if outcome == "stale" { "Связь с сервером зависла, переподключаюсь…".to_string() } else { explain(&said.lock().map(|s| s.clone()).unwrap_or_default()) };
            if let Ok(mut s) = st.lock() {
                s.up = false;
                s.detail = why;
                s.restarts += 1;
            }
            // A link that held for a minute was healthy: forget the earlier failures. Otherwise back off, to 30 s.
            pause = if began.elapsed() > Duration::from_secs(60) { Duration::from_secs(1) } else { (pause * 2).min(Duration::from_secs(30)) };
            tokio::select! { _ = tokio::time::sleep(pause) => {}, _ = stop_rx.changed() => return }
        }
    });
    Tunnel { port, state, _stop: stop_tx }
}

/// Wait until the node answers or the time runs out, so the window does not open on a link that is still forming.
pub async fn wait_up(port: u16, limit: Duration) -> bool {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if healthy(port).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_are_explained_in_words() {
        assert!(explain("root@1.2.3.4: Permission denied (publickey).").contains("ключ доступа"));
        assert!(explain("Host key verification failed.").contains("Ключ сервера изменился"));
        assert!(explain("ssh: Could not resolve hostname x: Name or service not known").contains("по имени"));
        assert!(explain("ssh: connect to host 1.2.3.4 port 22: Connection timed out").contains("недоступен"));
        assert!(explain("Warning: Identity file k not accessible: No such file or directory.").contains("файл ключа"));
        assert!(explain("").contains("Соединяюсь"));
        // an unknown message is passed on rather than hidden, and cut to a sane length
        let odd = explain(&format!("something odd {}", "x".repeat(400)));
        assert!(odd.starts_with("Не удаётся соединиться: something odd") && odd.chars().count() < 220);
    }

    #[test]
    fn a_free_port_is_found_even_when_the_preferred_one_is_taken() {
        let held = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let taken = held.local_addr().unwrap().port();
        let got = pick_port(taken);
        assert_ne!(got, taken, "the busy port is not handed out");
        assert!(got > 0);
        // a free preferred port is kept, so the address stays the same from day to day
        drop(held);
        assert_eq!(pick_port(taken), taken);
    }

    #[tokio::test]
    async fn health_is_read_from_what_the_node_answers() {
        use tokio::io::AsyncWriteExt;
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            // first a dead end (accepts, says nothing useful), then a real answer
            for reply in [&b"HTTP/1.1 502 Bad Gateway\r\n\r\n"[..], &b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}"[..]] {
                let (mut s, _) = l.accept().await.unwrap();
                let mut buf = [0u8; 256];
                let _ = tokio::io::AsyncReadExt::read(&mut s, &mut buf).await;
                let _ = s.write_all(reply).await;
            }
        });
        assert!(!healthy(port).await, "a 502 is not a live node");
        assert!(healthy(port).await);
        // nothing listening at all
        let free = pick_port(0);
        assert!(!healthy(free).await);
    }
}
