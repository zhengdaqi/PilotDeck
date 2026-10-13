//! Small `ui/server/services` modules:
//! - `uploadLimits.js` (`read_upload_limits`);
//! - `gatewayConnectionCache.js` (`GatewayConnectionCache`, sans-IO: the host runs `connect` and timers and feeds
//!   their outcomes back);
//! - `regenerateLastMessage.js` (`regenerate_last_message_transaction`, synchronous host callbacks).
use crate::js_number::{js_trim, number};
use crate::js_object::truthy;
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::collections::HashMap;

// ---- uploadLimits.js -------------------------------------------------------------------------------------------

/// `DEFAULT_UPLOAD_LIMITS` (src/gateway/dialog/UploadStore.ts).
pub const DEFAULT_MAX_FILE_BYTES: f64 = 1_073_741_824.0;
pub const DEFAULT_MAX_TASK_BYTES: f64 = 2_147_483_648.0;
pub const DEFAULT_MAX_FILES: f64 = 500.0;
pub const DEFAULT_MAX_CONCURRENT_PER_PROJECT: f64 = 3.0;
pub const DEFAULT_RETENTION_MS: f64 = 86_400_000.0;

/// `readUploadLimits(env)` as the JSON object TS returns.
pub fn read_upload_limits(env: &HashMap<String, String>) -> Value {
    let n = |name: &str, fallback: f64| {
        let v = env.get(name).map_or(f64::NAN, |s| number(s));
        if v.fract() == 0.0 && v.abs() <= 9_007_199_254_740_991.0 && v > 0.0 {
            v
        } else {
            fallback
        }
    };
    // Every result is a positive safe integer.
    let num = |f: f64| Value::Number((f as i64).into());
    json!({
        "maxFileBytes": num(n("PILOTDECK_UPLOAD_MAX_FILE_BYTES", DEFAULT_MAX_FILE_BYTES)),
        "maxTaskBytes": num(n("PILOTDECK_UPLOAD_MAX_TASK_BYTES", DEFAULT_MAX_TASK_BYTES)),
        "maxFiles": num(500f64.min(n("PILOTDECK_UPLOAD_MAX_FILES", DEFAULT_MAX_FILES))),
        "maxConcurrentPerProject": num(n("PILOTDECK_UPLOAD_MAX_CONCURRENT", DEFAULT_MAX_CONCURRENT_PER_PROJECT)),
        "retentionMs": num(n("PILOTDECK_UPLOAD_RETENTION_MS", DEFAULT_RETENTION_MS)),
    })
}

// ---- gatewayConnectionCache.js ---------------------------------------------------------------------------------

/// What the host must do, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum CacheEffect<G> {
    /// Call `connect()` for this attempt and report `resolve`/`reject`.
    Connect(u64),
    Connected(G),
    Disconnected(G),
    Invalidated(G),
    /// `setTimeout(fire_timer, delay)`.
    SetTimer(f64),
    ClearTimer,
}

/// `createGatewayConnectionCache`. Attempts stand for the shared `get()` promise: `get` returns the attempt whose
/// outcome the caller awaits.
pub struct GatewayConnectionCache<G> {
    pending: Option<u64>,
    current: Option<G>,
    timer: bool,
    reconnect_attempt: i32,
    next_attempt: u64,
    base_delay_ms: f64,
    max_delay_ms: f64,
    should_reconnect: Box<dyn Fn() -> bool>,
}

impl<G: Clone + PartialEq> GatewayConnectionCache<G> {
    pub fn new(base_delay_ms: f64, max_delay_ms: f64, should_reconnect: Box<dyn Fn() -> bool>) -> Self {
        GatewayConnectionCache { pending: None, current: None, timer: false, reconnect_attempt: 0, next_attempt: 0, base_delay_ms, max_delay_ms, should_reconnect }
    }

    pub fn current(&self) -> Option<&G> {
        self.current.as_ref()
    }

    /// `get()`.
    pub fn get(&mut self) -> (u64, Vec<CacheEffect<G>>) {
        if let Some(p) = self.pending {
            return (p, vec![]);
        }
        let mut fx = vec![];
        if self.timer {
            self.timer = false;
            fx.push(CacheEffect::ClearTimer);
        }
        self.next_attempt += 1;
        let a = self.next_attempt;
        self.pending = Some(a);
        fx.push(CacheEffect::Connect(a));
        (a, fx)
    }

    /// `connect()` for `attempt` resolved with `gateway`.
    pub fn resolve(&mut self, attempt: u64, gateway: G) -> Vec<CacheEffect<G>> {
        if self.pending != Some(attempt) {
            return vec![];
        }
        self.current = Some(gateway.clone());
        self.reconnect_attempt = 0;
        vec![CacheEffect::Connected(gateway)]
    }

