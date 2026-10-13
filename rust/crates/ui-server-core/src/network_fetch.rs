//! Port of src/network/fetch.ts `networkFetch` / `networkFetchJson` / `jitteredBackoff` / `isRetryableNetworkCode`.
//! Sans-IO: the host performs the request, sleeps, and supplies `Math.random()` and `Date.now()`.
//! Pending (host slice): abort/timeout signals (`options.signal`, `timeoutMs`) and the "Network retry aborted." path.
//! Retry-After dates: the IMF-fixdate form (`Tue, 13 Oct 2026 07:00:05 GMT`, what servers send) is parsed; V8's
//! lenient legacy `Date.parse` formats are not (recorded as a gap; such headers fall back to backoff).
use crate::js_number::{js_number_to_string, js_trim, number};
use crate::model_probe::{normalize_network_error, FetchFailure, NetworkFetchError};
use serde_json::Value;
use std::future::Future;

#[derive(Clone, Debug, Default)]
pub struct RetryOptions {
    pub max_retries: Option<f64>,
    pub base_delay_ms: Option<f64>,
    pub max_delay_ms: Option<f64>,
    pub retry_on_post: bool,
    pub retry_statuses: Option<Vec<f64>>,
}

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    /// `headers.get("retry-after")`: the host passes the value as Fetch `Headers` exposes it (leading and
    /// trailing HTTP whitespace already stripped).
    pub retry_after: Option<String>,
    pub text: String,
}

pub trait FetchHost {
    /// `method` is `init.method` as given (`None` = not set); the request goes out with it unchanged.
    fn fetch(&mut self, method: Option<&str>) -> impl Future<Output = Result<HttpResponse, FetchFailure>>;
    fn sleep(&mut self, ms: f64) -> impl Future<Output = ()>;
    /// `Math.random()`
    fn random(&mut self) -> f64;
    /// `Date.now()`
    fn now_ms(&mut self) -> f64;
}

const DEFAULT_BASE_DELAY_MS: f64 = 1000.0;
const DEFAULT_MAX_DELAY_MS: f64 = 30_000.0;
const DEFAULT_RETRY_STATUSES: [u16; 8] = [408, 409, 425, 429, 500, 502, 503, 504];

pub fn is_retryable_network_code(code: &str) -> bool {
    code != "network_abort" && code != "network_tls_error"
}

fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// `Date.parse` restricted to IMF-fixdate (RFC 9110 §5.6.7).
pub fn parse_imf_fixdate(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    if b.len() != 29 || &b[3..5] != b", " || b[7] != b' ' || b[11] != b' ' || b[16] != b' ' || b[19] != b':' || b[22] != b':' || &b[25..] != b" GMT" {
        return None;
    }
    if !["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].contains(&&s[0..3]) {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r).filter(|t| t.bytes().all(|c| c.is_ascii_digit())).and_then(|t| t.parse::<i64>().ok());
    let day = num(5..7)?;
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"].iter().position(|m| *m == &s[8..11])? as i64 + 1;
    let (year, h, mi, se) = (num(12..16)?, num(17..19)?, num(20..22)?, num(23..25)?);
    if !(1..=31).contains(&day) || h > 24 || mi > 59 || se > 59 {
        return None;
    }
    // days from civil (Howard Hinnant)
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 86_400 + h * 3600 + mi * 60 + se) * 1000) as f64)
}

fn parse_retry_after(header: Option<&str>, now_ms: f64) -> Option<f64> {
    let h = header.filter(|h| !h.is_empty())?;
    let seconds = number(js_trim(h));
    if seconds.is_finite() && seconds >= 0.0 {
        return Some(js_round(seconds * 1000.0));
    }
    let date = parse_imf_fixdate(h)?;
    let delta = date - now_ms;
    (delta > 0.0).then_some(delta)
}

/// `resolveRetryDelay` / `jitteredBackoff`. `random` is only consulted when no Retry-After applies.
pub fn jittered_backoff(attempt: f64, retry: &RetryOptions, retry_after: Option<&str>, now_ms: f64, random: &mut dyn FnMut() -> f64) -> f64 {
    let cap = retry.max_delay_ms.unwrap_or(DEFAULT_MAX_DELAY_MS);
    if let Some(ra) = parse_retry_after(retry_after, now_ms) {
        return cap.min(ra);
    }
    let base = retry.base_delay_ms.unwrap_or(DEFAULT_BASE_DELAY_MS);
    let exponential = cap.min(base * 2f64.powf(attempt));
    let jitter = (random() * 1f64.max((exponential * 0.25).floor())).floor();
    cap.min(exponential + jitter)
}

