//! Oracle test against `rust/parity/ui_server/gen_connection_test_tasks.mjs`: replays each op script on the
//! Rust port with the same mocked host and compares the event log line by line.
use pilotdeck_ui_server_core::connection_test_tasks::*;
use serde_json::{json, Map, Value};

struct Host {
    clock: f64,
    ids: u64,
    runs: u64,
    persists: u64,
    current: bool,
    rec_ok: bool,
    reason: String,
    log: Vec<String>,
}

fn undef(v: Option<&Value>) -> String {
    v.map(Value::to_string).unwrap_or_else(|| "undefined".into())
}

fn err_of(v: &Value) -> JsError {
    JsError { message: v["message"].as_str().unwrap().into(), status: v.get("status").cloned(), code: v.get("code").cloned() }
}

impl ConnectionTestHost for Host {
    fn now(&mut self) -> f64 {
        self.clock
    }
    fn new_id(&mut self) -> String {
        self.ids += 1;
        format!("T{}", self.ids)
    }
    fn prepare(&mut self, body: &Value, user_id: &str) -> Result<(), JsError> {
        self.log.push(format!("prepare {user_id} {body} true"));
        match body.get("prepareThrow") {
            Some(e) => Err(err_of(e)),
            None => Ok(()),
        }
    }
    fn start_run(&mut self) -> u64 {
        self.runs += 1;
        self.log.push(format!("run {}", self.runs));
        self.runs - 1
    }
    fn persist(&mut self, user_id: &str, test_id: Option<&Value>) -> u64 {
        self.persists += 1;
        self.log.push(format!("persist {} {user_id} {}", self.persists, undef(test_id)));
        self.persists - 1
    }
    fn get_record(&mut self, user_id: &str, test_id: Option<&Value>) -> (Option<Value>, String) {
        self.log.push(format!("getRecord {user_id} {}", undef(test_id)));
        let mut rec = Map::new();
        if let Some(t) = test_id {
            rec.insert("testId".into(), t.clone());
        }
        (self.rec_ok.then_some(Value::Object(rec)), self.reason.clone())
    }
    fn apply_image(&mut self, record: &Value, body: &Value) -> Value {
        let mut m = Map::new();
        m.insert("status".into(), "passed".into());
        if let Some(t) = record.get("testId") {
            m.insert("testId".into(), t.clone());
        }
        if let Some(i) = body.get("image") {
            m.insert("image".into(), i.clone());
        }
        Value::Object(m)
    }
    fn is_current(&mut self, _user_id: &str, _task: &Map<String, Value>) -> bool {
        self.current
    }
}

fn replay(ops: &[Value]) -> Vec<String> {
    let mut h = Host { clock: 1_700_000_000_000.0, ids: 0, runs: 0, persists: 0, current: true, rec_ok: true, reason: "gone".into(), log: vec![] };
    let mut c = ConnectionTestTasks::new();
    let mut started: Vec<String> = vec![];
    let mut done_runs = std::collections::HashSet::new();
    let mut done_persists = std::collections::HashSet::new();
    for op in ops {
        h.log.push(format!("> {op}"));
        let user = op["user"].as_str().unwrap_or("");
        let tid = op["t"].as_i64().map(|t| if t >= 0 && (t as usize) < started.len() { started[t as usize].clone() } else { "bogus".into() }).unwrap_or_default();
        let r: Option<Result<Value, JsError>> = match op["op"].as_str().unwrap() {
            "start" => {
                let r = c.start(&mut h, user, &op["body"], op.get("modelId"));
                if let Ok(v) = &r {
                    started.push(v["id"].as_str().unwrap().into());
                }
                Some(r)
            }
            "list" => Some(Ok(c.list(&mut h, user))),
            "retry" => Some(c.retry(&mut h, user, &tid)),
            "confirm" => Some(c.confirm(&mut h, user, &tid, &op["body"])),
            "acknowledge" => Some(c.acknowledge(&mut h, user, &tid)),
            "cancel" => Some(c.cancel(&mut h, user, &tid)),
            "runResolve" | "runReject" => {
                let k = op["k"].as_u64().unwrap();
                if k < h.runs && done_runs.insert(k) {
                    let outcome = if op["op"] == "runResolve" { Ok(op.get("result").cloned()) } else { Err(err_of(&op["error"])) };
                    c.run_settled(&mut h, k, outcome);
                }
                None
            }
            "persistResolve" | "persistReject" => {
                let k = op["k"].as_u64().unwrap();
                if k < h.persists && done_persists.insert(k) {
                    let outcome = if op["op"] == "persistResolve" { Ok(()) } else { Err(err_of(&op["error"])) };
                    c.persist_settled(&mut h, k, outcome);
                }
                None
            }
            "advance" => {
                h.clock += op["ms"].as_f64().unwrap();
                None
            }
            "tick" => {
                c.tick(&mut h);
                None
            }
            "current" => {
                h.current = op["value"].as_bool().unwrap();
                None
            }
            "record" => {
                h.rec_ok = op["ok"].as_bool().unwrap();
                h.reason = op["reason"].as_str().unwrap().into();
                None
            }
            o => panic!("{o}"),
        };
        match r {
            Some(Ok(v)) => h.log.push(format!("= {v}")),
            Some(Err(e)) => h.log.push(format!("throw {} {} {}", json!(e.message), undef(e.status.as_ref()), undef(e.code.as_ref()))),
            None => {}
        }
    }
    h.log
}

#[test]
fn connection_test_tasks_match_ts() {
    let fx: Value = serde_json::from_str(include_str!("fixtures/connection_test_tasks.json")).unwrap();
    let cases = fx["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 612);
    for (i, c) in cases.iter().enumerate() {
        let want: Vec<String> = c["log"].as_array().unwrap().iter().map(|l| l.as_str().unwrap().to_string()).collect();
        assert_eq!(replay(c["ops"].as_array().unwrap()), want, "case {i}");
    }
}
