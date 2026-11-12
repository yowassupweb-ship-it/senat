//! Pictures made to order, inside Senate.
//!
//! The person connects an image service once (Settings → API: an address, a key, what it is for) and says which
//! specialists may use it. A specialist then asks for a picture by describing it; the node calls the service, keeps
//! the key to itself, writes the picture into the uploads folder and hands back a path on this machine. The picture
//! also appears in the chat, so the person sees what was made without opening anything.
//!
//! The request is the OpenAI image shape (`POST <base>/images/generations`, `{model, prompt, size, n}`), which the
//! common services speak or imitate. The answer may carry the picture inline (`b64_json`) or as a link (`url`);
//! both are handled, and a link is fetched by the node, never by the agent.

use crate::{uploads, App};
use senate_core::tools::ToolCtx;
use senate_core::Error;
use serde_json::{json, Value};
use std::time::Duration;

/// A picture is large and slow: its own client, with a longer wait than an ordinary API call.
fn client() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(180)).redirect(reqwest::redirect::Policy::none()).build().unwrap()
}

/// What an image service is asked for. `size` is passed through as the services spell it ("1024x1024").
pub(crate) fn request_body(model: &str, prompt: &str, size: &str, n: u8) -> Value {
    let mut b = json!({"prompt": prompt, "n": n, "response_format": "b64_json"});
    if !model.is_empty() {
        b["model"] = json!(model);
    }
    if !size.is_empty() {
        b["size"] = json!(size);
    }
    b
}

/// The picture inside an answer: inline bytes, or a link to fetch. The shapes differ between services, so the
/// usual spellings are all accepted.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Picture {
    Inline(String),
    Link(String),
}

pub(crate) fn find_picture(body: &Value) -> Option<Picture> {
    let first = body["data"].as_array().and_then(|a| a.first()).cloned().or_else(|| body["images"].as_array().and_then(|a| a.first()).cloned()).unwrap_or(body.clone());
    for key in ["b64_json", "b64", "image_base64", "base64"] {
        if let Some(s) = first[key].as_str().filter(|s| !s.is_empty()) {
            return Some(Picture::Inline(s.to_string()));
        }
    }
    if let Some(s) = first.as_str().filter(|s| s.starts_with("data:image")) {
        return s.split_once(",").map(|(_, b)| Picture::Inline(b.to_string()));
    }
    for key in ["url", "image_url", "image"] {
        match &first[key] {
            Value::String(s) if s.starts_with("http") => return Some(Picture::Link(s.clone())),
            Value::String(s) if s.starts_with("data:image") => return s.split_once(",").map(|(_, b)| Picture::Inline(b.to_string())),
            Value::Object(o) => {
                if let Some(u) = o.get("url").and_then(|v| v.as_str()).filter(|u| u.starts_with("http")) {
                    return Some(Picture::Link(u.to_string()));
                }
            }
            _ => {}
        }
    }
    None
}

fn decode_base64(s: &str) -> Option<Vec<u8>> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = match c {
            b'-' => 62,
            b'_' => 63,
            _ => T.iter().position(|&t| t == c)? as u32,
        };
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

/// A name for the file, made from what was asked for: a person looking in the folder can tell them apart.
pub(crate) fn file_name(prompt: &str, ext: &str) -> String {
    let stem: String = prompt
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .take(6)
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(60)
        .collect();
    format!("{}.{ext}", if stem.is_empty() { "image".into() } else { stem.to_lowercase() })
}

fn ext_of(content_type: &str) -> &'static str {
    match content_type {
        t if t.contains("jpeg") || t.contains("jpg") => "jpg",
        t if t.contains("webp") => "webp",
        t if t.contains("gif") => "gif",
        _ => "png",
    }
}

