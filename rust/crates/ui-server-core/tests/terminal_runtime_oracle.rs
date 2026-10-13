//! Oracle test against `rust/parity/ui_server/gen_terminal_runtime.mjs` (terminalSessionRegistry.js,
//! runtimeCoordination.js). Fake PTYs, sockets, timers and IPC log the same lines as the JS fakes.
use pilotdeck_ui_server_core::terminal_runtime::*;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::rc::Rc;

type Log = Rc<RefCell<Vec<String>>>;

#[derive(Clone)]
struct Pty {
    id: String,
    kind: String,
    log: Log,
}
impl PartialEq for Pty {
    fn eq(&self, o: &Self) -> bool {
        self.id == o.id
    }
}
impl TerminalPty for Pty {
    fn kill(&self) {
        match self.kind.as_str() {
            "nokill" => {}
            "throwkill" => self.log.borrow_mut().push(format!("kill {} (throws)", self.id)),
            _ => self.log.borrow_mut().push(format!("kill {}", self.id)),
        }
    }
    fn write(&self, data: &str) -> bool {
        if self.kind == "nowrite" {
            return false;
        }
        self.log.borrow_mut().push(format!("write {} {}", self.id, json!(data)));
        true
    }
    fn resize(&self, c: f64, r: f64) -> bool {
        if self.kind == "noresize" {
            return false;
        }
        self.log.borrow_mut().push(format!("resize {} {c} {r}", self.id));
        true
    }
}

struct Timers {
    seq: u64,
    log: Log,
}
impl TimerHost for Timers {
    fn set_timeout(&mut self, ms: f64) -> u64 {
        self.seq += 1;
        self.log.borrow_mut().push(format!("setTimer {} {}", self.seq, pilotdeck_ui_server_core::js_number::js_number_to_string(ms)));
        self.seq
    }
    fn clear_timeout(&mut self, id: u64) {
        self.log.borrow_mut().push(format!("clearTimer {id}"));
    }
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Object(o) => Some(match o["$num"].as_str().unwrap() {
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            _ => f64::NAN,
        }),
        _ => panic!("{v}"),
    }
}

fn ws_ref(w: &WsSlot<String>) -> String {
    match w {
        WsSlot::Undefined => "undefined".into(),
        WsSlot::Null => "null".into(),
        WsSlot::Ws(id) => id.clone(),
    }
}

fn snap(s: Option<TerminalSnapshot<Pty, String>>) -> String {
    let Some(s) = s else { return "null".into() };
    let mut m = serde_json::Map::new();
    m.insert("key".into(), s.key.into());
    m.insert("pty".into(), s.pty.id.into());
    m.insert("ws".into(), ws_ref(&s.ws).into());
    m.insert("buffer".into(), json!(s.buffer));
    if let Some(p) = s.project_path {
        m.insert("projectPath".into(), p);
    }
    if let Some(p) = s.session_id {
        m.insert("sessionId".into(), p);
    }
    Value::Object(m).to_string()
}

fn terminal_log(c: &Value) -> Vec<String> {
    let log: Log = Default::default();
    let ptys: Vec<Pty> = c["ptys"].as_array().unwrap().iter().enumerate().map(|(i, k)| Pty { id: format!("p{}", i + 1), kind: k.as_str().unwrap().into(), log: log.clone() }).collect();
    let mut h = Timers { seq: 0, log: log.clone() };
    let cfg = &c["cfg"];
    let mut reg: TerminalSessionRegistry<Pty, String> = match TerminalSessionRegistry::new(cfg.get("timeoutMs").and_then(num), cfg.get("maxBufferEntries").and_then(num)) {
        Ok(r) => r,
        Err(TypeError(m)) => return vec![format!("ctor TypeError: {m}")],
    };
    let pty = |v: &Value| -> Option<Pty> {
        match v.as_i64().unwrap() {
            -1 | -2 => None,
            i => Some(ptys[i as usize].clone()),
        }
    };
    let ws = |v: &Value| -> WsSlot<String> {
        match v.as_i64().unwrap() {
            -1 => WsSlot::Null,
            -2 => WsSlot::Undefined,
            i => WsSlot::Ws(format!("w{}", i + 1)),
        }
    };
    let opt = |v: Option<&Value>| v.cloned();
    for op in c["ops"].as_array().unwrap() {
        log.borrow_mut().push(format!("> {op}"));
        let key = op["key"].as_str().unwrap_or("");
        let r: Option<String> = match op["op"].as_str().unwrap() {
            "register" => Some(match reg.register(&mut h, key, pty(&op["pty"]), ws(&op["ws"]), opt(op.get("projectPath")), opt(op.get("sessionId"))) {
                Ok(s) => snap(s),
                Err(TypeError(m)) => {
                    log.borrow_mut().push(format!("throw TypeError: {m}"));
                    continue;
                }
            }),
            "replace" => Some(reg.replace(&mut h, key).to_string()),
            "reconnect" => Some(snap(reg.reconnect(&mut h, key, pty(&op["pty"]).as_ref(), ws(&op["ws"])))),
            "append" => Some(snap(reg.append_output(key, pty(&op["pty"]).as_ref(), op["chunk"].as_str().unwrap()))),
            "complete" => Some(snap(reg.complete(&mut h, key, pty(&op["pty"]).as_ref()))),
            "write" => Some(reg.write(key, pty(&op["pty"]).as_ref(), &ws(&op["ws"]), op["data"].as_str().unwrap()).to_string()),
            "resize" => Some(reg.resize(key, pty(&op["pty"]).as_ref(), &ws(&op["ws"]), op["cols"].as_f64().unwrap(), op["rows"].as_f64().unwrap()).to_string()),
            "detach" => Some(snap(reg.detach(&mut h, key, pty(&op["pty"]).as_ref(), &ws(&op["ws"])))),
            "get" => Some(snap(reg.get(key))),
            "size" => Some(reg.size().to_string()),
            "dispose" => {
                reg.dispose(&mut h);
                None
            }
            "fire" => {
                reg.fire_timeout(&mut h, op["id"].as_u64().unwrap());
                None
            }
            o => panic!("{o}"),
        };
        if let Some(r) = r {
            log.borrow_mut().push(format!("= {r}"));
        }
    }
    let out = log.borrow().clone();
    out
}

