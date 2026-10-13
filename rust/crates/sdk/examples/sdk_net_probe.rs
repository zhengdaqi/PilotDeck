//! Parity probe for `pilotdeck_sdk::net` (driven by `rust/parity/sdk/net_transport_parity.mjs`).
//! stdin: {"options": {...}, "ops": [...]}; stdout: one JSON array of op results.
use pilotdeck_sdk::net::{ConnectionOptions, GatewayTransport, ReconnectOptions};
use pilotdeck_sdk::transport_core::SdkError;
use serde_json::{json, Value};
use std::io::Read;
use std::sync::{Arc, Mutex};

fn err(e: &SdkError) -> Value {
    let mut m = json!({"name": "PilotDeckError", "code": e.code, "message": e.message, "retryable": e.retryable});
    if let Some(d) = &e.details {
        m["details"] = d.clone();
    }
    if e.request_id.is_some() {
        m["requestId"] = json!("<id>");
    }
    m
}

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    let spec: Value = serde_json::from_str(&input).unwrap();
    let o = &spec["options"];
    let num = |v: &Value| v.as_f64();
    let options = ConnectionOptions {
        url: o["url"].as_str().unwrap().into(),
        token: o["token"].as_str().unwrap_or("").into(),
        client_name: o["clientName"].as_str().map(Into::into),
        client_version: o["clientVersion"].as_str().map(Into::into),
        protocol_version: o["protocolVersion"].as_str().map(Into::into),
        connect_timeout_ms: o["connectTimeoutMs"].as_u64(),
        request_timeout_ms: o["requestTimeoutMs"].as_u64(),
        reconnect: o.get("reconnect").map(|r| ReconnectOptions { max_attempts: num(&r["maxAttempts"]), initial_delay_ms: num(&r["initialDelayMs"]), max_delay_ms: num(&r["maxDelayMs"]), jitter: num(&r["jitter"]) }),
    };
    let t = GatewayTransport::new(options);
    let notes = Arc::new(Mutex::new(vec![]));
    let n2 = notes.clone();
    t.on_notification(move |v| n2.lock().unwrap().push(v.clone()));
    let mut out = vec![];
    for op in spec["ops"].as_array().unwrap() {
        let method = op["method"].as_str().unwrap_or("");
        let params = op.get("params").cloned().unwrap_or(Value::Null);
        out.push(match op["op"].as_str().unwrap() {
            "connect" => match t.connect() {
                Ok(v) => json!({"ok": v}),
                Err(e) => json!({"err": err(&e)}),
            },
            "request" => match t.request(method, &params) {
                Ok(v) => json!({"ok": v.unwrap_or(json!("__undefined"))}),
                Err(e) => json!({"err": err(&e)}),
            },
            "stream" => {
                let mut items = vec![];
                let mut end = json!("done");
                for r in t.stream(method, &params) {
                    match r {
                        Ok(v) => items.push(v),
                        Err(e) => {
                            end = json!({"err": err(&e)});
                            break;
                        }
                    }
                }
                json!({"items": items, "end": end})
            }
            "close" => {
                t.close();
                json!("closed")
            }
            "sleep" => {
                std::thread::sleep(std::time::Duration::from_millis(op["ms"].as_u64().unwrap()));
                json!("slept")
            }
            "notes" => json!(std::mem::take(&mut *notes.lock().unwrap())),
            other => panic!("op {other}"),
        });
    }
    println!("{}", Value::Array(out));
}
