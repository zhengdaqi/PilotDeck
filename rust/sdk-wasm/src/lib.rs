//! wasm-bindgen surface of pilotdeck-sdk. JSON strings in/out; the JS wrapper (npm/index.js)
//! restores the TS SDK's async API shape and supplies `Date.now()`.
use pilotdeck_sdk::session_store::{self as ss, InMemorySessionStore, SessionKey, SessionSummary};
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

fn parse(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or(Value::Null)
}
fn arr(s: &str) -> Vec<Value> {
    parse(s).as_array().cloned().unwrap_or_default()
}

#[wasm_bindgen]
pub struct WasmSessionStore(InMemorySessionStore);

#[wasm_bindgen]
impl WasmSessionStore {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmSessionStore {
        WasmSessionStore(InMemorySessionStore::new())
    }
    pub fn load(&self, key: &str) -> String {
        self.0.load(&SessionKey::from_json(&parse(key))).map(Value::from).unwrap_or(Value::Null).to_string()
    }
    pub fn append(&mut self, key: &str, entries: &str, now: f64) {
        self.0.append(&SessionKey::from_json(&parse(key)), &arr(entries), now as i64);
    }
    #[wasm_bindgen(js_name = listSessions)]
    pub fn list_sessions(&self, project_key: &str) -> String {
        Value::Array(self.0.list_sessions(project_key).into_iter().map(|(id, m)| json!({"sessionId": id, "mtime": m})).collect()).to_string()
    }
    #[wasm_bindgen(js_name = listSessionSummaries)]
    pub fn list_session_summaries(&self, project_key: &str) -> String {
        Value::Array(self.0.list_session_summaries(project_key).iter().map(|s| s.to_json()).collect()).to_string()
    }
    #[wasm_bindgen(js_name = listSubkeys)]
    pub fn list_subkeys(&self, project_key: &str, session_id: &str) -> String {
        json!(self.0.list_subkeys(project_key, session_id)).to_string()
    }
    pub fn delete(&mut self, key: &str) {
        self.0.delete(&SessionKey::from_json(&parse(key)));
    }
}

impl Default for WasmSessionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen(js_name = foldSessionSummary)]
pub fn fold_session_summary(previous: &str, key: &str, entries: &str, mtime: Option<f64>) -> String {
    let prev = Some(parse(previous)).filter(|p| !p.is_null()).map(|p| SessionSummary::from_json(&p));
    ss::fold_session_summary(prev.as_ref(), &SessionKey::from_json(&parse(key)), &arr(entries), mtime.map(|m| m as i64)).to_json().to_string()
}

#[wasm_bindgen(js_name = summaryToSessionInfo)]
pub fn summary_to_session_info(summary: &str) -> String {
    Value::Object(ss::summary_to_session_info(&SessionSummary::from_json(&parse(summary)))).to_string()
}

#[wasm_bindgen(js_name = importBatchSize)]
pub fn import_batch_size(batch_size: Option<f64>) -> f64 {
    ss::import_batch_size(batch_size) as f64
}

// ---- GatewayTransport (packages/sdk/src/transport.ts): sans-IO core; the browser WebSocket lives in npm/transport.js.
use pilotdeck_sdk::transport_core::{self as tc, Effect, SdkError, TransportCore};

fn err_json(e: &SdkError) -> Value {
    let mut m = json!({"code": e.code, "message": e.message, "retryable": e.retryable});
    if let Some(d) = &e.details {
        m["details"] = d.clone();
    }
    if let Some(r) = &e.request_id {
        m["requestId"] = json!(r);
    }
    m
}

fn effects_json(effects: Vec<Effect>) -> String {
    Value::Array(
        effects
            .into_iter()
            .map(|e| match e {
                Effect::Hello(h) => json!({"t": "hello", "value": h}),
                Effect::Settle { id, result: Ok(v) } => match v {
                    Some(v) => json!({"t": "settle", "id": id, "ok": true, "value": v}),
                    None => json!({"t": "settle", "id": id, "ok": true}),
                },
                Effect::Settle { id, result: Err(e) } => json!({"t": "settle", "id": id, "ok": false, "error": err_json(&e)}),
                Effect::StreamItem { id, value } => json!({"t": "item", "id": id, "value": value}),
                Effect::StreamEnd { id, error } => match error {
                    Some(e) => json!({"t": "end", "id": id, "error": err_json(&e)}),
                    None => json!({"t": "end", "id": id}),
                },
                Effect::Notification(n) => json!({"t": "note", "value": n}),
                Effect::DropSocket => json!({"t": "drop"}),
            })
            .collect(),
    )
    .to_string()
}

