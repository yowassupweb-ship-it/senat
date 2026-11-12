//! Local-work permissions. The user picks a mode per chat or project; the environment enforces it
//! on every tool call the agent makes (Senate.md section 18). The agent's own wishes and the text
//! of a prompt never widen it.
//!
//!  plan  — read and research only; any change waits for the plan to be approved
//!  ask   — every change is confirmed
//!  edit  — edits inside the project folder go through; commands are confirmed unless harmless
//!  auto  — turnkey: nothing is asked. Only what wrecks the machine itself still stops (see CATASTROPHIC).

use crate::PermissionMode;
use serde_json::Value;
use std::path::Path;

/// Every question about a dangerous operation starts with this. "Allow always" never applies to them.
pub const DANGER_PREFIX: &str = "Опасная операция";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Ask(String),
    Deny(String),
}

const READ_ONLY: &[&str] = &[
    "Read", "Glob", "Grep", "LS", "NotebookRead", "WebSearch", "WebFetch", "TodoWrite", "ToolSearch", "Task", "Agent", "BashOutput",
    "KillShell", "KillBash",
];
const FILE_WRITE: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit"];
const SHELL: &[&str] = &["Bash", "PowerShell"];

/// Commands that only look. Matched on the first word(s) of every segment of a pipeline.
const SAFE_COMMANDS: &[&str] = &[
    "ls", "dir", "pwd", "cat", "type", "head", "tail", "wc", "echo", "which", "where", "whoami", "date", "tree", "stat", "file",
    "get-childitem", "get-content", "get-location", "select-string", "get-item", "test-path", "rg", "grep", "findstr", "find", "fd",
    "git status", "git diff", "git log", "git show", "git branch", "git remote", "git rev-parse", "git blame", "git ls-files",
    "cargo check", "cargo test", "cargo build", "cargo clippy", "cargo fmt --check", "cargo metadata", "cargo tree",
    "npm test", "npm run test", "npm run build", "npm run lint", "npm run typecheck", "npm ls", "npx tsc", "npx vitest", "npx eslint",
    "node --version", "node -v", "python --version", "python -m pytest", "pytest", "go test", "go build", "go vet", "tsc",
    "dotnet build", "dotnet test", "make test", "make check",
];

/// Things that destroy the machine or what is on it beyond this project, and cannot be undone by any
/// repository. These are the only questions that survive "Под ключ": the person chose turnkey, not reckless.
const CATASTROPHIC: &[&str] = &[
    "mkfs", "format-volume", "format c:", "diskpart", "dd if=", "shutdown", "reboot", "restart-computer", "stop-computer",
    "rm -rf /", "rm -rf c:", "rm -fr /", "remove-item -recurse c:\\", "del /s /q c:\\", "reg delete",
];

/// Operations a human confirms in the asking modes (plan / ask / edit). In "Под ключ" they go through.
const DANGEROUS: &[&str] = &[
    "rm -rf", "rm -fr", "rm -r ", "rmdir /s", "del /s", "del /q", "remove-item -recurse", "remove-item -force", "ri -r", "format-volume",
    "git push", "git reset --hard", "git clean", "git checkout --", "git branch -d", "git branch -D", "git rebase", "git filter",
    "npm publish", "cargo publish", "docker push", "kubectl delete", "drop table", "drop database", "truncate table",
    "sudo ", "chmod -R", "chown -R", "mkfs", "dd if=", "shutdown", "reboot", "restart-computer", "stop-computer",
    "curl | sh", "curl|sh", "| sh", "| bash", "iex (", "invoke-expression", "invoke-webrequest", "reg delete", "reg add", "setx ",
    "ssh ", "scp ", "rsync ", "winget uninstall", "apt remove", "apt-get remove", "brew uninstall",
];

fn inside(root: &Path, p: &str) -> bool {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let path = Path::new(p);
    let full = if path.is_absolute() { path.to_path_buf() } else { root.join(path) };
    // The file may not exist yet: canonicalize its nearest existing ancestor.
    let mut probe = full.clone();
    let mut tail = vec![];
    while !probe.exists() {
        match (probe.file_name().map(|f| f.to_os_string()), probe.parent()) {
            (Some(f), Some(parent)) => {
                tail.push(f);
                probe = parent.to_path_buf();
            }
            _ => break,
        }
    }
    let mut resolved = std::fs::canonicalize(&probe).unwrap_or(probe);
    for part in tail.into_iter().rev() {
        resolved.push(part);
    }
    // `..` segments left in the unresolved tail must not escape.
    if resolved.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return false;
    }
    resolved.starts_with(&root)
}

