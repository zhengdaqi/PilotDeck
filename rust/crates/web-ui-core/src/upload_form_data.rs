//! Port of the decision logic in `ui/src/utils/uploadFormData.ts` (`postFormDataWithProgress`); the XHR itself
//! is the host's. Each function is one XHR callback.
use serde_json::Value;

/// `Math.round` (ties toward +Infinity).
fn js_round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 {
        f + 1.0
    } else {
        f
    }
}

/// `xhr.upload.onprogress`: `Some((percent, uploadedBytes, totalBytes))` when `onProgress` is called.
pub fn progress_event(length_computable: bool, loaded: f64, total: f64, known_total_bytes: f64) -> Option<(f64, f64, f64)> {
    let total = if length_computable && total > 0.0 { total } else { known_total_bytes };
    if total <= 0.0 {
        return None;
    }
    let percent = (js_round(loaded / total * 10000.0) / 100.0).min(100.0);
    Some((percent, loaded.min(total), total))
}

/// `xhr.upload.onload`: `onProgress(100, known, known)` then `onTransferred?.()`.
pub fn upload_complete(known_total_bytes: f64) -> (f64, f64, f64) {
    (100.0, known_total_bytes, known_total_bytes)
}

/// The `Authorization` header set before `send` (`!IS_PLATFORM && token`).
pub fn authorization_header(is_platform: bool, stored_token: Option<&str>) -> Option<String> {
    match stored_token {
        Some(t) if !is_platform && !t.is_empty() => Some(format!("Bearer {t}")),
        _ => None,
    }
}

pub struct UploadResponse {
    pub ok: bool,
    pub status: u16,
    pub body: Value,
    /// A refreshed token to store as `auth-token`.
    pub store_token: Option<String>,
}

/// `xhr.onload`.
pub fn response(status: u16, response_text: &str, refreshed_token: Option<&str>) -> UploadResponse {
    let text = if response_text.is_empty() { "{}" } else { response_text };
    UploadResponse {
        ok: (200..300).contains(&status),
        status,
        body: serde_json::from_str(text).unwrap_or_else(|_| Value::Object(Default::default())),
        store_token: refreshed_token.filter(|t| !t.is_empty()).map(str::to_string),
    }
}

pub const NETWORK_ERROR: (&str, &str) = ("UPLOAD_NETWORK_ERROR", "Upload failed. Check your connection and retry.");
pub const TIMEOUT_ERROR: (&str, &str) = ("UPLOAD_TIMEOUT", "Upload timed out. Please retry.");
pub const ABORTED: &str = "Upload aborted";
