//! What the person's development folder looks like, from the computer it lives on.
//!
//! A project's folder is on one computer (the node itself, or one of the person's computers running the Senate
//! app). Every look at it — the folder picker, the file tree, a file, git, "open in VS Code" — is one operation
//! carried out *there*: by the node directly, or by that computer's app through the runner channel (`runner::rpc`).
//! The same function serves both, so the panel behaves the same wherever the folder is.
//!
//! Everything stays inside the project folder: a relative path that climbs out, or one that resolves outside
//! through a link, is refused.

use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Folders a tree shows but does not open by itself: big and generated.
const HEAVY: &[&str] = &["node_modules", "target", "dist", "build", ".next", ".nuxt", ".svelte-kit", ".venv", "venv", "__pycache__", ".turbo", ".cache", "vendor", "coverage"];
const READ_LIMIT: u64 = 512 * 1024;

fn quiet(c: &mut Command) {
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
}

/// `root` joined with a relative `rel`, guaranteed to stay inside `root` (links resolved).
pub(crate) fn inside(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim().trim_start_matches(['/', '\\']);
    let r = Path::new(rel);
    if r.is_absolute() || r.components().any(|c| matches!(c, Component::ParentDir | Component::Prefix(_) | Component::RootDir)) {
        return Err("путь за пределами папки проекта".into());
    }
    let base = std::fs::canonicalize(root).map_err(|_| format!("папка {} недоступна на этом компьютере", root.display()))?;
    let full = base.join(r);
    if let Ok(real) = std::fs::canonicalize(&full) {
        if !real.starts_with(&base) {
            return Err("путь за пределами папки проекта".into());
        }
    }
    Ok(full)
}

fn arg<'a>(args: &'a Value, k: &str) -> &'a str {
    args[k].as_str().unwrap_or("")
}

/// Where this app keeps specialists' private folders (`<app data>/runner/workspaces`); set by the runner.
pub(crate) static WS_ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// A root as the node names it: a real path, or `@ws/<slug>` — a specialist's private folder on this computer.
fn root_of(args: &Value) -> PathBuf {
    let r = arg(args, "root");
    match r.strip_prefix("@ws/") {
        Some(rest) if !rest.contains("..") => WS_ROOT.get().map(|w| w.join(rest)).unwrap_or_else(|| PathBuf::from(r)),
        _ => PathBuf::from(r),
    }
}

/// Carry out one operation on this computer.
pub(crate) fn op(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "dirs" => Ok(dirs(arg(args, "path"))),
        "is_dir" => Ok(json!({"is_dir": Path::new(arg(args, "path")).is_dir()})),
        "tree" => {
            let root = root_of(args);
            // a private folder that was never used yet is simply empty, not an error
            if arg(args, "root").starts_with("@ws/") {
                let _ = std::fs::create_dir_all(&root);
            }
            tree(&root, arg(args, "path"))
        }
        "read" => read(&root_of(args), arg(args, "path")),
        "git_status" => Ok(crate::project_git::status(&root_of(args))),
        "git_diff" => {
            let p = arg(args, "path");
            if !p.is_empty() {
                inside(&root_of(args), p)?;
            }
            Ok(json!({"diff": crate::project_git::diff(&root_of(args), (!p.is_empty()).then_some(p))}))
        }
        "open" => open_in_editor(&root_of(args), arg(args, "path")),
        "env" => Ok(environment()),
        other => Err(format!("неизвестная операция {other}")),
    }
}

