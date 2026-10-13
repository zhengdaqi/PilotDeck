//! Port of `src/agent/modules/transport/agentLoopSidecarServer.ts` (control plane) and
//! `validateModuleMessage` from `src/agent/modules/protocol.ts`.
//!
//! Status: the JSON-lines control plane (hello, capabilities, cancel, status, resume, ack,
//! module_call rejection, parse/validation errors, execute admission incl. expired deadlines)
//! is native. AgentLoop execution itself is not migrated yet: `execute` requests that pass
//! admission are reported through `Server::on_execute`, which the binary answers by refusing
//! to run (see main.rs). Production keeps the TS sidecar until CAP-AGENT-LOOP is complete.

use pilotdeck_skills::jsjson::{stringify_compact, JsObject, JsValue};

pub const MODULE_PROTOCOL_VERSION: &str = "2.0";

fn s(v: &str) -> JsValue {
    JsValue::Str(v.to_string())
}

fn get<'a>(o: &'a JsObject, k: &str) -> Option<&'a JsValue> {
    o.get(k)
}

fn is_id(v: Option<&JsValue>) -> bool {
    matches!(v, Some(JsValue::Str(x)) if !x.is_empty())
}
fn is_record(v: Option<&JsValue>) -> bool {
    matches!(v, Some(JsValue::Obj(_)))
}
fn is_int_ge(v: Option<&JsValue>, min: f64) -> bool {
    matches!(v, Some(JsValue::Num(n)) if n.fract() == 0.0 && n.is_finite() && *n >= min)
}
fn is_outcome(v: Option<&JsValue>) -> bool {
    matches!(v, Some(JsValue::Str(x)) if ["completed", "failed", "cancelled", "result_unknown"].contains(&x.as_str()))
}

const MODULE_CALL_TARGETS: &[&str] = &[
    "model", "budget", "turn", "capability", "permission", "checkpoint", "context", "skills", "knowledge", "lifecycle", "event",
];

