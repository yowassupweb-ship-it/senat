//! Files the person attaches to a message — screenshots first of all.
//!
//! A file is stored once under `<data dir>/uploads/<id>/<name>`. A message refers to it only by `id`:
//! the node looks the file up itself, so a client cannot point an agent at some other path on the disk.

use serde_json::{json, Value};
use std::path::PathBuf;

/// One file. Screenshots are a few MB; this leaves room for a PDF or a log.
pub(crate) const MAX_BYTES: usize = 25 * 1024 * 1024;
/// Files in one message.
pub(crate) const MAX_PER_MESSAGE: usize = 8;

pub(crate) fn dir(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("uploads")
}

/// An id is what `new_id` makes: letters and digits. Anything else is not ours (`..`, separators).
pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// A file name that is safe on every disk and in a URL: no folders, no control characters, no reserved
/// characters, not too long, never empty.
pub(crate) fn safe_name(raw: &str) -> String {
    let base = raw.rsplit(|c| c == '/' || c == '\\').next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    let cleaned: String = cleaned.chars().take(100).collect();
    if cleaned.is_empty() {
        "file".into()
    } else {
        cleaned
    }
}

fn ext(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default()
}

/// Content type and whether a browser may show it inline. Only raster images are shown inline: an uploaded
/// HTML or SVG page opened on the app's own address could run as the app.
pub(crate) fn kind(name: &str) -> (&'static str, bool) {
    match ext(name).as_str() {
        "png" => ("image/png", true),
        "jpg" | "jpeg" => ("image/jpeg", true),
        "gif" => ("image/gif", true),
        "webp" => ("image/webp", true),
        "pdf" => ("application/pdf", false),
        "txt" | "log" | "md" | "csv" | "json" => ("text/plain", false),
        _ => ("application/octet-stream", false),
    }
}

pub(crate) fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16)) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The attachment as the UI and the agent see it. `path` stays on this machine: it is what the agent opens.
pub(crate) fn describe(id: &str, name: &str, size: u64, path: &std::path::Path) -> Value {
    json!({
        "id": id, "name": name, "type": kind(name).0, "size": size,
        "url": format!("/api/uploads/{id}/{}", percent_encode(name)),
        "path": path.to_string_lossy(),
    })
}

/// Save one file; returns its description.
pub(crate) async fn store(data_dir: &std::path::Path, raw_name: &str, content_type: &str, bytes: &[u8]) -> Result<Value, String> {
    if bytes.is_empty() {
        return Err("пустой файл".into());
    }
    if bytes.len() > MAX_BYTES {
        return Err(format!("файл больше {} МБ", MAX_BYTES / 1024 / 1024));
    }
    let mut name = safe_name(raw_name);
    // A pasted screenshot may come without a usable name: give it an extension from its content type.
    if ext(&name).is_empty() {
        let e = match content_type {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "application/pdf" => "pdf",
            _ => "",
        };
        if !e.is_empty() {
            name = format!("{name}.{e}");
        }
    }
    let id = senate_core::new_id();
    let folder = dir(data_dir).join(&id);
    tokio::fs::create_dir_all(&folder).await.map_err(|e| format!("папка загрузок: {e}"))?;
    let path = folder.join(&name);
    tokio::fs::write(&path, bytes).await.map_err(|e| format!("не удалось сохранить файл: {e}"))?;
    Ok(describe(&id, &name, bytes.len() as u64, &path))
}

/// Where a stored file lives, if there is one by this id and name.
pub(crate) fn locate(data_dir: &std::path::Path, id: &str, name: &str) -> Option<PathBuf> {
    if !valid_id(id) || safe_name(name) != name {
        return None;
    }
    let p = dir(data_dir).join(id).join(name);
    p.is_file().then_some(p)
}

/// What the client sent as attachments (`[{id}]`) → what is really stored. Unknown ids are dropped, the count is
/// capped, and everything else the client said (name, path, size) is ignored.
pub(crate) fn resolve(data_dir: &std::path::Path, requested: &[Value]) -> Vec<Value> {
    let mut out = vec![];
    for r in requested.iter().take(MAX_PER_MESSAGE) {
        let Some(id) = r["id"].as_str().filter(|i| valid_id(i)) else { continue };
        let folder = dir(data_dir).join(id);
        let Some(entry) = std::fs::read_dir(&folder).ok().and_then(|mut d| d.find_map(|e| e.ok()).filter(|e| e.path().is_file())) else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(describe(id, &name, size, &entry.path()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_safe() {
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("C:\\Users\\a\\скрин.png"), "скрин.png");
        assert_eq!(safe_name("a<b>:c?.png"), "a_b__c_.png");
        assert_eq!(safe_name("..."), "file");
        assert_eq!(safe_name(""), "file");
        assert!(safe_name(&"x".repeat(500)).chars().count() <= 100);
    }

    #[test]
    fn ids_cannot_escape_the_folder() {
        assert!(valid_id("01a1c75a09bc70ec"));
        for bad in ["", "..", "a/b", "a\\b", "a.b", "../x"] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn only_raster_images_are_shown_inline() {
        assert_eq!(kind("a.PNG"), ("image/png", true));
        assert_eq!(kind("a.svg"), ("application/octet-stream", false));
        assert_eq!(kind("a.html"), ("application/octet-stream", false));
        assert_eq!(kind("a.pdf"), ("application/pdf", false));
    }

    #[test]
    fn percent_round_trip() {
        let n = "скрин шот (1).png";
        assert_eq!(percent_decode(&percent_encode(n)), n);
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[tokio::test]
    async fn a_file_is_stored_found_and_resolved_by_id_only() {
        let d = std::env::temp_dir().join(format!("senate-up-{}", senate_core::new_id()));
        let v = store(&d, "shot", "image/png", b"\x89PNG....").await.unwrap();
        assert_eq!(v["name"], "shot.png", "extension from the content type");
        let id = v["id"].as_str().unwrap();
        assert!(locate(&d, id, "shot.png").is_some());
        assert!(locate(&d, id, "other.png").is_none());
        assert!(locate(&d, "../x", "shot.png").is_none());
        // the client's own claims about the file are ignored; unknown ids vanish
        let r = resolve(&d, &[json!({"id": id, "path": "C:/secret.txt", "name": "evil"}), json!({"id": "nope"}), json!({"path": "C:/x"})]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0]["name"], "shot.png");
        assert!(r[0]["path"].as_str().unwrap().contains(id));
        assert!(store(&d, "e", "", b"").await.is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
