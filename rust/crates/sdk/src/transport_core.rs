//! Sans-IO core of `packages/sdk/src/transport.ts` (`GatewayTransport`, `mapError`, `normalizeGatewayCode`,
//! `AsyncEventQueue`). The IO layers (native WebSocket, browser WebSocket via sdk-wasm) feed frames and close events
//! in and carry the effects out.
//!
//! Intended difference (SRC-DEFECT-015, ADR-0004): TS throws inside the WebSocket message listener for some malformed
//! frames, which surfaces as an uncaught exception in the host process and leaves the request pending:
//! - a `null` frame;
//! - `ok: false` without an `error` object;
//! - a non-string `error.code`.
//!
//! Rust treats a `null` frame as ignorable, a missing `error` as `{}`, and a non-string code as absent.
use serde_json::{Map, Value};
use std::collections::{HashMap, VecDeque};

pub const PROTOCOL_VERSION: &str = "1.2";

/// `PilotDeckError` fields that cross the API.
#[derive(Debug, Clone, PartialEq)]
pub struct SdkError {
    pub code: String,
    pub message: String,
    pub details: Option<Value>,
    pub retryable: bool,
    pub request_id: Option<String>,
}

impl SdkError {
    pub fn new(code: &str, message: impl Into<String>, retryable: bool) -> Self {
        SdkError { code: code.into(), message: message.into(), details: None, retryable, request_id: None }
    }
}

/// `normalizeGatewayCode(code)`.
pub fn normalize_gateway_code(code: Option<&str>) -> String {
    let Some(n) = code.map(|c| c.to_lowercase()).filter(|c| !c.is_empty()) else { return "gateway_request_failed".into() };
    match n.as_str() {
        "capability_unavailable" => "unsupported_capability".into(),
        "session_not_found" | "project_not_found" => "not_found".into(),
        "session_busy" => "conflict".into(),
        _ if n.starts_with("invalid_") => "validation_error".into(),
        _ => n,
    }
}

/// `mapError(error)` for a non-`PilotDeckError` error object `{ code?, message?, details? }`.
pub fn map_error(code: Option<&str>, message: &str, details: Option<Value>) -> SdkError {
    let code = normalize_gateway_code(Some(code.unwrap_or("transport_error")));
    let retryable = code == "timeout" || code == "transport_error";
    SdkError { code, message: message.into(), details, retryable, request_id: None }
}

/// Backoff delay for attempt `attempt` (1-based) of `openWithRetry`, before jitter; `factor` is `1 + (r*2-1)*jitter`.
pub fn retry_delay_ms(attempt: u32, initial_delay_ms: f64, max_delay_ms: f64, factor: f64) -> f64 {
    let initial = initial_delay_ms.max(0.0);
    let max = max_delay_ms.max(initial);
    let base = max.min(initial * 2f64.powi(attempt as i32 - 1));
    (base * factor).round().max(0.0)
}

pub fn is_retryable_connect_error(e: &SdkError) -> bool {
    e.code == "transport_error" || e.code == "timeout"
}

pub fn js_number_to_string(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f == 0.0 {
        return "0".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if f < 0.0 {
        return format!("-{}", js_number_to_string(-f));
    }
    let e = format!("{f:e}");
    let (mant, exp) = e.split_once('e').unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp.parse::<i32>().unwrap() + 1;
    if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let rest = if k > 1 { format!(".{}", &digits[1..]) } else { String::new() };
        format!("{}{rest}e{}{}", &digits[..1], if n >= 1 { "+" } else { "-" }, (n - 1).abs())
    }
}

/// `String(v)` (what `new Error(v)` does to a non-string message).
pub fn js_string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => js_number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(|x| if x.is_null() { String::new() } else { js_string(x) }).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        _ => true,
    }
}