fn is_safe_segment(seg: &str) -> bool {
    let s = seg.trim().to_lowercase();
    if s.is_empty() {
        return true;
    }
    SAFE_COMMANDS.iter().any(|c| s == *c || s.starts_with(&format!("{c} ")))
}

/// `a && b | c; d` → segments. Redirections into files make a command non-harmless.
pub fn command_is_harmless(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    if lower.contains('>') || lower.contains("$(") || lower.contains('`') || lower.contains("out-file") || lower.contains("set-content") {
        return false;
    }
    lower.split(|c| c == '|' || c == ';' || c == '&' || c == '\n').all(is_safe_segment)
}

fn dangerous(cmd: &str) -> Option<&'static str> {
    let lower = cmd.to_lowercase();
    DANGEROUS.iter().find(|d| lower.contains(&d.to_lowercase())).copied()
}

fn catastrophic(cmd: &str) -> Option<&'static str> {
    let lower = cmd.to_lowercase();
    CATASTROPHIC.iter().find(|d| lower.contains(&d.to_lowercase())).copied()
}

fn path_of(input: &Value) -> Option<&str> {
    ["file_path", "path", "notebook_path"].iter().find_map(|k| input.get(*k).and_then(|v| v.as_str()))
}

pub fn judge(mode: PermissionMode, tool: &str, input: &Value, root: &Path, trusted_mcp: &[String]) -> Verdict {
    let t = tool.strip_prefix("mcp__senate__").unwrap_or(tool);
    if tool.starts_with("mcp__senate__") || t.starts_with("senate_") {
        // Senate's own tools are gated by the environment's grants.
        return Verdict::Allow;
    }
    // A server the user marked as trusted: its tools do not ask.
    if let Some(rest) = tool.strip_prefix("mcp__") {
        if let Some((server, _)) = rest.split_once("__") {
            if trusted_mcp.iter().any(|t| t == server) {
                return Verdict::Allow;
            }
        }
    }
    if tool == "ExitPlanMode" {
        return if mode == PermissionMode::Auto { Verdict::Allow } else { Verdict::Ask("План готов. Выполнять?".into()) };
    }
    if READ_ONLY.contains(&tool) {
        return Verdict::Allow;
    }
    if mode == PermissionMode::Plan {
        return Verdict::Deny("В режиме «План» изменения не выполняются. Составьте план и попросите утвердить его.".into());
    }
    if FILE_WRITE.contains(&tool) {
        let p = path_of(input).unwrap_or("");
        return match mode {
            // Turnkey: the specialist writes wherever the work is. A project without a folder set is the normal
            // case, not a reason to interrogate the person on every file.
            PermissionMode::Auto => Verdict::Allow,
            PermissionMode::Ask => Verdict::Ask(format!("Изменить файл {p}")),
            _ if !p.is_empty() && inside(root, p) => Verdict::Allow,
            _ => Verdict::Ask(format!("Изменить файл вне папки проекта: {p}")),
        };
    }
    if SHELL.contains(&tool) {
        let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(d) = catastrophic(cmd) {
            return Verdict::Ask(format!("{DANGER_PREFIX} ({d}): {}", crate::clip(cmd, 160)));
        }
        if mode != PermissionMode::Auto {
            if let Some(d) = dangerous(cmd) {
                return Verdict::Ask(format!("{DANGER_PREFIX} ({d}): {}", crate::clip(cmd, 160)));
            }
        }
        return match mode {
            PermissionMode::Auto => Verdict::Allow,
            PermissionMode::Edit if command_is_harmless(cmd) => Verdict::Allow,
            _ => Verdict::Ask(format!("Выполнить команду: {}", crate::clip(cmd, 200))),
        };
    }
    match mode {
        PermissionMode::Auto => Verdict::Allow,
        _ => Verdict::Ask(format!("Использовать инструмент {tool}")),
    }
}

