// Stamps the node with what it was built from, so a running node can say which code it is.
// The server builds from a packed archive without .git: `deploy/pack.ps1` then writes BUILD_INFO next to this
// file. A build inside the repository asks git directly.
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let o = Command::new("git").args(args).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let file = std::path::Path::new(&dir).join("BUILD_INFO");
    println!("cargo:rerun-if-changed={}", file.display());
    let stamp = std::fs::read_to_string(&file)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let commit = git(&["rev-parse", "--short", "HEAD"])?;
            let dirty = git(&["status", "--porcelain"]).map(|s| !s.is_empty()).unwrap_or(false);
            Some(if dirty { format!("{commit}+edits") } else { commit })
        })
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=SENATE_BUILD={stamp}");
}