/// Folder picker: drives and the home folder at the top, then sub-folders only.
fn dirs(path: &str) -> Value {
    if path.is_empty() {
        let mut roots: Vec<Value> = vec![];
        if let Some(h) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
            roots.push(json!({"name": "Домашняя папка", "path": PathBuf::from(h).to_string_lossy()}));
        }
        #[cfg(windows)]
        for l in b'A'..=b'Z' {
            let p = format!("{}:\\", l as char);
            if Path::new(&p).exists() {
                roots.push(json!({"name": p, "path": p}));
            }
        }
        #[cfg(not(windows))]
        roots.push(json!({"name": "/", "path": "/"}));
        return json!({"path": "", "parent": null, "entries": roots});
    }
    let p = PathBuf::from(path);
    let Ok(rd) = std::fs::read_dir(&p) else { return json!({"path": path, "parent": p.parent().map(|x| x.to_string_lossy().into_owned()), "entries": [], "error": "не открыть"}) };
    let mut entries: Vec<Value> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == "node_modules" || name == "$RECYCLE.BIN" || name == "System Volume Information" {
                return None;
            }
            Some(json!({"name": name, "path": e.path().to_string_lossy(), "git": e.path().join(".git").exists()}))
        })
        .collect();
    entries.sort_by_key(|e| e["name"].as_str().unwrap_or("").to_lowercase());
    json!({"path": path, "parent": p.parent().map(|x| x.to_string_lossy().into_owned()), "entries": entries, "git": p.join(".git").exists()})
}

/// One level of the project tree: folders first, then files, names as the file manager sorts them.
fn tree(root: &Path, rel: &str) -> Result<Value, String> {
    let dir = inside(root, rel)?;
    let rd = std::fs::read_dir(&dir).map_err(|e| format!("не открыть папку: {e}"))?;
    let mut entries: Vec<Value> = vec![];
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name == ".git" || name == ".DS_Store" || name == "Thumbs.db" {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        let is_dir = ft.is_dir() || (ft.is_symlink() && e.path().is_dir());
        let child = if rel.is_empty() { name.clone() } else { format!("{}/{}", rel.trim_end_matches(['/', '\\']), name) };
        let size = if is_dir { 0 } else { e.metadata().map(|m| m.len()).unwrap_or(0) };
        entries.push(json!({"name": name, "path": child, "dir": is_dir, "size": size, "heavy": is_dir && HEAVY.contains(&name.as_str()), "hidden": name.starts_with('.')}));
        if entries.len() >= 2000 {
            break;
        }
    }
    entries.sort_by(|a, b| {
        b["dir"].as_bool().cmp(&a["dir"].as_bool()).then_with(|| a["name"].as_str().unwrap_or("").to_lowercase().cmp(&b["name"].as_str().unwrap_or("").to_lowercase()))
    });
    Ok(json!({"path": rel, "entries": entries}))
}

/// A file to look at: text up to half a megabyte; binary files are named, not shown.
fn read(root: &Path, rel: &str) -> Result<Value, String> {
    let p = inside(root, rel)?;
    let meta = std::fs::metadata(&p).map_err(|e| format!("не открыть файл: {e}"))?;
    if meta.is_dir() {
        return Err("это папка".into());
    }
    let size = meta.len();
    let mut bytes = Vec::new();
    {
        use std::io::Read;
        let f = std::fs::File::open(&p).map_err(|e| format!("не открыть файл: {e}"))?;
        f.take(READ_LIMIT).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    }
    if bytes.iter().take(8000).any(|b| *b == 0) {
        return Ok(json!({"path": rel, "size": size, "binary": true}));
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(json!({"path": rel, "size": size, "text": text, "truncated": size > READ_LIMIT}))
}

/// VS Code on this computer: the project folder, and the file at its place when one is given.
fn open_in_editor(root: &Path, rel: &str) -> Result<Value, String> {
    let base = inside(root, "")?;
    let file = if rel.is_empty() { None } else { Some(inside(root, rel)?) };
    let exe = if cfg!(windows) { "code.cmd" } else { "code" };
    let mut c = Command::new(exe);
    c.arg(&base);
    if let Some(f) = &file {
        c.arg("-g").arg(f);
    }
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c.spawn().map_err(|_| "VS Code не найден на этом компьютере (нужна команда code в PATH)".to_string())?;
    Ok(json!({"ok": true}))
}

fn version_of(cmd: &str, args: &[&str]) -> Option<String> {
    let exe = if cfg!(windows) && !cmd.ends_with(".exe") { format!("{cmd}.cmd") } else { cmd.to_string() };
    let run = |e: &str| {
        let mut c = Command::new(e);
        c.args(args);
        quiet(&mut c);
        let child = c.spawn().ok()?;
        let out = wait_with_timeout(child, Duration::from_secs(4))?;
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let line = text.lines().find(|l| !l.trim().is_empty())?.trim().to_string();
        Some(line.chars().take(60).collect::<String>())
    };
    run(cmd).or_else(|| if exe != cmd { run(&exe) } else { None })
}

fn wait_with_timeout(mut child: std::process::Child, limit: Duration) -> Option<std::process::Output> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
}