fn array_index(k: &str) -> Option<u64> {
    if k == "0" {
        return Some(0);
    }
    if k.is_empty() || k.starts_with('0') || !k.bytes().all(|b| b.is_ascii_digit()) || k.len() > 10 {
        return None;
    }
    k.parse::<u64>().ok().filter(|n| *n < 4_294_967_295)
}

/// V8 own-key order for a plain object: index keys ascending, then insertion order (recursively).
pub fn js_order(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut idx: Vec<(u64, String, Value)> = vec![];
            let mut rest = vec![];
            for (k, v) in m {
                match array_index(&k) {
                    Some(n) => idx.push((n, k, js_order(v))),
                    None => rest.push((k, js_order(v))),
                }
            }
            idx.sort_by_key(|x| x.0);
            Value::Object(idx.into_iter().map(|x| (x.1, x.2)).chain(rest).collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(js_order).collect()),
        o => o,
    }
}

/// `{ ...event, sequence: seq }`.
fn spread_with_sequence(event: Option<&Value>, seq: &Value) -> Value {
    let mut m: Map<String, Value> = Map::new();
    match event {
        Some(Value::Object(o)) => {
            if let Value::Object(o) = js_order(Value::Object(o.clone())) {
                m = o;
            }
        }
        Some(Value::Array(a)) => {
            for (i, v) in a.iter().enumerate() {
                m.insert(i.to_string(), v.clone());
            }
        }
        Some(Value::String(s)) => {
            for (i, u) in s.encode_utf16().enumerate() {
                m.insert(i.to_string(), Value::String(String::from_utf16_lossy(&[u])));
            }
        }
        _ => {}
    }
    m.insert("sequence".into(), seq.clone());
    js_order(Value::Object(m))
}

/// What the IO layer must deliver.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Hello(Value),
    /// A request settled.
    /// `Ok(None)`: the response had no `result` (undefined in JS).
    Settle { id: String, result: Result<Option<Value>, SdkError> },
    StreamItem { id: String, value: Value },
    /// A stream ended (None: normally).
    StreamEnd { id: String, error: Option<SdkError> },
    Notification(Value),
    /// The socket should be dropped (`this.socket = undefined`).
    DropSocket,
}

/// Bookkeeping of `GatewayTransport` for one socket.
#[derive(Default)]
pub struct TransportCore {
    pub hello: Option<Value>,
    pending: Vec<String>,
    streams: Vec<String>,
    last_seq: HashMap<String, i64>,
    /// `this.socket` is set and open.
    pub connected: bool,
    pub closed: bool,
}

impl TransportCore {
    pub fn new() -> Self {
        Self::default()
    }

    /// The frame to send for a request (`send` throws when not connected).
    pub fn begin_request(&mut self, id: &str, method: &str, params: &Value) -> Result<String, SdkError> {
        if !self.connected {
            return Err(SdkError::new("transport_error", "Gateway WebSocket is not connected.", true));
        }
        self.pending.push(id.into());
        Ok(request_frame(id, method, params))
    }

    /// Request timeout: TS removes the pending entry and rejects.
    pub fn timeout_request(&mut self, id: &str, method: &str) -> Option<SdkError> {
        let pos = self.pending.iter().position(|p| p == id)?;
        self.pending.remove(pos);
        let mut e = SdkError::new("timeout", format!("Gateway request timed out: {method}"), true);
        e.request_id = Some(id.into());
        Some(e)
    }

    pub fn begin_stream(&mut self, id: &str, method: &str, params: &Value) -> Result<String, SdkError> {
        if !self.connected {
            return Err(SdkError::new("transport_error", "Gateway WebSocket is not connected.", true));
        }
        self.streams.push(id.into());
        self.last_seq.insert(id.into(), -1);
        Ok(request_frame(id, method, params))
    }

    fn remove_stream(&mut self, id: &str) {
        self.streams.retain(|s| s != id);
        self.last_seq.remove(id);
    }

