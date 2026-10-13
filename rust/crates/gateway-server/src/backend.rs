//! The `Gateway` port behind the server edge, and the JSON-lines host-process bridge.
//!
//! Bridge protocol (one JSON object per line):
//! - host -> server, first line: `{"type":"ready","methods":["listSessions",...]}` (implemented Gateway methods)
//! - server -> host: `{"id":N,"method":"<Gateway method>","argc":0|1,"params":<value>}`; argc 0 = called with
//!   no argument, argc 1 without `params` = called with `undefined`
//! - host -> server: `{"id":N,"event":<GatewayEvent>}` (submitTurn only, zero or more), then
//!   `{"id":N,"ok":true,"result":<value>}` or `{"id":N,"ok":false,"error":<GwError wire>}`
//! The error wire carries the TS classification GatewayWsConnection branches on:
//! `kind` = `skill_validation` | `skill_manager` | `structured` (DialogGatewayError or Error with string code) | `plain`.
use crate::{get, JsValue};
use pilotdeck_skills::jsjson::stringify_compact;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub enum GwError {
    SkillValidation { code: JsValue, message: String, validation: JsValue },
    SkillManager { code: JsValue, message: String },
    /// `DialogGatewayError` or any `Error` with a string `code`.
    Structured { code: JsValue, message: String, details: JsValue, status: JsValue },
    /// Any other thrown value; `message` is `error.message` or `String(error)`.
    Plain { message: String },
}

impl GwError {
    pub fn capability_unavailable(message: String) -> Self {
        GwError::Structured { code: crate::s("CAPABILITY_UNAVAILABLE"), message, details: JsValue::Undefined, status: JsValue::Undefined }
    }
    pub fn from_wire(e: &JsValue) -> Self {
        let message = match get(e, "message") {
            JsValue::Str(m) => m.clone(),
            _ => String::new(),
        };
        match get(e, "kind") {
            JsValue::Str(k) if k == "skill_validation" => GwError::SkillValidation { code: get(e, "code").clone(), message, validation: get(e, "validation").clone() },
            JsValue::Str(k) if k == "skill_manager" => GwError::SkillManager { code: get(e, "code").clone(), message },
            JsValue::Str(k) if k == "structured" => GwError::Structured {
                code: get(e, "code").clone(), message, details: get(e, "details").clone(), status: get(e, "status").clone() },
            _ => GwError::Plain { message },
        }
    }
}

pub type GwResult = Result<JsValue, GwError>;

/// The TS `Gateway` interface as seen by the server edge.
pub trait GatewayBackend: Send + Sync {
    /// Whether the Gateway implements `method` (TS optional-method presence check).
    fn has(&self, method: &str) -> bool;
    /// Invoke `method`; `params == None` means "called with no argument".
    fn call(&self, method: &str, params: Option<JsValue>) -> GwResult;
    /// `submitTurn` async iterator: `on_event` is invoked per event, in order.
    fn submit_turn(&self, params: JsValue, on_event: &mut dyn FnMut(JsValue)) -> Result<(), GwError>;
}

enum Msg {
    Event(JsValue),
    Done(GwResult),
}

/// Gateway served by a host process over JSON lines (see module docs).
pub struct ProcessGateway {
    stdin: Mutex<ChildStdin>,
    methods: HashSet<String>,
    pending: Arc<Mutex<HashMap<u64, Sender<Msg>>>>,
    next: Mutex<u64>,
    _child: Mutex<Child>,
}