/// The development tools this computer has (name → version line). Looked up once per process: a few seconds.
pub(crate) fn environment() -> Value {
    static CACHE: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            let probes: &[(&str, &str, &[&str])] = &[
                ("git", "git", &["--version"]),
                ("node", "node", &["--version"]),
                ("npm", "npm", &["--version"]),
                ("pnpm", "pnpm", &["--version"]),
                ("python", if cfg!(windows) { "python" } else { "python3" }, &["--version"]),
                ("cargo", "cargo", &["--version"]),
                ("go", "go", &["version"]),
                ("docker", "docker", &["--version"]),
                ("gh", "gh", &["--version"]),
                ("code", "code", &["--version"]),
            ];
            let mut have = serde_json::Map::new();
            let mut missing = vec![];
            for (name, cmd, args) in probes {
                match version_of(cmd, args) {
                    Some(v) => {
                        have.insert(name.to_string(), json!(v));
                    }
                    None => missing.push(*name),
                }
            }
            json!({"os": std::env::consts::OS, "shell": if cfg!(windows) { "PowerShell" } else { "bash" }, "tools": have, "missing": missing})
        })
        .clone()
}

/// What the specialist is told about where they work, so they use what is there and do not reach for what is not.
#[cfg(test)]
pub(crate) fn environment_block(machine: &str, cwd: &Path, env: &Value) -> String {
    environment_block_for(machine, cwd, env, "")
}

/// The same, for the CLI that will run (`backend` like "claude:opus"): the shell its command tool really uses differs.
pub(crate) fn environment_block_for(machine: &str, cwd: &Path, env: &Value, backend: &str) -> String {
    let provider = backend.split(':').next().unwrap_or("");
    let tools: Vec<String> = env["tools"].as_object().map(|o| o.iter().map(|(k, v)| format!("{k} ({})", v.as_str().unwrap_or("").trim_start_matches(k).trim()).replace(" ()", "")).collect()).unwrap_or_default();
    let missing: Vec<&str> = env["missing"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
    let os = match env["os"].as_str().unwrap_or("") {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        o => o,
    };
    let mut s = format!("\n\n## Где ты работаешь\nКомпьютер «{machine}» ({os}), рабочая папка: {}.\n", cwd.display());
    if !tools.is_empty() {
        s += &format!("Есть: {}.\n", tools.join(", "));
    }
    if !missing.is_empty() {
        s += &format!("Нет: {}. Не рассчитывай на них; если без чего-то не обойтись, скажи человеку, что поставить.\n", missing.join(", "));
    }
    // Claude Code and Kimi run commands through Git Bash on Windows; Codex through PowerShell.
    let bash_on_windows = os == "Windows" && matches!(provider, "claude" | "kimi");
    if bash_on_windows {
        s += "Оболочка твоего инструмента команд — Git Bash (POSIX): пути вида /e/Projects/... или \"E:/Projects/...\", `&&`, `export`, `nohup`; PowerShell — только через `powershell -NoProfile -Command \"…\"`. Многострочный текст в git передавай через файл (`git commit -F`).\n";
    } else if os == "Windows" {
        s += "Оболочка — PowerShell: пути с обратной косой чертой, многострочный текст в git передавай через файл (`git commit -F`).\n";
    }
    s += if bash_on_windows {
        "Всё, что не заканчивается само (dev-сервер, `npm run dev`, `tauri dev`, watch), запускай в фоне и не жди: `nohup npm run dev > dev.log 2>&1 &`, потом проверь порт (`curl -sI http://localhost:N`). Запущенная напрямую команда, которая не завершается, держит твой ход до остановки человеком.\n"
    } else if os == "Windows" {
        "Всё, что не заканчивается само (dev-сервер, `npm run dev`, `tauri dev`, watch), запускай в фоне и не жди: `Start-Process -WindowStyle Hidden npm -ArgumentList 'run','dev' -RedirectStandardOutput dev.log`, потом проверь порт (`Test-NetConnection localhost -Port N` или запрос к странице). Запущенная напрямую команда, которая не завершается, держит твой ход до остановки человеком.\n"
    } else {
        "Всё, что не заканчивается само (dev-сервер, `npm run dev`, watch), запускай в фоне и не жди: `nohup npm run dev > dev.log 2>&1 &`, потом проверь порт запросом к странице. Запущенная напрямую команда, которая не завершается, держит твой ход до остановки человеком.\n"
    };
    s
}

// ---------------------------------------------------------------- private workspaces that follow the specialist
//
// A specialist without a project folder works in their own folder: `<data>/workspaces/<slug>` on the node,
// `<app data>/runner/workspaces/<slug>` on a computer. When they move between the two (switched to "Локально" and
// back), their files go with them: before a turn on a computer the app takes from the node what is newer there —
// their own folder and their colleagues' beside it, so "the stand the developer made" is at `../dev/…` — and after
// the turn sends back what they changed in their own. Generated folders and large files stay where they are.

const SYNC_FILE_LIMIT: u64 = 20 << 20;
const SYNC_FILES_MAX: usize = 5000;

fn mtime_ms(m: &std::fs::Metadata) -> i64 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Files under `root` (relative paths with `/`), with size and change time; generated folders and `.git` skipped.
pub(crate) fn manifest(root: &Path) -> Vec<Value> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<Value>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            if out.len() >= SYNC_FILES_MAX {
                return;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                if name == ".git" || HEAVY.contains(&name.as_str()) {
                    continue;
                }
                walk(base, &e.path(), out);
            } else if let Ok(m) = e.metadata() {
                if m.len() > SYNC_FILE_LIMIT || name.ends_with(".log") {
                    continue;
                }
                let rel = e.path().strip_prefix(base).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                out.push(json!({"path": rel, "size": m.len(), "mtime": mtime_ms(&m)}));
            }
        }
    }
    let mut out = vec![];
    walk(root, root, &mut out);
    out
}