/// CLI-native permission mode that matches ours (the CLI pre-approves what that mode pre-approves,
/// so fewer round trips; everything else still reaches `judge`).
pub fn cli_mode(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Plan => "plan",
        PermissionMode::Ask => "default",
        PermissionMode::Edit | PermissionMode::Auto => "acceptEdits",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn root() -> std::path::PathBuf {
        std::env::temp_dir()
    }

    #[test]
    fn reads_always_pass_writes_follow_mode() {
        let r = root();
        assert_eq!(judge(PermissionMode::Ask, "Read", &json!({"file_path": "x"}), &r, &[]), Verdict::Allow);
        assert!(matches!(judge(PermissionMode::Ask, "Write", &json!({"file_path": "a.txt"}), &r, &[]), Verdict::Ask(_)));
        assert_eq!(judge(PermissionMode::Edit, "Write", &json!({"file_path": "a.txt"}), &r, &[]), Verdict::Allow);
        assert!(matches!(judge(PermissionMode::Plan, "Write", &json!({"file_path": "a.txt"}), &r, &[]), Verdict::Deny(_)));
    }

    #[test]
    fn in_the_asking_modes_a_file_outside_the_project_is_confirmed() {
        let r = root();
        let outside = if cfg!(windows) { "C:\\Windows\\hosts" } else { "/etc/hosts" };
        assert!(matches!(judge(PermissionMode::Edit, "Write", &json!({"file_path": outside}), &r, &[]), Verdict::Ask(_)));
        assert!(matches!(judge(PermissionMode::Edit, "Edit", &json!({"file_path": "../escape.txt"}), &r, &[]), Verdict::Ask(_)));
    }

    #[test]
    fn commands_harmless_vs_not() {
        assert!(command_is_harmless("git status && git diff"));
        assert!(command_is_harmless("cargo test -p senate-core"));
        assert!(!command_is_harmless("cargo test > out.txt"));
        assert!(!command_is_harmless("git status; rm file"));
        assert!(!command_is_harmless("echo $(whoami)"));
    }

    #[test]
    fn shell_by_mode() {
        let r = root();
        let ls = json!({"command": "git log --oneline"});
        let build = json!({"command": "pip install requests"});
        assert_eq!(judge(PermissionMode::Edit, "PowerShell", &ls, &r, &[]), Verdict::Allow);
        assert!(matches!(judge(PermissionMode::Edit, "Bash", &build, &r, &[]), Verdict::Ask(_)));
        assert_eq!(judge(PermissionMode::Auto, "Bash", &build, &r, &[]), Verdict::Allow);
        assert!(matches!(judge(PermissionMode::Ask, "Bash", &ls, &r, &[]), Verdict::Ask(_)));
    }

    #[test]
    fn turnkey_does_not_ask_and_the_other_modes_still_do() {
        let r = root();
        // "Под ключ" means turnkey: the daily dangerous-looking work goes through without a word
        for cmd in ["rm -rf build", "git push origin main", "git reset --hard HEAD~3", "ssh deploy@host 'systemctl restart app'", "curl http://x | sh"] {
            assert_eq!(judge(PermissionMode::Auto, "Bash", &json!({"command": cmd}), &r, &[]), Verdict::Allow, "{cmd}");
        }
        // ...and so does a file anywhere on the disk, and any tool
        let outside = if cfg!(windows) { "D:\\other\\app\\next.config.mjs" } else { "/srv/app/next.config.mjs" };
        assert_eq!(judge(PermissionMode::Auto, "Write", &json!({"file_path": outside}), &r, &[]), Verdict::Allow);
        assert_eq!(judge(PermissionMode::Auto, "mcp__figma__get_file", &json!({}), &r, &[]), Verdict::Allow);
        assert_eq!(judge(PermissionMode::Auto, "ExitPlanMode", &json!({}), &r, &[]), Verdict::Allow);

        // only what wrecks the machine itself still stops, in every mode
        for cmd in ["mkfs.ext4 /dev/sda1", "shutdown /r /t 0", "dd if=/dev/zero of=/dev/sda", "rm -rf /", "reg delete HKLM\\Software"] {
            assert!(matches!(judge(PermissionMode::Auto, "Bash", &json!({"command": cmd}), &r, &[]), Verdict::Ask(_)), "{cmd}");
        }
        // the asking modes keep the long list
        for cmd in ["git push origin main", "rm -rf build"] {
            assert!(matches!(judge(PermissionMode::Edit, "Bash", &json!({"command": cmd}), &r, &[]), Verdict::Ask(_)), "{cmd}");
        }
        assert!(matches!(judge(PermissionMode::Edit, "Write", &json!({"file_path": outside}), &r, &[]), Verdict::Ask(_)));
    }

    #[test]
    fn senate_tools_pass_through_to_environment_grants() {
        assert_eq!(judge(PermissionMode::Ask, "mcp__senate__senate_remember", &json!({}), &root(), &[]), Verdict::Allow);
    }
}
