//! Sidecar runtime: stdio JSON-lines transport, control plane, module calls and stream events.
//! Port of `agentLoopSidecarServer.ts` (single connection, stdio transport).
use crate::json::*;
use crate::obj;
use crate::{capabilities, now_ms, parse_js_date, random_id, validate_module_message, MODULE_PROTOCOL_VERSION};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

pub enum Incoming {
    Line(String),
    Eof,
}

pub struct Active {
    pub run_id: String,
    pub operation_id: String,
    pub request_id: String,
    /// Abort reason once the controller is aborted.
    pub reason: Option<JsValue>,
    pub deadline_exceeded: bool,
    pub deadline_at: Option<f64>,
}

#[derive(Debug, Clone)]
pub enum ErrKind {
    Generic,
    /// SidecarAbortError
    Aborted,
    /// code RESULT_UNKNOWN
    ResultUnknown,
    /// terminal capability failure
    Terminal,
    /// ModelProviderError carrying a canonical model error
    Model(JsValue),
}

#[derive(Debug, Clone)]
pub struct LoopErr {
    pub kind: ErrKind,
    pub code: Option<String>,
    pub message: String,
}

impl LoopErr {
    pub fn msg(message: impl Into<String>) -> Self {
        LoopErr { kind: ErrKind::Generic, code: None, message: message.into() }
    }
    pub fn code(code: &str, message: impl Into<String>) -> Self {
        LoopErr { kind: ErrKind::Generic, code: Some(code.to_string()), message: message.into() }
    }
    pub fn is_terminal_tool_error(&self) -> bool {
        matches!(self.kind, ErrKind::ResultUnknown | ErrKind::Terminal) || self.code.as_deref() == Some("RESULT_UNKNOWN")
    }
}

/// A module call before wire framing (`SidecarModuleCall`).
pub struct Call {
    pub run_id: String,
    pub operation_id: String,
    pub idempotency_key: Option<String>,
    pub request_id: String,
    pub module: &'static str,
    pub abortable: bool,
    pub record_failure: bool,
    pub payload: JsValue,
}

pub struct Rt {
    rx: Receiver<Incoming>,
    out: Box<dyn Write>,
    pub module_id: String,
    pub module_instance_id: String,
    pub binding_generation: String,
    responses: HashMap<String, JsValue>,
    outstanding: HashSet<String>,
    ignored: HashSet<String>,
    emit_queue: VecDeque<(Call, String)>,
    emit_inflight: Option<String>,
    pub active: Option<Active>,
    ops: Vec<(String, JsObject)>,
    request_ops: HashMap<String, String>,
    queued_executes: VecDeque<JsValue>,
    pub eof: bool,
    module_failures: HashMap<String, JsValue>,
}

pub fn next_id(prefix: &str) -> String {
    format!("{prefix}-{}", random_id())
}

impl Rt {
    pub fn new(rx: Receiver<Incoming>, out: Box<dyn Write>) -> Self {
        let module_id = "pilotdeck-agent-loop".to_string();
        let module_instance_id = format!("{module_id}-instance-{}", random_id());
        let connection_generation = format!("{module_instance_id}-{}", random_id());
        let binding_generation = format!("{connection_generation}-{}", random_id());
        Rt {
            rx,
            out,
            module_id,
            module_instance_id,
            binding_generation,
            responses: HashMap::new(),
            outstanding: HashSet::new(),
            ignored: HashSet::new(),
            emit_queue: VecDeque::new(),
            emit_inflight: None,
            active: None,
            ops: Vec::new(),
            request_ops: HashMap::new(),
            queued_executes: VecDeque::new(),
            eof: false,
            module_failures: HashMap::new(),
        }
    }

    pub fn send(&mut self, v: &JsValue) {
        let line = to_json(v);
        let _ = writeln!(self.out, "{line}");
        let _ = self.out.flush();
    }

