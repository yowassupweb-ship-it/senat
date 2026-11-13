//! Subscription usage of the agent accounts on this node: the five-hour and the weekly window of Claude,
//! ChatGPT (Codex) and Kimi Code, as percent used and when each window resets.
//!
//! Each CLI keeps its own login in the home folder; this module reads the access token from there and asks the
//! provider the same question the CLI's own `/usage` or `/status` asks. It never refreshes a token: refresh
//! tokens rotate, and refreshing behind the CLI's back would log it out. An expired token simply means "ask
//! later" — the CLI renews it on its next run, and the last known figures stay on screen with their time.
//! Claude also reports its windows during a run (`accounts::note_limits`); both paths write the same shape:
//! `{"windows": {"five_hour": {"used": 12, "resets_at": <unix sec>}, "seven_day": {...}}, "at": <ms>, "plan": ""}`.

use crate::App;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn home() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default()
}

fn read_json(p: PathBuf) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

fn now_sec() -> i64 {
    senate_core::now_ms() / 1000
}

fn window(used: f64, resets_at: i64) -> Value {
    json!({"used": used.clamp(0.0, 100.0).round(), "resets_at": resets_at})
}

/// Seconds since the epoch from a number (sec or ms) or an RFC 3339 string.
fn epoch(v: &Value) -> i64 {
    if let Some(n) = v.as_f64() {
        return if n > 1e12 { (n / 1000.0) as i64 } else { n as i64 };
    }
    v.as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp()).unwrap_or(0)
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// Claude: `GET /api/oauth/usage` → `{five_hour: {utilization: 0..100, resets_at}, seven_day: {...}, ...}`.
pub(crate) fn parse_claude(v: &Value) -> Map<String, Value> {
    let mut m = Map::new();
    for k in ["five_hour", "seven_day", "seven_day_opus", "seven_day_sonnet"] {
        if let Some(u) = v[k]["utilization"].as_f64() {
            m.insert(k.into(), window(u, epoch(&v[k]["resets_at"])));
        }
    }
    m
}

/// ChatGPT: `GET /backend-api/wham/usage` → `rate_limit.primary_window` (five hours) and `secondary_window` (week).
pub(crate) fn parse_codex(v: &Value) -> Map<String, Value> {
    let mut m = Map::new();
    for w in ["primary_window", "secondary_window"] {
        let x = &v["rate_limit"][w];
        let Some(used) = x["used_percent"].as_f64() else { continue };
        let secs = x["limit_window_seconds"].as_i64().unwrap_or(0);
        let key = if secs > 0 { key_for_minutes(secs as f64 / 60.0) } else if w == "primary_window" { "five_hour" } else { "seven_day" };
        m.insert(key.into(), window(used, epoch(&x["reset_at"])));
    }
    m
}

fn kimi_detail(d: &Value) -> Option<(f64, i64)> {
    let limit = num(&d["limit"]).filter(|l| *l > 0.0)?;
    let used = num(&d["used"]).or_else(|| num(&d["remaining"]).map(|r| limit - r))?;
    Some((used / limit * 100.0, epoch(&d["resetTime"]).max(epoch(&d["reset_time"])).max(epoch(&d["resetAt"]))))
}

/// The window key by its length: up to six hours is the short one, up to eight days the week, longer the month.
fn key_for_minutes(minutes: f64) -> &'static str {
    if minutes <= 360.0 {
        "five_hour"
    } else if minutes <= 8.0 * 1440.0 {
        "seven_day"
    } else {
        "month"
    }
}

/// Kimi Code: `GET {base}/usages` → `usage` (the plan's monthly quota) and `limits[]`, each `{window: {duration,
/// timeUnit}, detail: {limit, used | remaining, resetTime}}` (the five-hour one among them).
pub(crate) fn parse_kimi(v: &Value) -> Map<String, Value> {
    let mut m = Map::new();
    // The current answer: `usages.limit_5h` and `usages.limit_month_total`, each `{used_ratio: 0..1, reset_time}`.
    for (src, key) in [("limit_5h", "five_hour"), ("limit_month_total", "month")] {
        let u = &v["usages"][src];
        if let Some(r) = u["used_ratio"].as_f64() {
            m.insert(key.into(), window(r * 100.0, epoch(&u["reset_time"])));
        }
    }
    if m.contains_key("month") && m.contains_key("five_hour") {
        return m;
    }
    if let Some((u, r)) = kimi_detail(&v["usage"]) {
        m.entry("month".to_string()).or_insert(window(u, r));
    }
    for l in v["limits"].as_array().into_iter().flatten() {
        let d = if l["detail"].is_object() { &l["detail"] } else { l };
        let Some((u, r)) = kimi_detail(d) else { continue };
        let n = num(&l["window"]["duration"]).unwrap_or(0.0);
        let unit = l["window"]["timeUnit"].as_str().unwrap_or("");
        let minutes = if unit.contains("HOUR") { n * 60.0 } else if unit.contains("DAY") { n * 1440.0 } else if unit.contains("SECOND") { n / 60.0 } else { n };
        let key = if minutes > 0.0 { key_for_minutes(minutes) } else { "five_hour" };
        m.entry(key.to_string()).or_insert(window(u, r));
    }
    m
}

async fn get(url: &str, headers: &[(&str, String)]) -> Option<Value> {
    let c = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().ok()?;
    let mut r = c.get(url);
    for (k, v) in headers {
        r = r.header(*k, v);
    }
    let resp = r.send().await.ok()?;
    if !resp.status().is_success() {
        tracing::debug!("usage {url}: {}", resp.status());
        return None;
    }
    resp.json().await.ok()
}

