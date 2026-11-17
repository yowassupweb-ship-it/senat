//! Read-only git view of a project folder — what the agent changed, so the user can review it
//! next to the chat without leaving Senate. Shells out to `git`; no git library needed.

use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Stdio};

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let mut c = Command::new("git");
    c.arg("-C").arg(root).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Repositories inside a folder that is not one itself, as paths relative to it. A chat without a project folder
/// works in the specialists' own folders, and the code is a level or two down (`dev2/crm-app-git`): the panel
/// offers these instead of stopping at "not a repository".
fn nested_repos(root: &Path) -> Vec<String> {
    const SKIP: [&str; 6] = ["node_modules", "target", "dist", "build", ".venv", "vendor"];
    let mut found = vec![];
    let mut level = vec![(root.to_path_buf(), String::new())];
    for _depth in 0..3 {
        let mut next = vec![];
        for (dir, rel) in level {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten().take(400) {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') || SKIP.contains(&name.as_str()) || !e.path().is_dir() {
                    continue;
                }
                let sub = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
                if e.path().join(".git").exists() {
                    found.push(sub);
                } else {
                    next.push((e.path(), sub));
                }
                if found.len() >= 12 {
                    found.sort();
                    return found;
                }
            }
        }
        level = next;
    }
    found.sort();
    found
}

pub fn status(root: &Path) -> Value {
    if !root.is_dir() {
        return json!({"available": false, "reason": "папка недоступна"});
    }
    let not_repo = || json!({"available": false, "reason": "не git-репозиторий", "repos": nested_repos(root)});
    let Some(inside) = git(root, &["rev-parse", "--is-inside-work-tree"]) else {
        return not_repo();
    };
    if inside.trim() != "true" {
        return not_repo();
    }
    let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default().trim().to_string();
    let porcelain = git(root, &["status", "--porcelain=v1", "-uall", "-z"]).unwrap_or_default();
    let mut changes = vec![];
    let mut parts = porcelain.split('\0').filter(|s| !s.is_empty());
    while let Some(entry) = parts.next() {
        if entry.len() < 4 {
            continue;
        }
        let (code, path) = entry.split_at(3);
        let code = code.trim();
        // Renames carry the old path as the next NUL field; skip it.
        if code.starts_with('R') || code.starts_with('C') {
            parts.next();
        }
        let kind = match code {
            "??" => "new",
            c if c.contains('D') => "deleted",
            c if c.contains('A') => "added",
            c if c.starts_with('R') => "renamed",
            _ => "modified",
        };
        changes.push(json!({"path": path, "status": kind}));
        if changes.len() >= 500 {
            break;
        }
    }
    let stat = git(root, &["diff", "HEAD", "--shortstat"]).unwrap_or_default().trim().to_string();
    let last = git(root, &["log", "-1", "--pretty=%h %s"]).unwrap_or_default().trim().to_string();
    // Commits not pushed yet / not pulled yet, when the branch tracks a remote one.
    let (behind, ahead) = git(root, &["rev-list", "--left-right", "--count", "@{u}...HEAD"])
        .and_then(|o| {
            let mut it = o.split_whitespace().map(|n| n.parse::<u64>().unwrap_or(0));
            Some((it.next()?, it.next()?))
        })
        .map(|(b, a)| (Some(b), Some(a)))
        .unwrap_or((None, None));
    json!({"available": true, "branch": branch, "changes": changes, "stat": stat, "last_commit": last, "ahead": ahead, "behind": behind})
}

/// Unified diff for one path (or everything). Untracked files are shown as additions.
pub fn diff(root: &Path, path: Option<&str>) -> String {
    let mut out = String::new();
    let mut args = vec!["diff", "HEAD", "--no-color", "--unified=3"];
    if let Some(p) = path {
        args.push("--");
        args.push(p);
    }
    if let Some(d) = git(root, &args) {
        out.push_str(&d);
    }
    if let Some(p) = path {
        if out.trim().is_empty() {
            // untracked → show content as an addition
            let full = root.join(p);
            if let Ok(text) = std::fs::read_to_string(&full) {
                let body: String = text.lines().take(2000).map(|l| format!("+{l}\n")).collect();
                out = format!("diff --git a/{p} b/{p}\nnew file\n--- /dev/null\n+++ b/{p}\n@@ -0,0 +1,{} @@\n{body}", text.lines().count().min(2000));
            }
        }
    }
    if out.len() > 400_000 {
        out.truncate(400_000);
        out.push_str("\n… diff обрезан …\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_that_is_not_a_repository_offers_the_ones_inside_it() {
        let root = std::env::temp_dir().join(format!("senate-nested-{}", uuid::Uuid::new_v4().simple()));
        for d in ["dev2/crm-app-git/.git", "designer/crm-app/src", "dev/site/.git", "dev/node_modules/pkg/.git", ".hidden/x/.git"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        assert_eq!(nested_repos(&root), vec!["dev/site".to_string(), "dev2/crm-app-git".to_string()], "dependencies and hidden folders are not offered");
        let st = status(&root);
        assert_eq!(st["available"], false);
        assert_eq!(st["repos"].as_array().map(|a| a.len()), Some(2));
        let _ = std::fs::remove_dir_all(&root);
    }
}