    /// Serve until EOF. Returns the process exit code.
    pub fn serve(&mut self) -> i32 {
        loop {
            if let Some(exec) = self.queued_executes.pop_front() {
                crate::exec::handle_execute(self, exec);
                continue;
            }
            if self.eof {
                return 0;
            }
            self.pump();
        }
    }

    /// Process exactly one incoming message (blocking, honours the active deadline).
    pub fn pump(&mut self) {
        let incoming = match self.active.as_ref().and_then(|a| if a.reason.is_none() { a.deadline_at } else { None }) {
            Some(deadline) => {
                let wait = (deadline - now_ms()).max(0.0);
                match self.rx.recv_timeout(Duration::from_millis(wait as u64)) {
                    Ok(m) => m,
                    Err(RecvTimeoutError::Timeout) => {
                        if let Some(a) = self.active.as_mut() {
                            a.deadline_exceeded = true;
                            a.reason = Some(obj! {"code" => "DEADLINE_EXCEEDED", "message" => "Module execution deadline exceeded."});
                        }
                        return;
                    }
                    Err(RecvTimeoutError::Disconnected) => Incoming::Eof,
                }
            }
            None => self.rx.recv().unwrap_or(Incoming::Eof),
        };
        match incoming {
            Incoming::Eof => {
                self.eof = true;
            }
            Incoming::Line(line) => self.handle_line(&line),
        }
    }

