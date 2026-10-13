#![allow(clippy::cmp_owned)] // compare against the serialized TS log text, not a JSON value.
//! Oracle test against `rust/parity/ui_server/gen_ui_services.mjs` (uploadLimits, gatewayConnectionCache,
//! regenerateLastMessage). Logs and values compare as serialized JSON strings.
use pilotdeck_ui_server_core::services::*;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/ui_services.json")).unwrap()
}

fn js_num(f: f64) -> String {
    pilotdeck_ui_server_core::js_number::js_number_to_string(f)
}

#[test]
fn upload_limits_match_ts() {
    let fx = fixture();
    let cases = fx["upload"].as_array().unwrap();
    for c in cases {
        let env: HashMap<String, String> = c["env"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().into())).collect();
        assert_eq!(read_upload_limits(&env).to_string(), c["out"].to_string(), "{env:?}");
    }
    assert_eq!(cases.len(), 300);
}

fn cache_log(cfg: &Value, ops: &[Value]) -> Vec<String> {
    let should = Rc::new(Cell::new(cfg["should"].as_bool().unwrap()));
    let s2 = should.clone();
    let mut c: GatewayConnectionCache<String> = GatewayConnectionCache::new(cfg["base"].as_f64().unwrap(), cfg["max"].as_f64().unwrap(), Box::new(move || s2.get()));
    let mut log = vec![];
    let mut timer = false;
    let mut subscribed: HashSet<String> = HashSet::new();
    let mut connect_done: Vec<bool> = vec![];
    let mut attempts: Vec<u64> = vec![]; // get-promise identity order
    let mut connect_attempt: Vec<u64> = vec![];
    let apply = |fx: Vec<CacheEffect<String>>, log: &mut Vec<String>, timer: &mut bool, subscribed: &mut HashSet<String>, connect_done: &mut Vec<bool>, connect_attempt: &mut Vec<u64>| {
        for e in fx {
            match e {
                CacheEffect::Connect(a) => {
                    connect_done.push(false);
                    connect_attempt.push(a);
                    log.push(format!("connect {}", connect_done.len()));
                }
                CacheEffect::Connected(g) => {
                    log.push(format!("connected {g}"));
                    subscribed.insert(g);
                }
                CacheEffect::Disconnected(g) => log.push(format!("disconnected {g}")),
                CacheEffect::Invalidated(g) => log.push(format!("invalidated {g}")),
                CacheEffect::SetTimer(d) => {
                    log.push(format!("setTimer {}", js_num(d)));
                    *timer = true;
                }
                CacheEffect::ClearTimer => {
                    log.push("clearTimer".into());
                    *timer = false;
                }
            }
        }
    };
    for op in ops {
        log.push(format!("> {op}"));
        match op["op"].as_str().unwrap() {
            "get" => {
                let (a, fx) = c.get();
                // TS calls connect() in a microtask, after `get` returned.
                let k = attempts.iter().position(|x| *x == a).unwrap_or_else(|| {
                    attempts.push(a);
                    attempts.len() - 1
                });
                log.push(format!("get #{k}"));
                let n = log.len() - 1;
                let mut tail = vec![];
                apply(fx, &mut tail, &mut timer, &mut subscribed, &mut connect_done, &mut connect_attempt);
                // clearTimer is synchronous (before the "get" line); connect follows it.
                for t in tail {
                    if t == "clearTimer" {
                        log.insert(n, t);
                    } else {
                        log.push(t);
                    }
                }
            }
            "resolve" => {
                let n = match op["n"].as_u64().unwrap() as usize {
                    0 => connect_done.len(),
                    n => n,
                };
                if n >= 1 && n <= connect_done.len() && !connect_done[n - 1] {
                    connect_done[n - 1] = true;
                    let fx = c.resolve(connect_attempt[n - 1], op["gid"].as_str().unwrap().into());
                    apply(fx, &mut log, &mut timer, &mut subscribed, &mut connect_done, &mut connect_attempt);
                }
            }
            "reject" => {
                let n = match op["n"].as_u64().unwrap() as usize {
                    0 => connect_done.len(),
                    n => n,
                };
                if n >= 1 && n <= connect_done.len() && !connect_done[n - 1] {
                    connect_done[n - 1] = true;
                    let fx = c.reject(connect_attempt[n - 1]);
                    apply(fx, &mut log, &mut timer, &mut subscribed, &mut connect_done, &mut connect_attempt);
                }
            }
            "disconnect" => {
                let g = op["gid"].as_str().unwrap().to_string();
                if subscribed.contains(&g) {
                    let fx = c.disconnect(&g);
                    apply(fx, &mut log, &mut timer, &mut subscribed, &mut connect_done, &mut connect_attempt);
                }
            }
            "invalidate" => {
                let g = op["gid"].as_str().map(String::from);
                let (r, fx) = c.invalidate(g.as_ref());
                let mut tail = vec![];
                apply(fx, &mut tail, &mut timer, &mut subscribed, &mut connect_done, &mut connect_attempt);
                log.extend(tail);
                log.push(format!("invalidate {r}"));
            }
            "fire" => {
                if timer {
                    timer = false;
                    // The timer's own get() promise is not one the script observed.
                    let (_a, fx) = c.fire_timer();
                    apply(fx, &mut log, &mut timer, &mut subscribed, &mut connect_done, &mut connect_attempt);
                }
            }
            "should" => should.set(op["value"].as_bool().unwrap()),
            other => panic!("{other}"),
        }
        log.push(format!("current {}", c.current().map_or("null".to_string(), |g| g.clone())));
    }
    log
}

