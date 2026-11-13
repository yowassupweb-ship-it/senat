//! Looking at a page a specialist started on the server.
//!
//! A specialist working on the node runs a dev server on the node's own `localhost`, which the person's computer
//! cannot reach. The node passes such a page through itself — `/preview/<port>/…` — so the person opens it through
//! the same link (the SSH tunnel) the app already uses. Only loopback ports of the node are reachable this way.
//!
//! Access: the app asks `/api/preview/<port>` (with the node token) for a link carrying a key; the first visit
//! trades the key for a cookie limited to that port's path, so the page's own requests need nothing more. The key
//! changes every time the node starts, and the node token never appears in a link.

use crate::App;
use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use std::sync::Arc;
use std::time::Duration;

const COOKIE: &str = "senate_pv";
const BODY_LIMIT: usize = 20 << 20;

pub(crate) fn new_key() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn port_ok(a: &App, port: u16) -> bool {
    port >= 80 && port != *a.port.read().unwrap()
}

/// The link the app opens: relative to the node's address.
pub(crate) fn link(a: &App, port: u16, path: &str) -> Option<String> {
    if !port_ok(a, port) {
        return None;
    }
    let p = path.trim_start_matches('/');
    Some(format!("/preview/{port}/{p}{}k={}", if p.contains('?') { "&" } else { "?" }, a.preview_key))
}

fn cookie_key(req: &Request) -> Option<String> {
    let c = req.headers().get(header::COOKIE)?.to_str().ok()?;
    c.split(';').find_map(|kv| kv.trim().strip_prefix(&format!("{COOKIE}="))).map(String::from)
}

/// Make root-relative addresses in a page point back under `/preview/<port>/`.
pub(crate) fn rewrite_html(html: &str, port: u16) -> String {
    let pre = format!("/preview/{port}/");
    let mut out = html.to_string();
    for attr in ["src", "href", "action"] {
        for q in ['"', '\''] {
            let from = format!("{attr}={q}/");
            let to = format!("{attr}={q}{pre}");
            // leave protocol-relative `//host` alone and do not prefix twice
            let mut res = String::with_capacity(out.len());
            let mut rest = out.as_str();
            while let Some(i) = rest.find(&from) {
                let after = &rest[i + from.len()..];
                res.push_str(&rest[..i]);
                if after.starts_with('/') || after.starts_with(&pre[1..]) {
                    res.push_str(&from);
                } else {
                    res.push_str(&to);
                }
                rest = after;
            }
            res.push_str(rest);
            out = res;
        }
    }
    out
}

pub(crate) async fn handle(State(a): State<Arc<App>>, Path(params): Path<Vec<(String, String)>>, req: Request) -> Response {
    let port: u16 = params.iter().find(|(k, _)| k == "port").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    let rest = params.iter().find(|(k, _)| k == "rest").map(|(_, v)| v.clone()).unwrap_or_default();
    if !port_ok(&a, port) {
        return (StatusCode::BAD_REQUEST, "недопустимый порт").into_response();
    }
    let query = req.uri().query().unwrap_or("").to_string();
    let key_in_link = query.split('&').find_map(|kv| kv.strip_prefix("k=")).map(String::from);
    // first visit: the key from the link becomes a cookie for this port, and the link is cleaned of it
    if key_in_link.as_deref() == Some(a.preview_key.as_str()) {
        let clean: Vec<&str> = query.split('&').filter(|kv| !kv.starts_with("k=") && !kv.is_empty()).collect();
        let to = format!("/preview/{port}/{rest}{}", if clean.is_empty() { String::new() } else { format!("?{}", clean.join("&")) });
        let mut r = Response::new(Body::empty());
        *r.status_mut() = StatusCode::FOUND;
        r.headers_mut().insert(header::LOCATION, HeaderValue::from_str(&to).unwrap_or(HeaderValue::from_static("/")));
        if let Ok(v) = HeaderValue::from_str(&format!("{COOKIE}={}; Path=/preview/{port}/; HttpOnly; SameSite=Lax", a.preview_key)) {
            r.headers_mut().insert(header::SET_COOKIE, v);
        }
        return r;
    }
    if cookie_key(&req).as_deref() != Some(a.preview_key.as_str()) {
        return (StatusCode::UNAUTHORIZED, "Откройте страницу по кнопке в Senate").into_response();
    }

    let url = format!("http://127.0.0.1:{port}/{rest}{}", if query.is_empty() { String::new() } else { format!("?{query}") });
    let method = req.method().clone();
    let mut headers = req.headers().clone();
    for h in [header::HOST, header::COOKIE, header::ACCEPT_ENCODING, header::CONNECTION] {
        headers.remove(h);
    }
    let body = match axum::body::to_bytes(req.into_body(), BODY_LIMIT).await {
        Ok(b) => b,
        Err(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "слишком большой запрос").into_response(),
    };
    let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(60)).build().unwrap();
    let resp = match client.request(method, &url).headers(headers).body(body).send().await {
        Ok(r) => r,
        Err(_) => return (StatusCode::BAD_GATEWAY, format!("На сервере ничего не отвечает на порту {port}. Сервер разработки не запущен или остановился.")).into_response(),
    };
    let status = resp.status();
    let mut out_headers = resp.headers().clone();
    for h in [header::TRANSFER_ENCODING, header::CONNECTION, header::CONTENT_LENGTH] {
        out_headers.remove(h);
    }
    if let Some(loc) = out_headers.get(header::LOCATION).and_then(|v| v.to_str().ok()).map(String::from) {
        let fixed = if let Some(p) = loc.strip_prefix(&format!("http://127.0.0.1:{port}")).or_else(|| loc.strip_prefix(&format!("http://localhost:{port}"))) {
            format!("/preview/{port}{}", if p.is_empty() { "/" } else { p })
        } else if loc.starts_with('/') && !loc.starts_with("//") {
            format!("/preview/{port}{loc}")
        } else {
            loc
        };
        if let Ok(v) = HeaderValue::from_str(&fixed) {
            out_headers.insert(header::LOCATION, v);
        }
    }
    let is_html = out_headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(|t| t.starts_with("text/html")).unwrap_or(false);
    let bytes = resp.bytes().await.unwrap_or_default();
    let body = if is_html { Body::from(rewrite_html(&String::from_utf8_lossy(&bytes), port)) } else { Body::from(bytes) };
    let mut r = Response::new(body);
    *r.status_mut() = status;
    *r.headers_mut() = out_headers;
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_relative_addresses_stay_under_the_preview() {
        let h = r#"<link href="/style.css"><script src='/app.js'></script><a href="//cdn.x/y">c</a><a href="/preview/3006/ok">o</a><img src="pic.png">"#;
        let out = rewrite_html(h, 3006);
        assert!(out.contains(r#"href="/preview/3006/style.css""#));
        assert!(out.contains("src='/preview/3006/app.js'"));
        assert!(out.contains(r#"href="//cdn.x/y""#), "protocol-relative untouched");
        assert!(out.contains(r#"href="/preview/3006/ok""#) && !out.contains("/preview/3006/preview/"), "not prefixed twice");
        assert!(out.contains(r#"src="pic.png""#), "relative untouched");
    }
}
