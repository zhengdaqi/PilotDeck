//! Oracle test against `rust/parity/sdk/gen_transport.mjs` (TS GatewayTransport driven through a mock WebSocket).
use pilotdeck_sdk::transport_core::{hello_frame, Effect, SdkError, TransportCore};
use serde_json::{json, Map, Value};

fn err_json(e: &SdkError) -> Value {
    let mut m = Map::new();
    m.insert("name".into(), "PilotDeckError".into());
    m.insert("code".into(), e.code.clone().into());
    m.insert("message".into(), e.message.clone().into());
    m.insert("retryable".into(), e.retryable.into());
    if let Some(d) = &e.details {
        m.insert("details".into(), d.clone());
    }
    if let Some(r) = &e.request_id {
        m.insert("requestId".into(), r.clone().into());
    }
    Value::Object(m)
}

fn idx(id: &str) -> i64 {
    id.strip_prefix("$id").and_then(|n| n.parse().ok()).unwrap()
}

fn effects_to_log(effects: Vec<Effect>, log: &mut Vec<Value>) {
    for e in effects {
        match e {
            Effect::Settle { id, result: Ok(v) } => log.push(json!({"req": idx(&id), "ok": v.unwrap_or(json!("__undefined"))})),
            Effect::Settle { id, result: Err(e) } => log.push(json!({"req": idx(&id), "err": err_json(&e)})),
            Effect::StreamItem { id, value } => log.push(json!({"stream": idx(&id), "item": value})),
            Effect::StreamEnd { id, error: None } => log.push(json!({"stream": idx(&id), "end": true})),
            Effect::StreamEnd { id, error: Some(e) } => log.push(json!({"stream": idx(&id), "err": err_json(&e)})),
            Effect::Notification(n) => log.push(json!({ "notif": n })),
            Effect::Hello(_) | Effect::DropSocket => {}
        }
    }
}

fn canon(log: Vec<Value>) -> Vec<Value> {
    let rank = |e: &Value| if e.get("sent").is_some() { 0 } else if e.get("crash").is_some() { 1 } else if e.get("req").is_some() { 2 } else if e.get("stream").is_some() { 3 } else { 4 };
    let key = |e: &Value| e.get("req").or(e.get("stream")).and_then(Value::as_i64).unwrap_or(0);
    let mut v: Vec<(usize, Value)> = log.into_iter().enumerate().collect();
    v.sort_by(|a, b| rank(&a.1).cmp(&rank(&b.1)).then(key(&a.1).cmp(&key(&b.1))).then(a.0.cmp(&b.0)));
    v.into_iter().map(|x| x.1).collect()
}

fn run(script: &[Value]) -> Vec<Value> {
    let mut core = TransportCore::new();
    let mut logs = vec![];
    // connect: open, hello, hello_ok.
    core.connected = true;
    let hello: Value = serde_json::from_str(&hello_frame(None, None, None, "tok")).unwrap();
    let hello_ok = json!({"type": "hello_ok", "protocolVersion": "1.2", "serverVersion": "9", "serverInfo": {"mode": "local", "projectKey": "pk", "capabilities": ["a"]}});
    core.handle_message(&hello_ok.to_string());
    logs.push(json!({"op": "connect", "log": [{"sent": hello}, {"connected": TransportCore::server_info(&hello_ok)}]}));
    let mut n = 0;
    for op in script {
        let mut log = vec![];
        let kind = op["op"].as_str().unwrap();
        match kind {
            "request" | "stream" => {
                let id = format!("$id{n}");
                let method = op["method"].as_str().unwrap();
                let r = if kind == "request" { core.begin_request(&id, method, &op["params"]) } else { core.begin_stream(&id, method, &op["params"]) };
                match r {
                    Ok(frame) => log.push(json!({ "sent": serde_json::from_str::<Value>(&frame).unwrap() })),
                    Err(e) => log.push(if kind == "request" { json!({"req": n, "err": err_json(&e)}) } else { json!({"stream": n, "err": err_json(&e)}) }),
                }
                n += 1;
            }
            "frame" => {
                if core.connected {
                    effects_to_log(core.handle_message(op["raw"].as_str().unwrap()), &mut log);
                }
            }
            "serverClose" => effects_to_log(core.handle_close(op["code"].as_u64().unwrap() as u16, op["reason"].as_str().unwrap()), &mut log),
            "clientClose" => effects_to_log(core.close(), &mut log),
            _ => unreachable!(),
        }
        logs.push(json!({"op": kind, "log": canon(log)}));
    }
    logs
}

#[test]
fn transport_core_matches_ts() {
    let f: Value = serde_json::from_str(include_str!("fixtures/transport.json")).unwrap();
    let mut ops = 0;
    for (i, c) in f["cases"].as_array().unwrap().iter().enumerate() {
        let got = run(c["script"].as_array().unwrap());
        let want = c["logs"].as_array().unwrap();
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(want) {
            assert_eq!(g.to_string(), w.to_string(), "case {i} script {}", c["script"]);
            ops += 1;
        }
    }
    assert!(ops > 6000, "{ops}");
}

/// SRC-DEFECT-015: TS throws inside the message listener (an uncaught exception in the host) and leaves the
/// request pending forever. Rust ignores a `null` frame and settles the others with a normal gateway error.
#[test]
fn malformed_frames_do_not_crash_src_defect_015() {
    let f: Value = serde_json::from_str(include_str!("fixtures/transport.json")).unwrap();
    let want_rust = [
        json!([]),
        json!([{"req": 0, "err": {"name": "PilotDeckError", "code": "gateway_request_failed", "message": "Gateway request failed.", "retryable": false}}]),
        json!([{"req": 0, "err": {"name": "PilotDeckError", "code": "gateway_request_failed", "message": "Gateway request failed.", "retryable": false}}]),
        json!([{"req": 0, "err": {"name": "PilotDeckError", "code": "gateway_request_failed", "message": "Gateway request failed.", "retryable": false}}]),
    ];
    let crash = f["crashCases"].as_array().unwrap();
    assert_eq!(crash.len(), want_rust.len());
    for (c, want) in crash.iter().zip(want_rust) {
        assert_eq!(c["logs"].as_array().unwrap().last().unwrap()["log"], json!([{"crash": true}]), "TS no longer shows the defect");
        let got = run(c["script"].as_array().unwrap());
        assert_eq!(got.last().unwrap()["log"], want);
    }
}

#[test]
fn retry_delay_matches_formula() {
    use pilotdeck_sdk::transport_core::retry_delay_ms;
    assert_eq!(retry_delay_ms(1, 100.0, 2000.0, 1.0), 100.0);
    assert_eq!(retry_delay_ms(4, 100.0, 2000.0, 1.0), 800.0);
    assert_eq!(retry_delay_ms(9, 100.0, 2000.0, 1.0), 2000.0);
    assert_eq!(retry_delay_ms(2, 100.0, 50.0, 1.2), 120.0);
    assert_eq!(retry_delay_ms(1, -5.0, 0.0, 1.0), 0.0);
}