/// `validateModuleMessage`: Ok(()) or Err((code, message)).
pub fn validate_module_message(value: &JsValue) -> Result<(), (&'static str, String)> {
    let inv = |c: &'static str, m: &str| Err((c, m.to_string()));
    let JsValue::Obj(m) = value else { return inv("INVALID_MESSAGE", "Module message must be an object.") };
    let kind = match get(m, "kind") {
        Some(JsValue::Str(k)) if ["request", "response", "event", "error"].contains(&k.as_str()) => k.as_str(),
        _ => return inv("INVALID_KIND", "Module message kind is invalid."),
    };
    if kind != "event" && !is_id(get(m, "messageId")) {
        return inv("MISSING_MESSAGE_ID", "Message messageId is required.");
    }
    if kind == "event" && get(m, "messageId").is_some() && !is_id(get(m, "messageId")) {
        return inv("INVALID_EVENT_FIELD", "Event messageId is invalid.");
    }
    let method = match get(m, "method") {
        Some(JsValue::Str(x)) => x.as_str(),
        _ => "",
    };
    let has = |k: &str| get(m, k).is_some();
    let present_not_id = |k: &str| has(k) && !is_id(get(m, k));
    match kind {
        "request" => match method {
            "execute" => {
                for f in ["runId", "operationId", "requestId"] {
                    if !is_id(get(m, f)) {
                        return Err(("MISSING_EXECUTE_FIELD", format!("Execute request field {f} is required.")));
                    }
                }
                if !is_record(get(m, "payload")) {
                    return inv("MISSING_EXECUTE_FIELD", "Execute request field payload is required.");
                }
                if present_not_id("idempotencyKey") {
                    return inv("INVALID_EXECUTE_FIELD", "Execute request idempotencyKey is invalid.");
                }
                for (f, msg) in [("operationDeadline", "Execute request operationDeadline is invalid."), ("attemptDeadline", "Execute request attemptDeadline is invalid.")] {
                    if has(f) && !matches!(get(m, f), Some(JsValue::Str(_))) {
                        return inv("INVALID_EXECUTE_FIELD", msg);
                    }
                }
                if m.keys().iter().any(|k| k == "attemptId" || k == "stepId") {
                    return inv("UNSUPPORTED_FIELD", "attemptId and stepId are not public fields.");
                }
                Ok(())
            }
            "module_call" => {
                for f in ["runId", "operationId", "requestId"] {
                    if !is_id(get(m, f)) {
                        return Err(("MISSING_MODULE_CALL_FIELD", format!("Module call field {f} is required.")));
                    }
                }
                if !is_record(get(m, "payload")) {
                    return inv("MISSING_MODULE_CALL_FIELD", "Module call field payload is required.");
                }
                if present_not_id("idempotencyKey") {
                    return inv("INVALID_MODULE_CALL_FIELD", "Module call idempotencyKey is invalid.");
                }
                match get(m, "module") {
                    Some(JsValue::Str(t)) if MODULE_CALL_TARGETS.contains(&t.as_str()) => Ok(()),
                    _ => inv("INVALID_MODULE", "Module call target is invalid."),
                }
            }
            "cancel" => {
                let reason_ok = matches!(get(m, "reason"), Some(JsValue::Str(r)) if !r.is_empty());
                if !is_id(get(m, "runId")) || !is_id(get(m, "operationId")) || !reason_ok {
                    return inv("INVALID_CANCEL_REQUEST", "Cancel requires runId, operationId, and reason.");
                }
                if present_not_id("requestId") {
                    return inv("INVALID_CANCEL_REQUEST", "Cancel requestId is invalid.");
                }
                Ok(())
            }
            "status" => if is_id(get(m, "requestId")) { Ok(()) } else { inv("INVALID_STATUS_REQUEST", "Status requires requestId.") },
            "resume" => {
                let binding_ok = match get(m, "previousBinding") {
                    Some(JsValue::Obj(b)) => is_id(b.get("moduleInstanceId")) && is_id(b.get("connectionGeneration")),
                    _ => false,
                };
                if !is_id(get(m, "streamId")) || !binding_ok || !is_int_ge(get(m, "lastAppliedSequence"), -1.0) {
                    return inv("INVALID_RESUME_REQUEST", "Resume requires streamId, previousBinding, and lastAppliedSequence.");
                }
                Ok(())
            }
            "ack" => {
                if !is_id(get(m, "streamId")) || !is_int_ge(get(m, "lastAppliedSequence"), 0.0) {
                    return inv("INVALID_ACK_REQUEST", "Ack requires streamId and lastAppliedSequence.");
                }
                Ok(())
            }
            "hello" | "capabilities" => if is_record(get(m, "payload")) { Ok(()) } else { inv("INVALID_HANDSHAKE_REQUEST", "Handshake requires payload.") },
            _ => inv("INVALID_METHOD", "Module request method is invalid."),
        },
        "event" => {
            for f in ["eventType", "streamId", "runId", "operationId", "requestId"] {
                if !is_id(get(m, f)) {
                    return Err(("MISSING_EVENT_FIELD", format!("Event field {f} is required.")));
                }
            }
            if !is_int_ge(get(m, "sequence"), 0.0) || !matches!(get(m, "final"), Some(JsValue::Bool(_))) || !is_record(get(m, "payload")) {
                return inv("MISSING_EVENT_FIELD", "Event sequence, final, and payload are required.");
            }
            if matches!(get(m, "final"), Some(JsValue::Bool(true))) && !is_outcome(get(m, "outcome")) {
                return inv("MISSING_OUTCOME", "Final event requires outcome.");
            }
            if has("outcome") && !is_outcome(get(m, "outcome")) {
                return inv("INVALID_OUTCOME", "Event outcome is invalid.");
            }
            if present_not_id("code") {
                return inv("INVALID_EVENT_FIELD", "Event code is invalid.");
            }
            if m.keys().iter().any(|k| k == "moduleInstanceId" || k == "connectionGeneration") {
                return inv("UNSUPPORTED_FIELD", "Connection binding belongs to transport context.");
            }
            Ok(())
        }
        "response" => {
            if !is_id(get(m, "inReplyTo")) || !matches!(get(m, "ok"), Some(JsValue::Bool(_))) {
                return inv("INVALID_RESPONSE", "Response requires inReplyTo and ok.");
            }
            if matches!(get(m, "final"), Some(JsValue::Bool(true))) && !is_outcome(get(m, "outcome")) {
                return inv("MISSING_OUTCOME", "Final response requires outcome.");
            }
            if has("outcome") && !is_outcome(get(m, "outcome")) {
                return inv("INVALID_OUTCOME", "Response outcome is invalid.");
            }
            Ok(())
        }
        _ => {
            let msg_ok = matches!(get(m, "message"), Some(JsValue::Str(x)) if !x.is_empty());
            if !is_id(get(m, "code")) || !msg_ok {
                return inv("INVALID_ERROR", "Error requires code and message.");
            }
            match get(m, "retryability") {
                Some(JsValue::Str(r)) if ["safe", "unsafe", "retry_after_status"].contains(&r.as_str()) => Ok(()),
                _ => inv("INVALID_ERROR", "Error retryability is invalid."),
            }
        }
    }
}