#[test]
fn gateway_connection_cache_matches_ts() {
    let fx = fixture();
    let cases = fx["cache"].as_array().unwrap();
    for (i, c) in cases.iter().enumerate() {
        let got = cache_log(&c["cfg"], c["ops"].as_array().unwrap());
        let want: Vec<String> = c["log"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
        // TS logs ops with JSON.stringify (gid null kept); Rust formats ops from the fixture value the same way.
        assert_eq!(got, want, "case {i}");
    }
    assert_eq!(cases.len(), 404);
}

fn thrown(t: &Value) -> Thrown {
    if t["kind"] == "err" {
        Thrown::Error(t["msg"].as_str().unwrap().into())
    } else {
        Thrown::Value(t["value"].clone())
    }
}

fn regen_log(s: &Value) -> Vec<Value> {
    let log = RefCell::new(vec![]);
    let mut w = |f: Value| log.borrow_mut().push(json!(["writer", f]));
    let mut sw = |f: Value| log.borrow_mut().push(json!(["stream", f]));
    let writer: RefCell<&mut dyn FnMut(Value)> = RefCell::new(&mut w);
    let stream_writer: RefCell<&mut dyn FnMut(Value)> = RefCell::new(&mut sw);
    let mut replace = |sid: &Value, tid: &Value, opts: &Value| {
        log.borrow_mut().push(json!(["replace", sid, tid, opts]));
        match s["replace"].get("throw") {
            Some(t) => Err(thrown(t)),
            None => Ok(s["replace"]["value"].clone()),
        }
    };
    let mut finalize = |sid: &Value, tx: &Value, action: &str, opts: &Value| {
        log.borrow_mut().push(json!(["finalize", sid, tx, action, opts]));
        if s["finalizeThrows"].as_bool().unwrap() {
            Err(Thrown::Error("fin".into()))
        } else {
            Ok(())
        }
    };
    let mut run = |command: &Value, options: &Value, send: &mut dyn FnMut(Value), provider: &Value, accept: &mut dyn FnMut()| {
        log.borrow_mut().push(json!(["run", command, options, provider]));
        for a in s["run"]["actions"].as_array().unwrap() {
            if let Some(n) = a.get("send") {
                send(json!({ "frame": n }));
            } else if a.get("accept").is_some() {
                accept();
            } else if let Some(t) = a.get("throw") {
                return Err(thrown(t));
            }
        }
        Ok(s["run"]["ret"].clone())
    };
    let data = &s["data"];
    let req = RegenerateRequest { data, session_id: &s["sessionId"], request_id: &s["requestId"], expected_turn_id: &s["expectedTurnId"], provider: &s["provider"] };
    let result = regenerate_last_message_transaction(&req, RegenerateHost { replace_last_turn: &mut replace, finalize_last_turn_replacement: &mut finalize, run_chat: &mut run, writer: &writer, stream_writer: &stream_writer });
    let mut out = log.into_inner();
    out.push(json!(["result", result]));
    out
}

#[test]
fn regenerate_last_message_matches_ts() {
    let fx = fixture();
    let cases = fx["regen"].as_array().unwrap();
    let mut bad = vec![];
    for (i, c) in cases.iter().enumerate() {
        let got = Value::Array(regen_log(&c["script"])).to_string();
        if got != c["log"].to_string() {
            bad.push(format!("case {i}\n script {}\n ts   {}\n rust {got}", c["script"], c["log"]));
        }
    }
    assert!(bad.is_empty(), "{} mismatches\n{}", bad.len(), bad.iter().take(5).cloned().collect::<Vec<_>>().join("\n"));
    assert_eq!(cases.len(), 400);
}
