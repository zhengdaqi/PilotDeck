//! Snapshot validation, key normalization and entry merging shared by the SDK stores
//! (`normalizeSessionKey`, `validateSnapshot`, `isSessionStoreEntry`, `mergeEntries`, `nextMtime` in session-store.ts).
use crate::js::trim;
use crate::session_store::truthy;
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum StoreError {
    /// `SessionStoreError` with code SESSION_STORE_INVALID / SESSION_STORE_CONFLICT.
    Store { code: &'static str, message: String },
    /// A raw (non-SessionStoreError) failure, e.g. an unexpected unlink error.
    Raw(String),
}

impl StoreError {
    pub fn invalid(m: impl Into<String>) -> Self {
        StoreError::Store { code: "SESSION_STORE_INVALID", message: m.into() }
    }
    pub fn message(&self) -> &str {
        match self {
            StoreError::Store { message, .. } | StoreError::Raw(message) => message,
        }
    }
    pub fn code(&self) -> Option<&'static str> {
        match self {
            StoreError::Store { code, .. } => Some(code),
            StoreError::Raw(_) => None,
        }
    }
}

fn non_empty_str(v: Option<&Value>) -> Option<&str> {
    match v {
        Some(Value::String(s)) if !trim(s).is_empty() => Some(s),
        _ => None,
    }
}

/// `normalizeSessionKey(key)`, returning the normalized JSON key `{projectKey, sessionId, subpath?}`.
pub fn normalize_session_key(key: &Value) -> Result<Value, StoreError> {
    let get = |k: &str| key.as_object().and_then(|o| o.get(k));
    let pk = non_empty_str(get("projectKey")).ok_or_else(|| StoreError::invalid("SessionStore projectKey must be a non-empty string."))?;
    let sid = non_empty_str(get("sessionId")).ok_or_else(|| StoreError::invalid("SessionStore sessionId must be a non-empty string."))?;
    let mut out = Map::new();
    out.insert("projectKey".into(), json!(pk));
    out.insert("sessionId".into(), json!(sid));
    match get("subpath") {
        None => {}
        Some(v) => {
            let sp = non_empty_str(Some(v)).ok_or_else(|| StoreError::invalid("SessionStore subpath must be a non-empty string when provided."))?;
            out.insert("subpath".into(), json!(sp));
        }
    }
    Ok(Value::Object(out))
}

/// `keyOf(key)` for a normalized key.
pub fn key_of(key: &Value) -> String {
    let s = |k: &str| key[k].as_str().unwrap_or("").to_string();
    format!("{}\0{}\0{}", s("projectKey"), s("sessionId"), s("subpath"))
}

pub fn is_session_store_entry(v: &Value) -> bool {
    matches!(v.get("type"), Some(Value::String(t)) if !trim(t).is_empty()) && v.is_object()
}

/// `validateSnapshot(value)` -> normalized snapshot JSON.
pub fn validate_snapshot(value: &Value) -> Result<Value, StoreError> {
    if !truthy(value) || !(value.is_object() || value.is_array()) {
        return Err(StoreError::invalid("SessionStore snapshot must be an object."));
    }
    let get = |k: &str| value.as_object().and_then(|o| o.get(k));
    if get("schemaVersion").and_then(|v| v.as_f64()) != Some(1.0) {
        return Err(StoreError::invalid("SessionStore snapshot has an unsupported schemaVersion."));
    }
    let key = normalize_session_key(get("key").unwrap_or(&Value::Null))?;
    let mtime = match get("mtime") {
        Some(Value::Number(n)) if n.as_f64().is_some_and(|f| f.is_finite() && f >= 0.0) => n.clone(),
        _ => return Err(StoreError::invalid("SessionStore snapshot mtime must be a non-negative finite number.")),
    };
    let entries = match get("entries") {
        Some(Value::Array(a)) if a.iter().all(is_session_store_entry) => a.clone(),
        _ => return Err(StoreError::invalid("SessionStore snapshot entries must be typed entry objects.")),
    };
    Ok(json!({"schemaVersion": 1, "key": key, "mtime": mtime, "entries": entries}))
}

fn seen_key(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(format!("s{s}")),
        Value::Number(n) => Some(format!("n{}", n.as_f64().unwrap_or(0.0))),
        Value::Bool(b) => Some(format!("b{b}")),
        _ => None,
    }
}

/// `mergeEntries(existing, incoming)` -> (entries, imported, skipped).
pub fn merge_entries(existing: &[Value], incoming: &[Value]) -> Result<(Vec<Value>, u64, u64), StoreError> {
    let mut entries = existing.to_vec();
    let mut seen: Vec<String> = entries.iter().filter_map(|e| match e.get("uuid") {
        Some(Value::String(s)) => Some(format!("s{s}")),
        _ => None,
    }).collect();
    let (mut imported, mut skipped) = (0, 0);
    for entry in incoming {
        if !is_session_store_entry(entry) {
            return Err(StoreError::invalid("SessionStore entries must contain a non-empty type."));
        }
        let uuid = entry.get("uuid").filter(|u| truthy(u));
        let k = uuid.and_then(seen_key);
        if let Some(k) = &k {
            if seen.contains(k) {
                skipped += 1;
                continue;
            }
        }
        entries.push(entry.clone());
        imported += 1;
        if let Some(k) = k {
            seen.push(k);
        }
    }
    Ok((entries, imported, skipped))
}

/// `nextMtime(previous)` with the host's `Date.now()`.
pub fn next_mtime(previous: Option<f64>, now: f64) -> f64 {
    now.max(previous.unwrap_or(0.0) + 1.0)
}

/// A JSON number for an mtime (integers stay integers, like JS).
pub fn mtime_json(f: f64) -> Value {
    if f.fract() == 0.0 && f.abs() < 9.0e15 {
        json!(f as i64)
    } else {
        json!(f)
    }
}
