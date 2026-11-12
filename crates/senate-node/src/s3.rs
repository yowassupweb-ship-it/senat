//! The cloud: files in an S3-compatible bucket (Yandex Object Storage, MinIO, AWS), the same way MBOX keeps its own.
//! Requests are signed with AWS Signature V4 by hand, so no SDK is pulled in; download links are pre-signed and expire.
//!
//! Settings live in the node (`s3_*` keys); the secret key never goes back to the UI.

use hmac::{Hmac, Mac};
use senate_core::store::Store;
use sha2::{Digest, Sha256};
use std::time::Duration;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Debug)]
pub struct Cfg {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access: String,
    pub secret: String,
    /// Everything of Senate lives under this folder of the bucket ("" = the bucket root).
    pub root: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Obj {
    pub name: String,
    pub key: String,
    pub size: u64,
    pub modified: String,
}

pub fn load(store: &Store) -> Option<Cfg> {
    let g = |k: &str| store.setting(k).ok().flatten().unwrap_or_default().trim().to_string();
    let c = Cfg {
        endpoint: g("s3_endpoint").trim_end_matches('/').to_string(),
        region: if g("s3_region").is_empty() { "ru-central1".into() } else { g("s3_region") },
        bucket: g("s3_bucket"),
        access: g("s3_access_key"),
        secret: g("s3_secret_key"),
        root: normalize_root(&g("s3_prefix")),
    };
    (!c.endpoint.is_empty() && !c.bucket.is_empty() && !c.access.is_empty() && !c.secret.is_empty()).then_some(c)
}

pub fn normalize_root(p: &str) -> String {
    let t = p.trim().trim_matches('/');
    if t.is_empty() {
        String::new()
    } else {
        format!("{t}/")
    }
}

/// A key coming from the UI: no way out of the root, no control characters.
pub fn safe_key(k: &str) -> Option<String> {
    let k = k.trim_start_matches('/');
    if k.len() > 900 || k.chars().any(|c| c.is_control() || c == '\\') || k.split('/').any(|s| s == "..") {
        return None;
    }
    Some(k.to_string())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn sha_hex(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}
fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut m = HmacSha256::new_from_slice(key).expect("hmac key");
    m.update(data.as_bytes());
    m.finalize().into_bytes().to_vec()
}
/// RFC 3986 encoding as S3 wants it (a slash is kept only in paths).
fn enc(s: &str, keep_slash: bool) -> String {
    let mut o = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            b'/' if keep_slash => o.push('/'),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

fn host(c: &Cfg) -> String {
    c.endpoint.split("://").nth(1).unwrap_or(&c.endpoint).to_string()
}
fn path(c: &Cfg, key: &str) -> String {
    if c.bucket.is_empty() {
        // virtual-hosted style (the bucket is in the host name)
        format!("/{}", enc(key, true))
    } else if key.is_empty() {
        format!("/{}", c.bucket)
    } else {
        format!("/{}/{}", c.bucket, enc(key, true))
    }
}
fn canonical_query(q: &[(String, String)]) -> String {
    let mut v: Vec<(String, String)> = q.iter().map(|(k, x)| (enc(k, false), enc(x, false))).collect();
    v.sort();
    v.iter().map(|(k, x)| format!("{k}={x}")).collect::<Vec<_>>().join("&")
}
fn signing_key(c: &Cfg, date: &str) -> Vec<u8> {
    let k = hmac(format!("AWS4{}", c.secret).as_bytes(), date);
    let k = hmac(&k, &c.region);
    let k = hmac(&k, "s3");
    hmac(&k, "aws4_request")
}

/// Headers for one request signed in the Authorization header.
fn signed(c: &Cfg, method: &str, key: &str, query: &[(String, String)], payload: &[u8]) -> (String, Vec<(String, String)>) {
    let now = chrono::Utc::now();
    let amz = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let ph = sha_hex(payload);
    let cq = canonical_query(query);
    let canon = format!("{method}\n{}\n{cq}\nhost:{}\nx-amz-content-sha256:{ph}\nx-amz-date:{amz}\n\nhost;x-amz-content-sha256;x-amz-date\n{ph}", path(c, key), host(c));
    let scope = format!("{date}/{}/s3/aws4_request", c.region);
    let to_sign = format!("AWS4-HMAC-SHA256\n{amz}\n{scope}\n{}", sha_hex(canon.as_bytes()));
    let sig = hex(&hmac(&signing_key(c, &date), &to_sign));
    let auth = format!("AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature={sig}", c.access);
    let url = if cq.is_empty() { format!("{}{}", c.endpoint, path(c, key)) } else { format!("{}{}?{cq}", c.endpoint, path(c, key)) };
    (url, vec![("x-amz-date".into(), amz), ("x-amz-content-sha256".into(), ph), ("authorization".into(), auth)])
}

/// A download link that works without the keys, for `expires` seconds.
pub fn presign(c: &Cfg, key: &str, expires: u64, file_name: Option<&str>) -> String {
    presign_at(c, key, expires, file_name, chrono::Utc::now())
}

fn presign_at(c: &Cfg, key: &str, expires: u64, file_name: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> String {
    let amz = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let scope = format!("{date}/{}/s3/aws4_request", c.region);
    let mut q: Vec<(String, String)> = vec![
        ("X-Amz-Algorithm".into(), "AWS4-HMAC-SHA256".into()),
        ("X-Amz-Credential".into(), format!("{}/{scope}", c.access)),
        ("X-Amz-Date".into(), amz.clone()),
        ("X-Amz-Expires".into(), expires.clamp(60, 604_800).to_string()),
        ("X-Amz-SignedHeaders".into(), "host".into()),
    ];
    if let Some(n) = file_name {
        q.push(("response-content-disposition".into(), format!("attachment; filename*=UTF-8''{}", enc(n, false))));
    }
    let cq = canonical_query(&q);
    let canon = format!("GET\n{}\n{cq}\nhost:{}\n\nhost\nUNSIGNED-PAYLOAD", path(c, key), host(c));
    let to_sign = format!("AWS4-HMAC-SHA256\n{amz}\n{scope}\n{}", sha_hex(canon.as_bytes()));
    let sig = hex(&hmac(&signing_key(c, &date), &to_sign));
    format!("{}{}?{cq}&X-Amz-Signature={sig}", c.endpoint, path(c, key))
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(120)).redirect(reqwest::redirect::Policy::none()).build().unwrap()
}

async fn send(c: &Cfg, method: &str, key: &str, query: &[(String, String)], body: Vec<u8>, ctype: Option<&str>) -> Result<String, String> {
    let (url, hs) = signed(c, method, key, query, &body);
    let mut r = client().request(reqwest::Method::from_bytes(method.as_bytes()).unwrap(), url);
    for (k, v) in hs {
        r = r.header(k, v);
    }
    if let Some(t) = ctype {
        r = r.header("content-type", t);
    }
    let resp = r.body(body).send().await.map_err(|e| format!("хранилище недоступно: {e}"))?;
    let st = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if st.is_success() {
        return Ok(text);
    }
    let code = tag(&text, "Code").unwrap_or_default();
    let msg = tag(&text, "Message").unwrap_or_default();
    Err(match st.as_u16() {
        403 => format!("хранилище отказало в доступе ({code}): проверьте ключи и права на бакет"),
        404 => format!("не найдено ({code}): проверьте бакет и адрес"),
        n => format!("ошибка хранилища {n} {code} {msg}"),
    })
}

fn unxml(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}
fn tag(xml: &str, name: &str) -> Option<String> {
    let a = xml.find(&format!("<{name}>"))? + name.len() + 2;
    let b = xml[a..].find(&format!("</{name}>"))?;
    Some(unxml(&xml[a..a + b]))
}
fn blocks<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let (open, close) = (format!("<{name}>"), format!("</{name}>"));
    let mut out = vec![];
    let mut rest = xml;
    while let Some(a) = rest.find(&open) {
        let after = &rest[a + open.len()..];
        let Some(b) = after.find(&close) else { break };
        out.push(&after[..b]);
        rest = &after[b + close.len()..];
    }
    out
}

/// One folder level: `(sub-folders, files)`, keys relative to the cloud root.
pub async fn list(c: &Cfg, prefix: &str) -> Result<(Vec<String>, Vec<Obj>), String> {
    let full = format!("{}{}", c.root, prefix);
    let q = vec![("list-type".to_string(), "2".to_string()), ("delimiter".into(), "/".into()), ("prefix".into(), full.clone()), ("max-keys".into(), "1000".into())];
    let xml = send(c, "GET", "", &q, vec![], None).await?;
    let rel = |k: &str| k.strip_prefix(c.root.as_str()).unwrap_or(k).to_string();
    let folders = blocks(&xml, "CommonPrefixes").iter().filter_map(|b| tag(b, "Prefix")).map(|p| rel(&p)).collect();
    let files = blocks(&xml, "Contents")
        .iter()
        .filter_map(|b| {
            let key = tag(b, "Key")?;
            if key == full || key.ends_with('/') {
                return None; // the folder's own marker
            }
            Some(Obj { name: key.rsplit('/').next().unwrap_or(&key).to_string(), key: rel(&key), size: tag(b, "Size").and_then(|s| s.parse().ok()).unwrap_or(0), modified: tag(b, "LastModified").unwrap_or_default() })
        })
        .collect();
    Ok((folders, files))
}

pub async fn put(c: &Cfg, key: &str, body: Vec<u8>, ctype: &str) -> Result<(), String> {
    send(c, "PUT", &format!("{}{key}", c.root), &[], body, Some(ctype)).await.map(|_| ())
}

pub async fn delete(c: &Cfg, key: &str) -> Result<(), String> {
    send(c, "DELETE", &format!("{}{key}", c.root), &[], vec![], None).await.map(|_| ())
}

/// A folder and everything in it.
pub async fn delete_folder(c: &Cfg, prefix: &str) -> Result<usize, String> {
    let full = format!("{}{}", c.root, prefix);
    let q = vec![("list-type".to_string(), "2".to_string()), ("prefix".into(), full), ("max-keys".into(), "1000".into())];
    let xml = send(c, "GET", "", &q, vec![], None).await?;
    let keys: Vec<String> = blocks(&xml, "Contents").iter().filter_map(|b| tag(b, "Key")).collect();
    for k in &keys {
        send(c, "DELETE", k, &[], vec![], None).await?;
    }
    Ok(keys.len())
}

pub fn link(c: &Cfg, key: &str, expires: u64, name: Option<&str>) -> String {
    presign(c, &format!("{}{key}", c.root), expires, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// The pre-signed GET example from the AWS documentation (Signature V4 for S3).
    #[test]
    fn presign_matches_the_aws_example() {
        let c = Cfg {
            endpoint: "https://examplebucket.s3.amazonaws.com".into(),
            region: "us-east-1".into(),
            bucket: String::new(),
            access: "AKIAIOSFODNN7EXAMPLE".into(),
            secret: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
            root: String::new(),
        };
        let t = chrono::Utc.with_ymd_and_hms(2013, 5, 24, 0, 0, 0).unwrap();
        let url = presign_at(&c, "test.txt", 86400, None, t);
        assert!(url.ends_with("X-Amz-Signature=aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404"), "{url}");
    }

    #[test]
    fn keys_cannot_escape() {
        assert!(safe_key("../x").is_none());
        assert!(safe_key("a/../x").is_none());
        assert_eq!(safe_key("/a/b.txt").as_deref(), Some("a/b.txt"));
    }

    #[test]
    fn list_xml_is_read() {
        let x = "<R><Contents><Key>p/a &amp; b.txt</Key><Size>12</Size></Contents><CommonPrefixes><Prefix>p/sub/</Prefix></CommonPrefixes></R>";
        assert_eq!(tag(blocks(x, "Contents")[0], "Key").as_deref(), Some("p/a & b.txt"));
        assert_eq!(tag(blocks(x, "CommonPrefixes")[0], "Prefix").as_deref(), Some("p/sub/"));
    }
}