/// Frame text, or `{"error": ...}` when `send` would throw.
fn frame_or_error(r: Result<String, SdkError>) -> String {
    match r {
        Ok(f) => json!({ "frame": f }).to_string(),
        Err(e) => json!({ "error": err_json(&e) }).to_string(),
    }
}

#[wasm_bindgen]
#[derive(Default)]
pub struct WasmTransportCore(TransportCore);

#[wasm_bindgen]
impl WasmTransportCore {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmTransportCore {
        WasmTransportCore(TransportCore::new())
    }
    #[wasm_bindgen(js_name = setConnected)]
    pub fn set_connected(&mut self, v: bool) {
        self.0.connected = v;
    }
    pub fn connected(&self) -> bool {
        self.0.connected
    }
    pub fn closed(&self) -> bool {
        self.0.closed
    }
    /// `hello_ok` frame JSON or "" when none.
    pub fn hello(&self) -> String {
        self.0.hello.as_ref().map_or(String::new(), Value::to_string)
    }
    #[wasm_bindgen(js_name = clearHello)]
    pub fn clear_hello(&mut self) {
        self.0.hello = None;
    }
    #[wasm_bindgen(js_name = beginRequest)]
    pub fn begin_request(&mut self, id: &str, method: &str, params: &str) -> String {
        frame_or_error(self.0.begin_request(id, method, &parse(params)))
    }
    #[wasm_bindgen(js_name = beginStream)]
    pub fn begin_stream(&mut self, id: &str, method: &str, params: &str) -> String {
        frame_or_error(self.0.begin_stream(id, method, &parse(params)))
    }
    /// Error JSON or "" when the request already settled.
    #[wasm_bindgen(js_name = timeoutRequest)]
    pub fn timeout_request(&mut self, id: &str, method: &str) -> String {
        self.0.timeout_request(id, method).map_or(String::new(), |e| err_json(&e).to_string())
    }
    #[wasm_bindgen(js_name = handleMessage)]
    pub fn handle_message(&mut self, raw: &str) -> String {
        effects_json(self.0.handle_message(raw))
    }
    #[wasm_bindgen(js_name = handleClose)]
    pub fn handle_close(&mut self, code: u16, reason: &str) -> String {
        effects_json(self.0.handle_close(code, reason))
    }
    pub fn close(&mut self) -> String {
        effects_json(self.0.close())
    }
}

#[wasm_bindgen(js_name = serverInfo)]
pub fn server_info(hello: &str) -> String {
    TransportCore::server_info(&parse(hello)).to_string()
}

#[wasm_bindgen(js_name = helloFrame)]
pub fn hello_frame(protocol_version: Option<String>, client_name: Option<String>, client_version: Option<String>, token: &str) -> String {
    tc::hello_frame(protocol_version.as_deref(), client_name.as_deref(), client_version.as_deref(), token)
}

#[wasm_bindgen(js_name = closeError)]
pub fn close_error(code: u16, reason: &str) -> String {
    err_json(&tc::close_error(code, reason)).to_string()
}

#[wasm_bindgen(js_name = retryDelayMs)]
pub fn retry_delay_ms(attempt: u32, initial: f64, max: f64, factor: f64) -> f64 {
    tc::retry_delay_ms(attempt, initial, max, factor)
}

/// `mapError` for a non-PilotDeckError thrown value `{ code?, message, details? }`.
#[wasm_bindgen(js_name = mapError)]
pub fn map_error(code: Option<String>, message: &str, details: &str) -> String {
    let d = Some(parse(details)).filter(|d| !d.is_null());
    err_json(&tc::map_error(code.as_deref(), message, d)).to_string()
}