struct Channel {
    cfg: Value,
    connected: Option<Value>,
    throws: bool,
    subscribed: bool,
    log: Vec<String>,
}
impl SupervisorChannel for Channel {
    fn can_send(&self) -> bool {
        self.cfg["send"] == true
    }
    fn disconnected(&self) -> bool {
        self.connected == Some(json!(false))
    }
    fn send(&mut self, m: &Value) -> Result<(), ()> {
        self.log.push(format!("send {m}"));
        if self.throws {
            Err(())
        } else {
            Ok(())
        }
    }
    fn subscribe(&mut self) {
        if self.cfg["on"] == true {
            self.log.push("on message".into());
            self.subscribed = true;
        }
    }
    fn unsubscribe(&mut self) {
        if self.cfg["off"] == true {
            self.log.push(format!("off message {}", self.subscribed));
        }
    }
}

fn runtime_log(c: &Value) -> Vec<String> {
    let cfg = &c["cfg"];
    let mut ch = Channel { cfg: cfg.clone(), connected: cfg.get("connected").cloned(), throws: false, subscribed: false, log: vec![] };
    let mut rc = RuntimeCoordination::new(&mut ch, cfg["env"].get("PILOTDECK_RUNTIME_SUPERVISED").and_then(Value::as_str));
    let mut n = 0;
    for op in c["ops"].as_array().unwrap() {
        ch.log.push(format!("> {op}"));
        let r: Option<Value> = match op["op"].as_str().unwrap() {
            "message" => {
                if ch.subscribed {
                    rc.on_message(&op["message"]);
                }
                None
            }
            "gateway" => Some(rc.gateway_state()),
            "configuration" => Some(rc.configuration_state()),
            "publish" => {
                n += 1;
                Some(rc.publish_configuration_state(&mut ch, json!({ "state": "ready", "n": n })))
            }
            "retry" => Some(Value::Bool(rc.request_gateway_retry(&mut ch))),
            "dispose" => {
                rc.dispose(&mut ch);
                None
            }
            "connected" => {
                ch.connected = Some(op["value"].clone());
                None
            }
            "throw" => {
                ch.throws = op["value"] == true;
                None
            }
            o => panic!("{o}"),
        };
        if let Some(r) = r {
            ch.log.push(format!("= {r}"));
        }
    }
    ch.log
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/terminal_runtime.json")).unwrap()
}

fn lines(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|l| l.as_str().unwrap().to_string()).collect()
}

#[test]
fn terminal_session_registry_matches_ts() {
    let fx = fixture();
    let cases = fx["terminal"].as_array().unwrap();
    assert_eq!(cases.len(), 709);
    for (i, c) in cases.iter().enumerate() {
        assert_eq!(terminal_log(c), lines(&c["log"]), "terminal case {i}");
    }
}

#[test]
fn runtime_coordination_matches_ts() {
    let fx = fixture();
    let cases = fx["runtime"].as_array().unwrap();
    assert_eq!(cases.len(), 300);
    for (i, c) in cases.iter().enumerate() {
        assert_eq!(runtime_log(c), lines(&c["log"]), "runtime case {i}");
    }
}
