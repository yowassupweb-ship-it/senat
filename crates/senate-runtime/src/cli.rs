//! Locating and spawning agent CLIs portably (npm shims on Windows, plain binaries elsewhere).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

#[cfg(windows)]
const EXE: &[&str] = &["exe", "cmd", "bat"];
#[cfg(not(windows))]
const EXE: &[&str] = &[""];

fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default()
}

/// Find a CLI by name. On Windows an npm `.cmd` shim is resolved to the real binary it launches,
/// which lets us spawn it directly (no `cmd.exe`, no quoting traps).
pub fn resolve(name: &str) -> Option<PathBuf> {
    if let Ok(over) = std::env::var(format!("SENATE_{}_PATH", name.to_uppercase())) {
        let p = PathBuf::from(over);
        if p.exists() {
            return Some(p);
        }
    }
    for dir in path_dirs() {
        for ext in EXE {
            let file = if ext.is_empty() { name.to_string() } else { format!("{name}.{ext}") };
            let candidate = dir.join(&file);
            if candidate.is_file() {
                #[cfg(windows)]
                if *ext == "cmd" || *ext == "bat" {
                    if let Some(real) = unwrap_shim(&candidate, name) {
                        return Some(real);
                    }
                }
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(windows)]
fn unwrap_shim(shim: &Path, name: &str) -> Option<PathBuf> {
    let dir = shim.parent()?;
    let nm = dir.join("node_modules");
    let candidates: &[&str] = match name {
        "claude" => &["@anthropic-ai/claude-code/bin/claude.exe"],
        "codex" => &[
            "@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe",
            "@openai/codex/node_modules/@openai/codex-win32-arm64/vendor/aarch64-pc-windows-msvc/bin/codex.exe",
        ],
        // Node scripts: launched through `node` (see `command`).
        "kimi" => &["@moonshot-ai/kimi-code/dist/main.mjs"],
        _ => &[],
    };
    candidates.iter().map(|c| nm.join(c)).find(|p| p.is_file())
}

#[cfg(not(windows))]
#[allow(dead_code)]
fn unwrap_shim(_: &Path, _: &str) -> Option<PathBuf> {
    None
}

/// Base command for a resolved CLI. Batch shims that could not be unwrapped go through `cmd /C`.
pub fn command(path: &Path) -> Command {
    let ext_any = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if matches!(ext_any.as_str(), "js" | "mjs" | "cjs") {
        let node = resolve("node").unwrap_or_else(|| PathBuf::from("node"));
        let mut c = command(&node);
        c.arg(path);
        return c;
    }
    #[cfg(windows)]
    {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if ext == "cmd" || ext == "bat" {
            let mut c = Command::new("cmd");
            c.arg("/d").arg("/c").arg(path);
            hide_window(&mut c);
            return c;
        }
    }
    let mut c = Command::new(path);
    #[cfg(windows)]
    hide_window(&mut c);
    c
}

#[cfg(windows)]
fn hide_window(c: &mut Command) {
    // CREATE_NO_WINDOW: no console flashing up when the desktop app launches a CLI.
    c.creation_flags(0x0800_0000);
}

pub async fn version(path: &Path) -> Option<String> {
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        command(path).arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).output(),
    )
    .await
    .ok()?
    .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Environment for child CLIs: ours minus things that must not leak into an agent's tools.
pub fn child_env() -> Vec<(String, String)> {
    const SECRET: [&str; 6] = ["SENATE_TOKEN", "SENATE_HUB_TOKEN", "TOKEN", "SECRET", "PASSWORD", "PRIVATE"];
    std::env::vars()
        .filter(|(k, _)| {
            let up = k.to_uppercase();
            // Keep the CLI's own login variables; drop obvious secrets of the environment.
            !(up.starts_with("SENATE_") || (SECRET.iter().any(|s| up.contains(s)) && !up.starts_with("CLAUDE") && !up.starts_with("ANTHROPIC") && !up.starts_with("OPENAI")))
        })
        .collect()
}