    fn fail_pending(&mut self, error: SdkError, out: &mut Vec<Effect>) {
        for id in std::mem::take(&mut self.pending) {
            out.push(Effect::Settle { id, result: Err(error.clone()) });
        }
        for id in std::mem::take(&mut self.streams) {
            out.push(Effect::StreamEnd { id, error: Some(error.clone()) });
        }
        self.last_seq.clear();
        self.connected = false;
        out.push(Effect::DropSocket);
    }

    /// `handleMessage(raw)`.
    pub fn handle_message(&mut self, raw: &str) -> Vec<Effect> {
        let mut out = vec![];
        let frame: Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(_) => {
                self.fail_pending(SdkError::new("validation_error", "Invalid Gateway JSON frame.", false), &mut out);
                return out;
            }
        };
        let Value::Object(f) = &frame else { return out };
        let ty = f.get("type").and_then(Value::as_str);
        let id = f.get("id").and_then(Value::as_str).unwrap_or("");
        match ty {
            Some("hello_ok") => {
                self.hello = Some(frame.clone());
                out.push(Effect::Hello(frame.clone()));
            }
            Some("response") => {
                let in_pending = f.get("id").is_some_and(Value::is_string) && self.pending.iter().any(|p| p == id);
                let err = || {
                    let e = f.get("error").cloned().unwrap_or(Value::Null);
                    move |fallback: &str| SdkError {
                        code: normalize_gateway_code(e.get("code").and_then(Value::as_str)),
                        message: match e.get("message") {
                            None | Some(Value::Null) => fallback.into(),
                            Some(m) => js_string(m),
                        },
                        details: e.get("details").cloned(),
                        retryable: false,
                        request_id: None,
                    }
                };
                if !in_pending {
                    if !(f.get("id").is_some_and(Value::is_string) && self.streams.iter().any(|s| s == id)) {
                        return out;
                    }
                    self.remove_stream(id);
                    let error = if truthy(f.get("ok")) { None } else { Some(err()("Gateway stream failed.")) };
                    out.push(Effect::StreamEnd { id: id.into(), error });
                    return out;
                }
                self.pending.retain(|p| p != id);
                let result = if truthy(f.get("ok")) { Ok(f.get("result").cloned()) } else { Err(err()("Gateway request failed.")) };
                out.push(Effect::Settle { id: id.into(), result });
            }
            Some("event") => {
                if !(f.get("id").is_some_and(Value::is_string) && self.streams.iter().any(|s| s == id)) {
                    return out;
                }
                let seq_v = f.get("seq").cloned().unwrap_or(Value::Null);
                let seq = seq_v.as_f64().filter(|s| s.fract() == 0.0 && *s >= 0.0 && s.is_finite());
                let Some(seq) = seq else {
                    self.remove_stream(id);
                    let msg = format!("Gateway event sequence must be a non-negative integer for {id}.");
                    out.push(Effect::StreamEnd { id: id.into(), error: Some(SdkError::new("validation_error", msg, false)) });
                    return out;
                };
                let previous = *self.last_seq.get(id).unwrap_or(&-1) as f64;
                if seq <= previous {
                    return out;
                }
                if seq != previous + 1.0 {
                    self.remove_stream(id);
                    let msg = format!("Gateway event sequence gap for {id}: expected {}, received {}.", js_number_to_string(previous + 1.0), js_number_to_string(seq));
                    out.push(Effect::StreamEnd { id: id.into(), error: Some(SdkError::new("validation_error", msg, false)) });
                    return out;
                }
                self.last_seq.insert(id.into(), seq as i64);
                out.push(Effect::StreamItem { id: id.into(), value: spread_with_sequence(f.get("event"), &seq_v) });
                if truthy(f.get("final")) {
                    self.remove_stream(id);
                    out.push(Effect::StreamEnd { id: id.into(), error: None });
                }
            }
            Some("notification") => {
                let mut n = Map::new();
                if let Some(name) = f.get("name") {
                    n.insert("name".into(), name.clone());
                }
                if let Some(p) = f.get("payload") {
                    n.insert("payload".into(), p.clone());
                }
                out.push(Effect::Notification(Value::Object(n)));
            }
            _ => {}
        }
        out
    }

    /// The socket's `close` event (only while it is the current socket).
    pub fn handle_close(&mut self, code: u16, reason: &str) -> Vec<Effect> {
        let mut out = vec![];
        if !self.connected {
            return out;
        }
        self.fail_pending(close_error(code, reason), &mut out);
        out
    }

    /// `close()` by the client.
    pub fn close(&mut self) -> Vec<Effect> {
        let mut out = vec![];
        if self.closed {
            return out;
        }
        self.closed = true;
        self.fail_pending(SdkError::new("transport_error", "Gateway client closed.", false), &mut out);
        out
    }

    /// `serverInfo(hello)`.
    pub fn server_info(hello: &Value) -> Value {
        let info = hello.get("serverInfo").cloned().unwrap_or(Value::Null);
        let mut m = Map::new();
        m.insert("mode".into(), info.get("mode").filter(|v| !v.is_null()).cloned().unwrap_or(Value::String("remote".into())));
        for (k, src) in [("protocolVersion", hello.get("protocolVersion")), ("serverVersion", hello.get("serverVersion")), ("projectKey", info.get("projectKey")), ("capabilities", info.get("capabilities"))] {
            if let Some(v) = src {
                m.insert(k.into(), v.clone());
            }
        }
        Value::Object(m)
    }
}