/// `Math.random().toString(36).slice(2)`-shaped identifier (random base36 digits).
pub fn random_id() -> String {
    let mut seed = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut seed);
    } else {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        seed[..16].copy_from_slice(&t.to_le_bytes());
    }
    let mut n = u128::from_le_bytes(seed);
    let len = 10 + (n % 2) as usize;
    n /= 2;
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    (0..len).map(|_| { let d = digits[(n % 36) as usize] as char; n /= 36; d }).collect()
}

pub fn capabilities() -> JsValue {
    let m = |name: &str, extra: JsObject| {
        let mut o = JsObject::new().with("name", s(name));
        for k in extra.keys() {
            o.set(k.clone(), extra.get(&k).cloned().unwrap());
        }
        JsValue::Obj(o)
    };
    JsValue::Obj(JsObject::new().with("capabilitiesVersion", s("1")).with(
        "methods",
        JsValue::Arr(vec![
            m("execute", JsObject::new()
                .with("enabled", JsValue::Bool(true))
                .with("profiles", JsValue::Arr(vec![s("streaming")]))
                .with("cancel", JsValue::Bool(true))
                .with("resumeSupport", s("none"))
                .with("retry", s("retry_after_status"))),
            m("cancel", JsObject::new().with("enabled", JsValue::Bool(true))),
            m("status", JsObject::new().with("enabled", JsValue::Bool(true))),
            m("resume", JsObject::new().with("enabled", JsValue::Bool(false))),
            m("ack", JsObject::new().with("enabled", JsValue::Bool(false))),
        ]),
    ))
}

/// Execute admission outcome handed to the execution layer.
pub enum Admission {
    /// Deadline already expired: DEADLINE_EXCEEDED response was written.
    Expired,
    /// Accepted; caller must run the loop and publish events on `stream_id`.
    Accepted { stream_id: String },
}

/// One stdio connection of the sidecar server (control plane state).
pub struct Server {
    pub module_id: String,
    pub module_instance_id: String,
    pub connection_generation: String,
    binding_generation: String,
    pub out: Vec<String>,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    pub fn new() -> Self {
        let module_id = "pilotdeck-agent-loop".to_string();
        let module_instance_id = format!("{module_id}-instance-{}", random_id());
        let connection_generation = format!("{module_instance_id}-{}", random_id());
        let binding_generation = format!("{connection_generation}-{}", random_id());
        Server { module_id, module_instance_id, connection_generation, binding_generation, out: Vec::new() }
    }