/// A relative path from a manifest, joined safely under `root`.
pub(crate) fn sync_path(root: &Path, rel: &str) -> Option<PathBuf> {
    let r = Path::new(rel);
    if rel.is_empty() || rel.contains('\\') || r.is_absolute() || r.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    Some(root.join(r))
}

/// Write a synced file and give it the time it had at its source, so the next comparison is fair.
pub(crate) fn write_synced(path: &Path, bytes: &[u8], mtime: i64) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, bytes)?;
    if mtime > 0 {
        let f = std::fs::OpenOptions::new().write(true).open(path)?;
        f.set_modified(std::time::UNIX_EPOCH + Duration::from_millis(mtime as u64))?;
    }
    Ok(())
}

/// Should the copy at `local` be replaced by the source's (`size`, `mtime`)? Missing, or older by more than the
/// rounding file systems do.
pub(crate) fn needs_update(local: &Path, size: u64, mtime: i64) -> bool {
    match std::fs::metadata(local) {
        Err(_) => true,
        Ok(m) => mtime > mtime_ms(&m) + 2000 || (m.len() != size && mtime > mtime_ms(&m)),
    }
}

/// This computer's name and a stable id, kept in the app's data folder.
pub(crate) fn machine_identity(data_dir: &Path) -> (String, String) {
    let file = data_dir.join("machine.json");
    let name = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).ok().filter(|n| !n.is_empty()).unwrap_or_else(|| {
        let mut c = Command::new("hostname");
        quiet(&mut c);
        c.output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "Компьютер".into())
    });
    if let Some(v) = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        if let Some(id) = v["id"].as_str().filter(|s| !s.is_empty()) {
            return (id.to_string(), v["name"].as_str().filter(|s| !s.is_empty()).map(String::from).unwrap_or(name));
        }
    }
    let id = format!("m_{}", uuid::Uuid::new_v4().simple());
    let _ = std::fs::create_dir_all(data_dir);
    let _ = std::fs::write(&file, json!({"id": id, "name": name}).to_string());
    (id, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_stays_inside_the_project_and_reads_text() {
        let d = std::env::temp_dir().join(format!("senate-ws-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("node_modules").join("x")).unwrap();
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join("src").join("main.ts"), "console.log('привет')\n").unwrap();
        std::fs::write(d.join("logo.png"), [137u8, 80, 78, 71, 0, 0, 1]).unwrap();
        std::fs::write(d.join("README.md"), "# x").unwrap();

        let t = op("tree", &json!({"root": d, "path": ""})).unwrap();
        let names: Vec<&str> = t["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["node_modules", "src", "logo.png", "README.md"], "folders first, .git hidden");
        assert_eq!(t["entries"][0]["heavy"], true);
        let sub = op("tree", &json!({"root": d, "path": "src"})).unwrap();
        assert_eq!(sub["entries"][0]["path"], "src/main.ts");

        let f = op("read", &json!({"root": d, "path": "src/main.ts"})).unwrap();
        assert!(f["text"].as_str().unwrap().contains("привет"));
        assert_eq!(op("read", &json!({"root": d, "path": "logo.png"})).unwrap()["binary"], true);

        for bad in ["../x", "src/../../x", "/etc/passwd", "C:\\Windows\\win.ini"] {
            assert!(op("read", &json!({"root": d, "path": bad})).is_err(), "{bad}");
            assert!(op("tree", &json!({"root": d, "path": bad})).is_err(), "{bad}");
        }
        assert!(op("tree", &json!({"root": d.join("nope"), "path": ""})).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn workspaces_are_listed_without_generated_folders_and_copied_with_their_time() {
        let d = std::env::temp_dir().join(format!("senate-sync-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(d.join("dev").join("crm").join("node_modules").join("x")).unwrap();
        std::fs::write(d.join("dev").join("crm").join("index.html"), "<h1>CRM</h1>").unwrap();
        std::fs::write(d.join("dev").join("crm").join("node_modules").join("x").join("a.js"), "x").unwrap();
        std::fs::write(d.join("dev").join("crm").join("server.log"), "noise").unwrap();
        let m = manifest(&d);
        let paths: Vec<&str> = m.iter().map(|e| e["path"].as_str().unwrap()).collect();
        assert_eq!(paths, vec!["dev/crm/index.html"]);

        let copy = d.join("copy");
        let target = sync_path(&copy, "dev/crm/index.html").unwrap();
        assert!(needs_update(&target, 12, m[0]["mtime"].as_i64().unwrap()));
        write_synced(&target, b"<h1>CRM</h1>", m[0]["mtime"].as_i64().unwrap()).unwrap();
        assert!(!needs_update(&target, 12, m[0]["mtime"].as_i64().unwrap()), "same time: nothing to do");
        assert!(needs_update(&target, 12, m[0]["mtime"].as_i64().unwrap() + 60_000), "newer at the source");

        for bad in ["../x", "/etc/x", "a/../../x", "a\\b", ""] {
            assert!(sync_path(&copy, bad).is_none(), "{bad}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_computer_keeps_its_id() {
        let d = std::env::temp_dir().join(format!("senate-mid-{}", uuid::Uuid::new_v4().simple()));
        let (a, _) = machine_identity(&d);
        let (b, _) = machine_identity(&d);
        assert_eq!(a, b);
        assert!(a.starts_with("m_"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_environment_block_names_what_is_there_and_what_is_not() {
        let env = json!({"os": "windows", "tools": {"git": "git version 2.47", "node": "v24.14.0"}, "missing": ["docker"]});
        let s = environment_block("WORK-PC", Path::new("E:/Projects/Degma"), &env);
        assert!(s.contains("WORK-PC") && s.contains("Windows") && s.contains("E:/Projects/Degma"));
        assert!(s.contains("node (v24.14.0)") && s.contains("Нет: docker") && s.contains("PowerShell"));
        // Claude and Kimi run commands through Git Bash on Windows: they are told so, not "PowerShell"
        let b = environment_block_for("WORK-PC", Path::new("E:/Projects/Degma"), &env, "claude:opus");
        assert!(b.contains("Git Bash") && b.contains("nohup") && !b.contains("Оболочка — PowerShell"));
        assert!(environment_block_for("WORK-PC", Path::new("E:/x"), &env, "codex").contains("Оболочка — PowerShell"));
    }
}