    /// `connect()` for `attempt` failed (sync throw or rejection).
    pub fn reject(&mut self, attempt: u64) -> Vec<CacheEffect<G>> {
        let mut fx = vec![];
        if self.pending == Some(attempt) {
            self.invalidate_into(None, &mut fx);
            self.schedule(&mut fx);
        }
        fx
    }

    /// The `onDisconnect` subscription of a gateway that became current.
    pub fn disconnect(&mut self, gateway: &G) -> Vec<CacheEffect<G>> {
        let mut fx = vec![];
        if !self.invalidate_into(Some(gateway), &mut fx) {
            return fx;
        }
        fx.push(CacheEffect::Disconnected(gateway.clone()));
        self.schedule(&mut fx);
        fx
    }

    /// `invalidate(expected)`.
    pub fn invalidate(&mut self, expected: Option<&G>) -> (bool, Vec<CacheEffect<G>>) {
        let mut fx = vec![];
        let r = self.invalidate_into(expected, &mut fx);
        (r, fx)
    }

    fn invalidate_into(&mut self, expected: Option<&G>, fx: &mut Vec<CacheEffect<G>>) -> bool {
        if let Some(e) = expected {
            if self.current.as_ref() != Some(e) {
                return false;
            }
        }
        let old = self.current.take();
        self.pending = None;
        if let Some(g) = old {
            fx.push(CacheEffect::Invalidated(g));
        }
        true
    }

    fn schedule(&mut self, fx: &mut Vec<CacheEffect<G>>) {
        if self.pending.is_some() || self.current.is_some() || self.timer || !(self.should_reconnect)() {
            return;
        }
        let delay = self.max_delay_ms.min(self.base_delay_ms * 2f64.powi(self.reconnect_attempt));
        self.reconnect_attempt += 1;
        self.timer = true;
        fx.push(CacheEffect::SetTimer(delay));
    }

    /// The reconnect timer fired: `get()` (its failure is swallowed).
    pub fn fire_timer(&mut self) -> (u64, Vec<CacheEffect<G>>) {
        self.timer = false;
        self.get()
    }
}

// ---- regenerateLastMessage.js ----------------------------------------------------------------------------------

/// A thrown value: an `Error` (message) or anything else (`String(value)`).
#[derive(Debug, Clone)]
pub enum Thrown {
    Error(String),
    Value(Value),
}

impl Thrown {
    fn message(&self) -> String {
        match self {
            Thrown::Error(m) => m.clone(),
            Thrown::Value(v) => crate::js_object_string(v),
        }
    }
}

pub struct RegenerateRequest<'a> {
    pub data: &'a Value,
    pub session_id: &'a Value,
    pub request_id: &'a Value,
    pub expected_turn_id: &'a Value,
    pub provider: &'a Value,
}

/// `runChat(command, options, streamWriter, provider, { onInputAccepted })`.
pub type RunChat<'a> = dyn FnMut(&Value, &Value, &mut dyn FnMut(Value), &Value, &mut dyn FnMut()) -> Result<Value, Thrown> + 'a;

pub struct RegenerateHost<'a> {
    pub replace_last_turn: &'a mut dyn FnMut(&Value, &Value, &Value) -> Result<Value, Thrown>,
    pub finalize_last_turn_replacement: &'a mut dyn FnMut(&Value, &Value, &str, &Value) -> Result<(), Thrown>,
    pub run_chat: &'a mut RunChat<'a>,
    pub writer: &'a RefCell<&'a mut dyn FnMut(Value)>,
    pub stream_writer: &'a RefCell<&'a mut dyn FnMut(Value)>,
}

fn get<'v>(v: &'v Value, k: &str) -> Option<&'v Value> {
    v.as_object().and_then(|o| o.get(k))
}

fn put(m: &mut Map<String, Value>, k: &str, v: Option<&Value>) {
    // JSON.stringify drops undefined members; frames are compared serialized.
    if let Some(v) = v {
        m.insert(k.into(), v.clone());
    }
}