    fn next_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", random_id())
    }

    fn send(&mut self, v: JsObject) {
        self.out.push(stringify_compact(&JsValue::Obj(v)));
    }

    fn handshake(&self, in_reply_to: &str, method: &str) -> JsObject {
        JsObject::new()
            .with("kind", s("response"))
            .with("messageId", s(&self.next_id(method)))
            .with("inReplyTo", s(in_reply_to))
            .with("ok", JsValue::Bool(true))
            .with("protocolVersion", s(MODULE_PROTOCOL_VERSION))
            .with("moduleId", s(&self.module_id))
            .with("moduleInstanceId", s(&self.module_instance_id))
            .with("connectionGeneration", s(&self.binding_generation))
            .with("capabilitiesVersion", s("1"))
            .with("payload", JsValue::Obj(JsObject::new()))
    }

    fn control_failure(&self, in_reply_to: &str, code: &str, prefix: &str) -> JsObject {
        JsObject::new()
            .with("kind", s("response"))
            .with("messageId", s(&self.next_id(prefix)))
            .with("inReplyTo", s(in_reply_to))
            .with("ok", JsValue::Bool(false))
            .with("code", s(code))
    }

    /// Handle one input line. Returns `Some(request)` for an admitted execute.
    pub fn handle_line(&mut self, line: &str, now_ms: f64) -> Option<(JsObject, Admission)> {
        if line.trim().is_empty() {
            return None;
        }
        let value = match pilotdeck_skills::v8json::parse(line) {
            Ok(v) => v,
            Err(_) => {
                let e = JsObject::new()
                    .with("kind", s("error"))
                    .with("messageId", s(&self.next_id("parse")))
                    .with("code", s("INVALID_JSON"))
                    .with("message", s("Module message is not valid JSON."))
                    .with("retryability", s("unsafe"));
                self.send(e);
                return None;
            }
        };
        if let Err((code, message)) = validate_module_message(&value) {
            let e = JsObject::new()
                .with("kind", s("error"))
                .with("messageId", s(&self.next_id("validation")))
                .with("code", s(code))
                .with("message", JsValue::Str(message))
                .with("retryability", s("unsafe"));
            self.send(e);
            return None;
        }
        let JsValue::Obj(m) = value else { return None };
        let text = |k: &str| match m.get(k) {
            Some(JsValue::Str(x)) => x.clone(),
            _ => String::new(),
        };
        if text("kind") != "request" {
            return None;
        }
        let message_id = text("messageId");
        match text("method").as_str() {
            "hello" => {
                let h = self.handshake(&message_id, "hello");
                self.send(h);
            }
            "capabilities" => {
                let mut h = self.handshake(&message_id, "capabilities");
                h.set("payload", capabilities());
                self.send(h);
            }
            "execute" => {
                let deadline = [text("operationDeadline"), text("attemptDeadline")]
                    .iter()
                    .filter_map(|d| parse_js_date(d))
                    .fold(None, |acc: Option<f64>, d| Some(acc.map_or(d, |a| a.min(d))));
                if let Some(d) = deadline {
                    if d <= now_ms {
                        let mut r = JsObject::new()
                            .with("kind", s("response"))
                            .with("messageId", s(&self.next_id("deadline")))
                            .with("inReplyTo", s(&message_id));
                        if let Some(v) = m.get("requestId") {
                            r.set("requestId", v.clone());
                        }
                        let r = r
                            .with("ok", JsValue::Bool(false))
                            .with("final", JsValue::Bool(true))
                            .with("outcome", s("failed"))
                            .with("code", s("DEADLINE_EXCEEDED"));
                        self.send(r);
                        return Some((m, Admission::Expired));
                    }
                }
                let stream_id = self.next_id("stream");
                let r = JsObject::new()
                    .with("kind", s("response"))
                    .with("messageId", s(&self.next_id("accepted")))
                    .with("inReplyTo", s(&message_id))
                    .with("requestId", m.get("requestId").cloned().unwrap_or(JsValue::Null))
                    .with("ok", JsValue::Bool(true))
                    .with("streamId", s(&stream_id))
                    .with("cursor", JsValue::Num(0.0));
                self.send(r);
                return Some((m, Admission::Accepted { stream_id }));
            }
            "module_call" => {
                let mut r = JsObject::new()
                    .with("kind", s("response"))
                    .with("messageId", s(&self.next_id("module-call")))
                    .with("inReplyTo", s(&message_id));
                if let Some(v) = m.get("requestId") {
                    r.set("requestId", v.clone());
                }
                let r = r
                    .with("ok", JsValue::Bool(false))
                    .with("final", JsValue::Bool(true))
                    .with("outcome", s("failed"))
                    .with("code", s("UNEXPECTED_MODULE_CALL"));
                self.send(r);
            }
            "cancel" => {
                // No active executions are tracked by the control plane yet.
                let mut r = JsObject::new()
                    .with("kind", s("response"))
                    .with("messageId", s(&self.next_id("cancel")))
                    .with("inReplyTo", s(&message_id));
                if let Some(v) = m.get("requestId") {
                    r.set("requestId", v.clone());
                }
                let r = r.with("ok", JsValue::Bool(false)).with("code", s("UNKNOWN_OPERATION"));
                self.send(r);
            }
            "status" => {
                let mut r = JsObject::new()
                    .with("kind", s("response"))
                    .with("messageId", s(&self.next_id("status")))
                    .with("inReplyTo", s(&message_id));
                if let Some(v) = m.get("requestId") {
                    r.set("requestId", v.clone());
                }
                let r = r.with("ok", JsValue::Bool(false)).with("code", s("UNKNOWN_REQUEST"));
                self.send(r);
            }
            "resume" => {
                let r = self.control_failure(&message_id, "RESUME_UNSUPPORTED", "resume");
                self.send(r);
            }
            "ack" => {
                let r = self.control_failure(&message_id, "ACK_UNSUPPORTED", "ack");
                self.send(r);
            }
            _ => {}
        }
        None
    }
}

/// `Date.parse` for the ISO-8601 forms the gateway emits; None for NaN.
pub fn parse_js_date(text: &str) -> Option<f64> {
    if text.is_empty() {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|d| d.timestamp_millis() as f64)
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .ok()
                .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis() as f64)
        })
}

/// Milliseconds since the Unix epoch (`Date.now()`).
pub fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

// AgentLoop execute-path port (work in progress; not wired into the binary yet).
pub mod json;
pub mod rt;
pub mod timeline;
pub mod agent;
pub mod factory;
pub mod exec;
pub mod util;
pub mod status;
pub mod looprun;
pub mod cacheplan;
pub mod lfr;
pub mod tokenizer;
pub mod truncate;
pub mod plantodo;
