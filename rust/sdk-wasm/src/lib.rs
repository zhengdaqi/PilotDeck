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