fn should_retry_status(status: u16, configured: Option<&[f64]>) -> bool {
    match configured {
        Some(c) => c.contains(&(status as f64)),
        None => DEFAULT_RETRY_STATUSES.contains(&status),
    }
}

/// `networkFetch(input, { method }, { retry })`.
pub async fn network_fetch<H: FetchHost>(h: &mut H, method: Option<&str>, retry: &RetryOptions) -> Result<HttpResponse, NetworkFetchError> {
    let m = retry.max_retries.unwrap_or(0.0);
    let max_retries = if m.is_nan() { f64::NAN } else { m.max(0.0) };
    let upper = method.unwrap_or("GET").to_uppercase();
    let can_retry_method = matches!(upper.as_str(), "GET" | "HEAD" | "OPTIONS") || retry.retry_on_post;
    let mut last_error: Option<FetchFailure> = None;
    let mut attempt = 0.0;
    while attempt <= max_retries {
        match h.fetch(method).await {
            Ok(response) => {
                if can_retry_method && attempt < max_retries && should_retry_status(response.status, retry.retry_statuses.as_deref()) {
                    let now = h.now_ms();
                    let d = jittered_backoff(attempt, retry, response.retry_after.as_deref(), now, &mut || h.random());
                    h.sleep(d).await;
                    attempt += 1.0;
                    continue;
                }
                return Ok(response);
            }
            Err(e) => {
                let normalized = normalize_network_error(&e);
                last_error = Some(e);
                if !can_retry_method || attempt >= max_retries || !is_retryable_network_code(normalized.code) {
                    return Err(normalized);
                }
                let now = h.now_ms();
                let d = jittered_backoff(attempt, retry, None, now, &mut || h.random());
                h.sleep(d).await;
            }
        }
        attempt += 1.0;
    }
    Err(normalize_network_error(&last_error.unwrap_or(FetchFailure::Other(None))))
}

/// `text.slice(0, n)` in UTF-16 code units (a split surrogate pair becomes U+FFFD; JS keeps a lone surrogate).
pub fn js_slice_units(text: &str, n: usize) -> String {
    let mut out = String::new();
    let mut units = 0;
    for c in text.chars() {
        let w = c.len_utf16();
        if units + w > n {
            if units < n {
                out.push('\u{FFFD}');
            }
            break;
        }
        out.push(c);
        units += w;
    }
    out
}

/// `networkFetchJson(input, { method }, { retry, expectedStatuses })` → (response, json).
pub async fn network_fetch_json<H: FetchHost>(h: &mut H, input: &str, method: Option<&str>, retry: &RetryOptions, expected_statuses: Option<&[u16]>) -> Result<(HttpResponse, Value), NetworkFetchError> {
    let response = network_fetch(h, method, retry).await?;
    let ok = match expected_statuses {
        Some(e) => e.contains(&response.status),
        None => (200..300).contains(&response.status),
    };
    if !ok {
        let code = if response.status == 429 {
            "network_rate_limited"
        } else if response.status >= 500 {
            "network_server_error"
        } else {
            "network_fetch_failed"
        };
        return Err(NetworkFetchError { code, message: format!("HTTP {} {}: {}", response.status, response.status_text, js_slice_units(&response.text, 500)) });
    }
    match serde_json::from_str::<Value>(&response.text) {
        Ok(v) => Ok((response, v)),
        Err(_) => Err(NetworkFetchError { code: "network_fetch_failed", message: format!("Non-JSON response from {input}: {}", js_slice_units(&response.text, 500)) }),
    }
}

/// `String(ms)` as logged by hosts and tests.
pub fn delay_label(ms: f64) -> String {
    js_number_to_string(ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slice_units_split_pair_is_replacement_char() {
        // JS keeps a lone high surrogate here; Rust strings cannot, so it becomes U+FFFD (lone-surrogate gap).
        assert_eq!(js_slice_units("ab😀c", 3), "ab\u{FFFD}");
        assert_eq!(js_slice_units("ab😀c", 4), "ab😀");
        assert_eq!(js_slice_units("ab😀c", 2), "ab");
        assert_eq!(parse_imf_fixdate("Tue, 13 Oct 2026 07:00:05 GMT"), Some(1_791_874_805_000.0));
        assert_eq!(parse_imf_fixdate("Tue, 13 Oct 2026 07:00:05 UTC"), None);
    }
}