    fn handle_line(&mut self, line: &str) {
        if line.trim().is_empty() {
            return;
        }
        let value = match pilotdeck_skills::v8json::parse(line) {
            Ok(v) => v,
            Err(_) => {
                let e = obj! {"kind" => "error", "messageId" => next_id("parse"), "code" => "INVALID_JSON",
                    "message" => "Module message is not valid JSON.", "retryability" => "unsafe"};
                self.send(&e);
                return;
            }
        };
        if let Err((code, message)) = validate_module_message(&value) {
            let e = obj! {"kind" => "error", "messageId" => next_id("validation"), "code" => code,
                "message" => message, "retryability" => "unsafe"};
            self.send(&e);
            return;
        }
        let kind = gs(&value, "kind").unwrap_or("").to_string();
        if kind == "response" {
            let reply = gs(&value, "inReplyTo").unwrap_or("").to_string();
            if self.outstanding.remove(&reply) {
                self.responses.insert(reply, value);
            } else if self.emit_inflight.as_deref() == Some(reply.as_str()) {
                self.emit_inflight = None;
                self.flush_emit_queue();
            } else {
                self.ignored.remove(&reply);
            }
            return;
        }
        if kind != "request" {
            return;
        }
        let message_id = gs(&value, "messageId").unwrap_or("").to_string();
        match gs(&value, "method").unwrap_or("") {
            "hello" => {
                let h = self.handshake(&message_id, "hello", obj! {});
                self.send(&h);
            }
            "capabilities" => {
                let h = self.handshake(&message_id, "capabilities", capabilities());
                self.send(&h);
            }
            "execute" => {
                let op = gs(&value, "operationId").unwrap_or("").to_string();
                if let Some(active) = &self.active {
                    if active.operation_id == op {
                        let code = if Some(active.run_id.as_str()) == gs(&value, "runId") {
                            "OPERATION_ALREADY_ACTIVE"
                        } else {
                            "OPERATION_IDENTITY_MISMATCH"
                        };
                        let r = obj! {"kind" => "response", "messageId" => next_id("execute-rejected"),
                            "inReplyTo" => message_id, "requestId" => get(&value, "requestId"),
                            "ok" => false, "code" => code};
                        self.send(&r);
                        return;
                    }
                }
                // Single-threaded port: a concurrent execute for another operation runs after
                // the active one settles (TS interleaves them).
                self.queued_executes.push_back(value);
            }
            "module_call" => {
                let r = obj! {"kind" => "response", "messageId" => next_id("module-call"), "inReplyTo" => message_id,
                    "requestId" => get(&value, "requestId"), "ok" => false, "final" => true,
                    "outcome" => "failed", "code" => "UNEXPECTED_MODULE_CALL"};
                self.send(&r);
            }
            "cancel" => {
                let op = gs(&value, "operationId").unwrap_or("").to_string();
                let run = gs(&value, "runId").unwrap_or("").to_string();
                let req = gs(&value, "requestId").map(|s| s.to_string());
                let mut matches = false;
                let mut known = false;
                if let Some(active) = self.active.as_mut() {
                    if active.operation_id == op {
                        known = true;
                        matches = active.run_id == run && req.as_ref().map_or(true, |r| *r == active.request_id);
                        if matches {
                            let reason = get(&value, "reason").clone();
                            if gs(&value, "reason") == Some(format!("timeout:{run}").as_str()) {
                                active.deadline_exceeded = true;
                            }
                            if active.reason.is_none() {
                                active.reason = Some(reason);
                            }
                        }
                    }
                }
                if matches {
                    self.op_update(&op, |s| {
                        if s.get("outcome").is_none() {
                            s.set("cancelRequested", JsValue::Bool(true));
                            s.set("state", s_("cancel_requested"));
                            s.set("updatedAt", JsValue::Str(iso_now()));
                        }
                    });
                }
                let mut r = obj! {"kind" => "response", "messageId" => next_id("cancel"), "inReplyTo" => message_id,
                    "requestId" => get(&value, "requestId"), "ok" => matches};
                if matches {
                    set(&mut r, "payload", obj! {"operationId" => op, "cancelled" => true});
                } else {
                    set(&mut r, "code", if known { "OPERATION_IDENTITY_MISMATCH" } else { "UNKNOWN_OPERATION" });
                }
                self.send(&r);
            }
            "status" => {
                let req = gs(&value, "requestId").unwrap_or("").to_string();
                let snapshot = self.request_ops.get(&req).and_then(|op| self.ops.iter().find(|(k, _)| k == op)).map(|(_, s)| s.clone());
                let mut r = obj! {"kind" => "response", "messageId" => next_id("status"), "inReplyTo" => message_id,
                    "requestId" => get(&value, "requestId"), "ok" => snapshot.is_some()};
                match snapshot {
                    Some(s) => set(&mut r, "payload", JsValue::Obj(s)),
                    None => set(&mut r, "code", "UNKNOWN_REQUEST"),
                }
                self.send(&r);
            }
            "resume" => {
                let r = obj! {"kind" => "response", "messageId" => next_id("resume"), "inReplyTo" => message_id, "ok" => false, "code" => "RESUME_UNSUPPORTED"};
                self.send(&r);
            }
            "ack" => {
                let r = obj! {"kind" => "response", "messageId" => next_id("ack"), "inReplyTo" => message_id, "ok" => false, "code" => "ACK_UNSUPPORTED"};
                self.send(&r);
            }
            _ => {}
        }
    }

    fn handshake(&self, in_reply_to: &str, method: &str, payload: JsValue) -> JsValue {
        obj! {"kind" => "response", "messageId" => next_id(method), "inReplyTo" => in_reply_to, "ok" => true,
            "protocolVersion" => MODULE_PROTOCOL_VERSION, "moduleId" => &self.module_id,
            "moduleInstanceId" => &self.module_instance_id, "connectionGeneration" => &self.binding_generation,
            "capabilitiesVersion" => "1", "payload" => payload}
    }