/// The error a socket `close` event rejects everything with.
pub fn close_error(code: u16, reason: &str) -> SdkError {
    let error_code = if code == 4003 || reason == "auth_failed" {
        "authentication_error"
    } else if reason == "protocol_mismatch" {
        "protocol_version_error"
    } else {
        "transport_error"
    };
    let message = if reason.is_empty() { "Gateway WebSocket closed." } else { reason };
    SdkError::new(error_code, message, error_code == "transport_error")
}

pub fn request_frame(id: &str, method: &str, params: &Value) -> String {
    let mut m = Map::new();
    m.insert("type".into(), "request".into());
    m.insert("id".into(), id.into());
    m.insert("method".into(), method.into());
    m.insert("params".into(), params.clone());
    Value::Object(m).to_string()
}

pub fn hello_frame(protocol_version: Option<&str>, client_name: Option<&str>, client_version: Option<&str>, token: &str) -> String {
    serde_json::json!({"type": "hello", "protocolVersion": protocol_version.unwrap_or(PROTOCOL_VERSION), "clientName": client_name.unwrap_or("sdk"), "clientVersion": client_version.unwrap_or("0.1.0"), "token": token}).to_string()
}

/// `AsyncEventQueue<T>`: buffered values are delivered before a failure.
pub struct EventQueue<T> {
    values: VecDeque<T>,
    closed: bool,
    error: Option<SdkError>,
}

pub enum QueueNext<T> {
    Item(T),
    Done,
    Error(SdkError),
    /// Nothing yet (the async iterator would wait).
    Pending,
}

impl<T> Default for EventQueue<T> {
    fn default() -> Self {
        EventQueue { values: VecDeque::new(), closed: false, error: None }
    }
}

impl<T> EventQueue<T> {
    pub fn push(&mut self, v: T) {
        self.values.push_back(v);
    }
    pub fn close(&mut self) {
        self.closed = true;
    }
    pub fn fail(&mut self, e: SdkError) {
        self.error = Some(e);
        self.closed = true;
    }
    pub fn next(&mut self) -> QueueNext<T> {
        if let Some(v) = self.values.pop_front() {
            return QueueNext::Item(v);
        }
        if let Some(e) = &self.error {
            return QueueNext::Error(e.clone());
        }
        if self.closed {
            QueueNext::Done
        } else {
            QueueNext::Pending
        }
    }
}