impl ProcessGateway {
    pub fn spawn(argv: &[String]) -> Result<Self, String> {
        let (prog, args) = argv.split_first().ok_or("missing gateway command")?;
        let mut child = Command::new(prog).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit())
            .spawn().map_err(|e| format!("failed to start gateway host: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let mut stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let mut first = String::new();
        stdout.read_line(&mut first).map_err(|e| e.to_string())?;
        let ready = pilotdeck_skills::v8json::parse(first.trim()).map_err(|_| "gateway host did not send a ready line".to_string())?;
        let methods = match get(&ready, "methods") {
            JsValue::Arr(a) => a.iter().filter_map(|m| if let JsValue::Str(s) = m { Some(s.clone()) } else { None }).collect(),
            _ => return Err("gateway host ready line has no methods".into()),
        };
        let pending: Arc<Mutex<HashMap<u64, Sender<Msg>>>> = Arc::new(Mutex::new(HashMap::new()));
        let p2 = pending.clone();
        std::thread::spawn(move || {
            for line in stdout.lines() {
                let Ok(line) = line else { break };
                let Ok(v) = pilotdeck_skills::v8json::parse(&line) else { continue };
                let id = match get(&v, "id") {
                    JsValue::Num(n) => *n as u64,
                    _ => continue,
                };
                let mut map = p2.lock().unwrap_or_else(|e| e.into_inner());
                let msg = if !matches!(get(&v, "event"), JsValue::Undefined) {
                    Msg::Event(get(&v, "event").clone())
                } else if get(&v, "ok") == &JsValue::Bool(true) {
                    Msg::Done(Ok(get(&v, "result").clone()))
                } else {
                    Msg::Done(Err(GwError::from_wire(get(&v, "error"))))
                };
                let done = matches!(msg, Msg::Done(_));
                if let Some(tx) = map.get(&id) {
                    let _ = tx.send(msg);
                }
                if done {
                    map.remove(&id);
                }
            }
            // Host exited: fail every pending call.
            let mut map = p2.lock().unwrap_or_else(|e| e.into_inner());
            for (_, tx) in map.drain() {
                let _ = tx.send(Msg::Done(Err(GwError::Plain { message: "Gateway host exited.".into() })));
            }
        });
        Ok(ProcessGateway { stdin: Mutex::new(stdin), methods, pending, next: Mutex::new(0), _child: Mutex::new(child) })
    }

    fn request(&self, method: &str, params: Option<JsValue>, on_event: &mut dyn FnMut(JsValue)) -> GwResult {
        let id = {
            let mut n = self.next.lock().unwrap_or_else(|e| e.into_inner());
            *n += 1;
            *n
        };
        let (tx, rx) = channel();
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(id, tx);
        let mut req = pilotdeck_skills::jsjson::JsObject::new();
        req.set("id", JsValue::Num(id as f64));
        req.set("method", crate::s(method));
        // argc distinguishes `gateway.m()` from `gateway.m(undefined)`; an undefined argument has no `params`.
        req.set("argc", JsValue::Num(if params.is_some() { 1.0 } else { 0.0 }));
        if let Some(p) = params.filter(|p| !matches!(p, JsValue::Undefined)) {
            req.set("params", p);
        }
        let line = stringify_compact(&JsValue::Obj(req));
        {
            let mut w = self.stdin.lock().unwrap_or_else(|e| e.into_inner());
            if writeln!(w, "{line}").and_then(|_| w.flush()).is_err() {
                self.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                return Err(GwError::Plain { message: "Gateway host exited.".into() });
            }
        }
        loop {
            match rx.recv() {
                Ok(Msg::Event(e)) => on_event(e),
                Ok(Msg::Done(r)) => return r,
                Err(_) => return Err(GwError::Plain { message: "Gateway host exited.".into() }),
            }
        }
    }
}

impl GatewayBackend for ProcessGateway {
    fn has(&self, method: &str) -> bool {
        self.methods.contains(method)
    }
    fn call(&self, method: &str, params: Option<JsValue>) -> GwResult {
        self.request(method, params, &mut |_| {})
    }
    fn submit_turn(&self, params: JsValue, on_event: &mut dyn FnMut(JsValue)) -> Result<(), GwError> {
        self.request("submitTurn", Some(params), on_event).map(|_| ())
    }
}