    // ---- operation ledger (ModuleOperationHost) ----
    pub fn op_accept(&mut self, run_id: &str, op: &str, request_id: &str) {
        if let Some((_, s)) = self.ops.iter_mut().find(|(k, _)| k == op) {
            if let Some(JsValue::Arr(ids)) = s.get_mut("requestIds") {
                if !ids.iter().any(|v| str_of(v) == Some(request_id)) {
                    ids.push(s_(request_id));
                }
            }
            if s.get("state").and_then(str_of) == Some("pending") {
                s.set("state", s_("running"));
            }
            s.set("updatedAt", JsValue::Str(iso_now()));
        } else {
            let snapshot = obj! {"runId" => run_id, "operationId" => op, "state" => "running",
                "requestIds" => vec![s_(request_id)], "cancelRequested" => false, "updatedAt" => iso_now()};
            if let JsValue::Obj(o) = snapshot {
                self.ops.push((op.to_string(), o));
            }
        }
        self.request_ops.insert(request_id.to_string(), op.to_string());
    }

    pub fn op_final(&mut self, op: &str, outcome: &str) {
        self.op_update(op, |s| {
            if s.get("outcome").is_some() {
                return;
            }
            s.set("outcome", s_(outcome));
            let state = match outcome {
                "completed" => "completed",
                "cancelled" => "cancelled",
                "result_unknown" => "resolving",
                _ => "failed",
            };
            s.set("state", s_(state));
            s.set("updatedAt", JsValue::Str(iso_now()));
        });
    }

    fn op_update(&mut self, op: &str, f: impl FnOnce(&mut JsObject)) {
        if let Some((_, s)) = self.ops.iter_mut().find(|(k, _)| k == op) {
            f(s);
        }
    }

    pub fn aborted(&self) -> bool {
        self.active.as_ref().map_or(false, |a| a.reason.is_some())
    }
    pub fn abort_reason(&self) -> Option<JsValue> {
        self.active.as_ref().and_then(|a| a.reason.clone())
    }
    pub fn abort(&mut self, reason: JsValue) {
        if let Some(a) = self.active.as_mut() {
            if a.reason.is_none() {
                a.reason = Some(reason);
            }
        }
    }

    pub fn module_failure(&self, op: &str) -> Option<JsValue> {
        self.module_failures.get(op).cloned()
    }
    pub fn clear_module_failure(&mut self, op: &str) {
        self.module_failures.remove(op);
    }

    // ---- module calls ----
    fn frame(call: &Call, message_id: &str) -> JsValue {
        obj! {"kind" => "request", "messageId" => message_id, "method" => "module_call",
            "runId" => &call.run_id, "operationId" => &call.operation_id,
            "idempotencyKey" => call.idempotency_key.clone(), "requestId" => &call.request_id,
            "module" => call.module, "abortSignal" => if call.abortable { obj! {} } else { JsValue::Undefined },
            "payload" => &call.payload}
    }

    /// Send a call and register it as awaited. Returns the message id.
    pub fn send_call(&mut self, call: &Call) -> String {
        let id = next_id("module-call");
        self.outstanding.insert(id.clone());
        let framed = Self::frame(call, &id);
        self.send(&framed);
        id
    }

    /// Fire-and-forget call (`void promise.catch()`): the response is ignored.
    pub fn send_detached(&mut self, call: &Call) {
        let id = next_id("module-call");
        self.ignored.insert(id.clone());
        let framed = Self::frame(call, &id);
        self.send(&framed);
    }

    /// Await a previously sent call. Abortable waits reject once the controller aborts.
    pub fn wait(&mut self, id: &str, call: &Call) -> Result<JsValue, LoopErr> {
        loop {
            if let Some(r) = self.responses.remove(id) {
                return self.post_process(call, r);
            }
            if self.aborted() {
                self.outstanding.remove(id);
                self.ignored.insert(id.to_string());
                return Err(LoopErr { kind: ErrKind::Aborted, code: None, message: "Sidecar module call aborted.".into() });
            }
            if self.eof {
                // Host closed the transport while a call is pending: the TS sidecar
                // never settles and Node exits on the unsettled top-level await.
                let _ = self.out.flush();
                std::process::exit(13);
            }
            self.pump();
        }
    }