/// `regenerateLastMessageTransaction(...)`: returns `{ success, inputAccepted, rolledBack? }`.
pub fn regenerate_last_message_transaction(req: &RegenerateRequest, host: RegenerateHost) -> Value {
    let RegenerateHost { replace_last_turn, finalize_last_turn_replacement, run_chat, writer, stream_writer } = host;
    let data = req.data;
    let options = get(data, "options");
    let options_or_empty = options.filter(|o| truthy(Some(o))).cloned().unwrap_or_else(|| json!({}));
    struct St {
        accepted: bool,
        release: bool,
        buffered: Vec<Value>,
    }
    let st = RefCell::new(St { accepted: false, release: false, buffered: vec![] });
    let mut replacement: Option<Value> = None;
    let mut rolled_back = false;

    let result: Result<Value, Thrown> = (|| {
        if !truthy(Some(req.session_id)) || !truthy(Some(req.expected_turn_id)) {
            return Err(Thrown::Error("The last message could not be identified for editing.".into()));
        }
        let run_id = match options.and_then(|o| get(o, "runId")) {
            Some(Value::String(s)) => js_trim(s).to_string(),
            _ => String::new(),
        };
        if run_id.is_empty() {
            return Err(Thrown::Error("The edited message could not be assigned a replacement turn.".into()));
        }
        replacement = Some(replace_last_turn(req.session_id, req.expected_turn_id, &options_or_empty)?);
        let rep = replacement.clone().unwrap();
        let acknowledge = || {
            if st.borrow().accepted {
                return;
            }
            st.borrow_mut().accepted = true;
            let mut f = Map::new();
            f.insert("type".into(), json!("session-turn-replaced"));
            put(&mut f, "requestId", Some(req.request_id));
            put(&mut f, "sessionId", Some(req.session_id));
            put(&mut f, "replacedTurnId", get(&rep, "replacedTurnId"));
            f.insert("replacementRunId".into(), json!(run_id));
            put(&mut f, "provider", Some(req.provider));
            let nullish = |v: Option<&Value>| v.filter(|v| !v.is_null()).cloned();
            let content = nullish(options.and_then(|o| get(o, "userVisibleInput"))).or_else(|| nullish(get(data, "command"))).unwrap_or(json!(""));
            f.insert("content".into(), content);
            let or = |v: Option<&Value>| v.filter(|v| truthy(Some(v))).cloned();
            f.insert("images".into(), or(options.and_then(|o| get(o, "images"))).unwrap_or(json!([])));
            let att = or(options.and_then(|o| get(o, "displayAttachments"))).or_else(|| or(options.and_then(|o| get(o, "attachments")))).unwrap_or(json!([]));
            f.insert("attachments".into(), att);
            (stream_writer.borrow_mut())(Value::Object(f));
            let mut w = Map::new();
            w.insert("type".into(), json!("regenerate-last-message-result"));
            put(&mut w, "requestId", Some(req.request_id));
            put(&mut w, "sessionId", Some(req.session_id));
            w.insert("success".into(), json!(true));
            (writer.borrow_mut())(Value::Object(w));
            st.borrow_mut().release = true;
            let frames = std::mem::take(&mut st.borrow_mut().buffered);
            for fr in frames {
                (stream_writer.borrow_mut())(fr);
            }
        };
        let acknowledge = RefCell::new(acknowledge);
        let mut send = |frame: Value| {
            if st.borrow().release {
                (stream_writer.borrow_mut())(frame);
            } else {
                st.borrow_mut().buffered.push(frame);
            }
        };
        let mut on_accepted = || (acknowledge.borrow_mut())();
        let command = get(data, "command").cloned().unwrap_or(Value::Null);
        let run = run_chat(&command, options.unwrap_or(&Value::Null), &mut send, req.provider, &mut on_accepted)?;
        if truthy(get(&run, "inputAccepted")) {
            (acknowledge.borrow_mut())();
            return Ok(json!({"success": true, "inputAccepted": true}));
        }
        finalize_last_turn_replacement(req.session_id, get(&rep, "transactionId").unwrap_or(&Value::Null), "rollback", &options_or_empty)?;
        rolled_back = true;
        Err(Thrown::Error("The edited message was not persisted, so the original turn was restored.".into()))
    })();
    match result {
        Ok(v) => v,
        Err(e) => {
            if let Some(rep) = &replacement {
                if !st.borrow().accepted && !rolled_back && finalize_last_turn_replacement(req.session_id, get(rep, "transactionId").unwrap_or(&Value::Null), "rollback", &options_or_empty).is_ok() {
                    rolled_back = true;
                }
            }
            let mut w = Map::new();
            w.insert("type".into(), json!("regenerate-last-message-result"));
            put(&mut w, "requestId", Some(req.request_id));
            put(&mut w, "sessionId", Some(req.session_id));
            w.insert("success".into(), json!(false));
            w.insert("error".into(), json!(e.message()));
            (writer.borrow_mut())(Value::Object(w));
            json!({"success": false, "inputAccepted": false, "rolledBack": rolled_back})
        }
    }
}