/// `senate_image_generate`: make a picture and keep it. Returns where it is, for the specialist to use.
pub(crate) async fn generate(app: &App, ctx: &ToolCtx, args: &Value) -> Result<Value, Error> {
    let prompt = args["prompt"].as_str().unwrap_or("").trim();
    if prompt.is_empty() {
        return Err(Error::invalid("опишите картинку: что на ней, в каком стиле, для чего"));
    }
    let wanted = args["service"].as_str().unwrap_or("").trim();
    let mine = app.store.integrations_for_agent(&ctx.agent_id)?;
    // The person names the service; with one connected for pictures there is nothing to name.
    let it = match wanted {
        "" => mine
            .iter()
            .find(|i| i.name == "images" || i.docs.to_lowercase().contains("картин") || i.docs.to_lowercase().contains("image"))
            .cloned()
            .ok_or_else(|| {
                Error::Forbidden(
                    "сервис картинок не подключён или не выдан тебе. Попроси человека подключить его в «API» (адрес и ключ) и выдать доступ; потом укажи его имя в service.".into(),
                )
            })?,
        name => mine.iter().find(|i| i.name == name).cloned().ok_or_else(|| Error::Forbidden(format!("подключение «{name}» тебе не выдано (см. senate_api_list)")))?,
    };
    let model = args["model"].as_str().unwrap_or("").trim();
    let size = args["size"].as_str().unwrap_or("1024x1024").trim();
    let body = request_body(model, prompt, size, 1);

    let url = format!("{}/images/generations", it.base_url.trim_end_matches('/'));
    let mut req = client().post(&url).json(&body);
    match it.auth_type.as_str() {
        "bearer" if !it.secret.is_empty() => req = req.bearer_auth(&it.secret),
        "header" if !it.secret.is_empty() => req = req.header(it.auth_name.as_str(), &it.secret),
        "basic" if !it.secret.is_empty() => {
            let (u, p) = it.secret.split_once(':').unwrap_or((&it.secret, ""));
            req = req.basic_auth(u, Some(p));
        }
        "query" if !it.secret.is_empty() => req = req.query(&[(it.auth_name.as_str(), it.secret.as_str())]),
        _ => {}
    }
    let started = std::time::Instant::now();
    let resp = req.send().await.map_err(|e| Error::invalid(format!("сервис картинок не ответил: {}", e.without_url())))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if status >= 400 {
        let msg: String = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().or(v["message"].as_str()).map(String::from))
            .unwrap_or_else(|| text.chars().take(300).collect());
        return Err(Error::invalid(format!("сервис картинок ответил {status}: {msg}")));
    }
    let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::String(text));
    let picture = find_picture(&parsed).ok_or_else(|| Error::invalid("в ответе сервиса нет картинки"))?;

    let (bytes, ctype) = match picture {
        Picture::Inline(b64) => (decode_base64(&b64).ok_or_else(|| Error::invalid("картинку не удалось раскодировать"))?, "image/png".to_string()),
        Picture::Link(link) => {
            let r = client().get(&link).send().await.map_err(|e| Error::invalid(format!("картинка не скачалась: {}", e.without_url())))?;
            let ct = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("image/png").to_string();
            (r.bytes().await.map_err(|e| Error::invalid(format!("картинка не прочиталась: {}", e.without_url())))?.to_vec(), ct)
        }
    };
    if bytes.is_empty() {
        return Err(Error::invalid("сервис вернул пустую картинку"));
    }
    let name = file_name(prompt, ext_of(&ctype));
    let saved = uploads::store(&app.cfg.data_dir, &name, &ctype, &bytes).await.map_err(Error::invalid)?;

    // The person sees what was made: the picture is attached to the specialist's own message in the chat.
    if let Some(mid) = ctx.session_id.as_deref().and_then(|s| app.store.get_session(s).ok()).and_then(|s| s.message_id) {
        if let Ok(m) = app.store.get_message(&mid) {
            let mut list = m.props["attachments"].as_array().cloned().unwrap_or_default();
            list.push(saved.clone());
            if let Ok(m) = app.store.update_message(&mid, None, Some(&json!({"attachments": list})), None) {
                app.publish("message", serde_json::to_value(&m).unwrap_or_default());
            }
        }
    }
    app.store
        .note_access_use(&it.id, &ctx.agent_id, prompt, "картинка")
        .ok();
    Ok(json!({
        "status": "generated",
        "path": saved["path"],
        "name": saved["name"],
        "size_bytes": saved["size"],
        "service": it.name,
        "model": if model.is_empty() { Value::Null } else { json!(model) },
        "ms": started.elapsed().as_millis() as u64,
        "note": "Картинка сохранена на этом компьютере и показана человеку в чате. Используй её по пути из path: скопируй в папку проекта, если она нужна в работе."
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_says_what_was_asked_for() {
        let b = request_body("gpt-image-1", "кот в шляпе", "1024x1024", 1);
        assert_eq!(b["model"], "gpt-image-1");
        assert_eq!(b["prompt"], "кот в шляпе");
        assert_eq!(b["size"], "1024x1024");
        assert_eq!(b["n"], 1);
        // a service with one model of its own needs neither field
        let b = request_body("", "кот", "", 1);
        assert!(b.get("model").is_none() && b.get("size").is_none());
    }

    #[test]
    fn a_picture_is_found_however_the_service_spells_it() {
        let inline = json!({"data": [{"b64_json": "QUJD"}]});
        assert_eq!(find_picture(&inline), Some(Picture::Inline("QUJD".into())));
        let link = json!({"data": [{"url": "https://cdn.example/a.png"}]});
        assert_eq!(find_picture(&link), Some(Picture::Link("https://cdn.example/a.png".into())));
        assert_eq!(find_picture(&json!({"images": [{"image_base64": "QUJD"}]})), Some(Picture::Inline("QUJD".into())));
        assert_eq!(find_picture(&json!({"images": [{"url": {"url": "https://x/y.webp"}}]})), Some(Picture::Link("https://x/y.webp".into())));
        assert_eq!(find_picture(&json!({"b64_json": "QUJD"})), Some(Picture::Inline("QUJD".into())));
        assert_eq!(find_picture(&json!({"data": [{"url": "data:image/png;base64,QUJD"}]})), Some(Picture::Inline("QUJD".into())));
        // nothing that looks like a picture
        assert_eq!(find_picture(&json!({"error": {"message": "no credits"}})), None);
        assert_eq!(find_picture(&json!({"data": []})), None);
    }

    #[test]
    fn base64_decodes_what_the_services_send() {
        assert_eq!(decode_base64("QUJD").unwrap(), b"ABC");
        assert_eq!(decode_base64("QUJDRA==").unwrap(), b"ABCD");
        assert_eq!(decode_base64("QU\nJD").unwrap(), b"ABC", "newlines inside are ignored");
        assert_eq!(decode_base64("iVBORw0KGgo=").unwrap(), vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], "a PNG header survives");
        assert!(decode_base64("не base64").is_none());
    }

    #[test]
    fn the_file_is_named_after_the_picture() {
        assert_eq!(file_name("Морской круиз на закате, акварель", "png"), "морской-круиз-на-закате-акварель.png");
        assert_eq!(file_name("   ", "png"), "image.png");
        assert!(file_name(&"слово ".repeat(40), "jpg").len() < 80);
        assert_eq!(ext_of("image/jpeg"), "jpg");
        assert_eq!(ext_of("image/webp"), "webp");
        assert_eq!(ext_of(""), "png");
    }
}