    pub fn call(&mut self, call: Call) -> Result<JsValue, LoopErr> {
        let id = self.send_call(&call);
        self.wait(&id, &call)
    }

    fn post_process(&mut self, call: &Call, r: JsValue) -> Result<JsValue, LoopErr> {
        let ok = gb(&r, "ok") == Some(true);
        if !ok && gs(&r, "outcome") == Some("result_unknown") && gs(&r, "code") == Some("RESULT_UNKNOWN") {
            let message = gs(get(&r, "error"), "message").map(|s| s.to_string())
                .unwrap_or_else(|| display_or(get(get(&r, "error"), "message"), "Host capability execution outcome is unknown."));
            return Err(LoopErr { kind: ErrKind::ResultUnknown, code: Some("RESULT_UNKNOWN".into()), message });
        }
        if call.module == "capability" && !ok && gb(&r, "final") == Some(true) && gs(&r, "outcome") == Some("failed") {
            let message = display_or(get(get(&r, "error"), "message"), "Host module execution failed.");
            return Err(LoopErr { kind: ErrKind::Terminal, code: gs(&r, "code").map(|s| s.to_string()), message });
        }
        if call.record_failure && !call.operation_id.is_empty() {
            if !ok {
                let mut f = JsObject::new();
                if let Some(c) = gs(&r, "code") {
                    f.set("code", s_(c));
                }
                let m = match get(get(&r, "error"), "message") {
                    JsValue::Undefined | JsValue::Null => gs(&r, "code").unwrap_or("Module call failed.").to_string(),
                    v => js_string(v),
                };
                f.set("message", JsValue::Str(m));
                self.module_failures.insert(call.operation_id.clone(), JsValue::Obj(f));
            } else {
                self.module_failures.remove(&call.operation_id);
            }
        }
        Ok(r)
    }

    // ---- chained event bridge (hostAgentEventBridge) ----
    pub fn emit_agent_event(&mut self, call: Call) {
        let id = next_id("module-call");
        self.emit_queue.push_back((call, id));
        if self.emit_inflight.is_none() {
            self.flush_emit_queue();
        }
    }
    fn flush_emit_queue(&mut self) {
        if let Some((call, id)) = self.emit_queue.pop_front() {
            self.emit_inflight = Some(id.clone());
            let framed = Self::frame(&call, &id);
            self.send(&framed);
        }
    }
    /// `eventBridge.flush()`: wait until every chained emit settled.
    pub fn flush_events(&mut self) {
        while self.emit_inflight.is_some() || !self.emit_queue.is_empty() {
            if self.eof {
                let _ = self.out.flush();
                std::process::exit(13);
            }
            self.pump();
        }
    }
}

pub fn s_(v: &str) -> JsValue {
    JsValue::Str(v.to_string())
}

/// `String(value)` for the shapes protocol errors carry.
pub fn js_string(v: &JsValue) -> String {
    match v {
        JsValue::Str(s) => s.clone(),
        JsValue::Undefined => "undefined".into(),
        JsValue::Null => "null".into(),
        JsValue::Bool(b) => b.to_string(),
        JsValue::Num(n) => pilotdeck_skills::jsjson::js_number_to_string(*n),
        JsValue::Arr(a) => a.iter().map(|x| match x { JsValue::Undefined | JsValue::Null => String::new(), o => js_string(o) }).collect::<Vec<_>>().join(","),
        JsValue::Obj(_) => "[object Object]".into(),
    }
}
fn display_or(v: &JsValue, fallback: &str) -> String {
    match v {
        JsValue::Undefined | JsValue::Null => fallback.to_string(),
        other => js_string(other),
    }
}

pub fn iso_now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

pub fn earliest_deadline(values: &[Option<&str>]) -> Option<f64> {
    values.iter().filter_map(|v| v.and_then(parse_js_date)).fold(None, |acc: Option<f64>, d| Some(acc.map_or(d, |a| a.min(d))))
}