/// Asks one provider; None when there is no login, the token has expired, or the answer has no windows.
async fn fetch(provider: &str) -> Option<(Map<String, Value>, String)> {
    let h = home();
    match provider {
        "claude" => {
            let c = read_json(h.join(".claude").join(".credentials.json"))?;
            let o = &c["claudeAiOauth"];
            let token = o["accessToken"].as_str()?;
            if epoch(&o["expiresAt"]) > 0 && epoch(&o["expiresAt"]) <= now_sec() + 30 {
                return None;
            }
            let v = get("https://api.anthropic.com/api/oauth/usage", &[("Authorization", format!("Bearer {token}")), ("anthropic-beta", "oauth-2025-04-20".into())]).await?;
            Some((parse_claude(&v), o["subscriptionType"].as_str().unwrap_or("").to_string()))
        }
        "codex" => {
            let a = read_json(h.join(".codex").join("auth.json"))?;
            let t = &a["tokens"];
            let token = t["access_token"].as_str()?;
            let v = get("https://chatgpt.com/backend-api/wham/usage", &[("Authorization", format!("Bearer {token}")), ("ChatGPT-Account-Id", t["account_id"].as_str().unwrap_or("").to_string()), ("User-Agent", "codex_cli_rs".into())]).await?;
            Some((parse_codex(&v), v["plan_type"].as_str().unwrap_or("").to_string()))
        }
        "kimi" => {
            let dir = [".kimi-code", ".kimi"].iter().map(|d| h.join(d)).find(|d| d.join("credentials").join("kimi-code.json").exists())?;
            let c = read_json(dir.join("credentials").join("kimi-code.json"))?;
            let token = c["access_token"].as_str()?;
            if epoch(&c["expires_at"]) > 0 && epoch(&c["expires_at"]) <= now_sec() + 30 {
                return None;
            }
            let v = get("https://api.kimi.com/coding/v1/usages", &[("Authorization", format!("Bearer {token}"))]).await?;
            let m = parse_kimi(&v);
            if m.is_empty() {
                let keys: Vec<&String> = v.as_object().map(|o| o.keys().collect()).unwrap_or_default();
                tracing::info!("kimi usage: не узнал ответ, ключи {keys:?}");
            }
            Some((m, String::new()))
        }
        _ => None,
    }
}

pub(crate) async fn refresh(app: &App, provider: &str) {
    let Some((windows, plan)) = fetch(provider).await else { return };
    if windows.is_empty() {
        return;
    }
    app.limits.write().unwrap().insert(provider.to_string(), json!({"windows": windows, "at": senate_core::now_ms(), "plan": plan}));
    crate::accounts::publish(app);
}

/// Every few minutes: cheap (one small request per logged-in provider) and skipped while a token is stale.
pub(crate) fn spawn(app: &Arc<App>) {
    let a = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        loop {
            for p in ["claude", "codex", "kimi"] {
                refresh(&a, p).await;
            }
            tokio::time::sleep(Duration::from_secs(180)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_are_read_from_each_provider() {
        let c = parse_claude(&json!({"five_hour": {"utilization": 12.4, "resets_at": "2026-11-13T15:00:00Z"}, "seven_day": {"utilization": 40.0, "resets_at": null}, "seven_day_opus": null}));
        assert_eq!(c["five_hour"]["used"], 12.0);
        assert!(c["five_hour"]["resets_at"].as_i64().unwrap() > 1_700_000_000);
        assert_eq!(c.len(), 2);

        let x = parse_codex(&json!({"rate_limit": {"primary_window": {"used_percent": 0, "limit_window_seconds": 18000, "reset_at": 1794586159}, "secondary_window": {"used_percent": 40, "limit_window_seconds": 604800, "reset_at": 1794849410}}}));
        assert_eq!(x["five_hour"]["used"], 0.0);
        assert_eq!(x["seven_day"]["used"], 40.0);
        assert_eq!(x["seven_day"]["resets_at"], 1794849410);

        let k = parse_kimi(&json!({"usage": {"limit": "100", "remaining": "75", "resetTime": "2026-11-20T00:00:00Z"}, "limits": [{"window": {"duration": 300, "timeUnit": "TIME_UNIT_MINUTE"}, "detail": {"limit": "50", "used": "10", "resetTime": "2026-11-13T18:00:00Z"}}]}));
        assert_eq!(k["month"]["used"], 25.0);
        assert_eq!(k["five_hour"]["used"], 20.0);
        assert!(parse_kimi(&json!({"odd": 1})).is_empty());

        // what Kimi Code answers now (November 2026): ratios under `usages`
        let k2 = parse_kimi(&json!({
            "limits": [{"window": {"duration": 300, "timeUnit": "TIME_UNIT_MINUTE"}, "detail": {"limit": "100", "remaining": "100", "resetTime": "2026-11-13T13:12:35Z"}}],
            "usages": {"limit_5h": {"used_ratio": 0.0027, "reset_time": "2026-11-13T13:12:34Z"}, "limit_month_total": {"used_ratio": 0.0313, "reset_time": "2026-12-13T00:00:00Z"}, "limit_month_code": {"used_ratio": 0.0072, "reset_time": "2026-12-13T00:00:00Z"}}
        }));
        assert_eq!(k2["five_hour"]["used"], 0.0);
        assert_eq!(k2["month"]["used"], 3.0);
        assert!(k2["month"]["resets_at"].as_i64().unwrap() > k2["five_hour"]["resets_at"].as_i64().unwrap());
    }
}
