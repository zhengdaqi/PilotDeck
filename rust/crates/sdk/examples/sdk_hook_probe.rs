//! Parity probe for `pilotdeck_sdk::hook_server` (driven by `rust/parity/sdk/hook_server_parity.mjs`).
//! stdin line 1: {"hooks": {event: [{matcher?, timeout?, hooks: [callback names]}]}, "start": {...}}.
//! Prints the config (or error) as one JSON line, serves until stdin gets "close", then prints "closed".
use pilotdeck_sdk::hook_server::{HookCallback, HookCallbackMatcher, HookError, HookServerStartOptions, HostedHookServer};
use pilotdeck_sdk::js::stringify;
use pilotdeck_sdk::transport_core::SdkError;
use serde_json::{json, Value};
use std::io::BufRead;
use std::sync::Arc;

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) if s == "NaN" => Some(f64::NAN),
        Value::String(s) if s == "Infinity" => Some(f64::INFINITY),
        _ => None,
    }
}

/// The callback catalogue shared with the TS side.
fn callback(name: &str) -> HookCallback {
    let name = name.to_string();
    Arc::new(move |inp: &Value, tid: Option<&str>, ctx| {
        let ev = inp["hook_event_name"].clone();
        Ok(Some(match name.as_str() {
            "allow" => json!({"hookSpecificOutput": {"hookEventName": ev, "permissionDecision": "allow"}}),
            "defer" => json!({"hookSpecificOutput": {"hookEventName": ev, "permissionDecision": "defer", "additionalContext": "d"}}),
            "wrong" => json!({"hookSpecificOutput": {"hookEventName": "Stop"}}),
            "specStr" => json!({"hookSpecificOutput": "abc"}),
            "block" => json!({"decision": "block", "reason": "r"}),
            "approve" => json!({"decision": "approve", "reason": "ok"}),
            "cont" => json!({"continue": false, "stopReason": "s"}),
            "sys1" => json!({"systemMessage": "one"}),
            "sys2" => json!({"systemMessage": "two", "continue": true}),
            "echo" => {
                // JSON.stringify drops an undefined tid.
                let mut o = serde_json::Map::new();
                o.insert("inp".into(), inp.clone());
                if let Some(t) = tid {
                    o.insert("tid".into(), json!(t));
                }
                o.insert("signal".into(), json!("object"));
                o.insert("idLen".into(), json!(ctx.async_hook_id.len()));
                json!({ "systemMessage": stringify(&Value::Object(o)) })
            }
            "async" => json!({"async": true, "asyncTimeout": 5}),
            "asyncNoT" => json!({"async": true}),
            "asyncBad" => json!({"async": true, "asyncTimeout": 0}),
            "asyncStr" => json!({"async": true, "asyncTimeout": "5"}),
            "none" => return Ok(None),
            "zero" => json!(0),
            "str" => json!("xy"),
            "num" => json!(7),
            "arr" => json!(["a"]),
            "throwSdk" => return Err(HookError::Sdk(SdkError::new("conflict", "c", false))),
            "throwErr" => return Err(HookError::Error("e".into())),
            "throwStr" => return Err(HookError::Other),
            "slow" => {
                std::thread::sleep(std::time::Duration::from_millis(400));
                json!({"systemMessage": "slow"})
            }
            "idx" => serde_json::from_str(r#"{"2":"b","1":"a","z":1}"#).unwrap(),
            other => panic!("callback {other}"),
        }))
    })
}

fn main() {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let spec: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    let hooks = spec["hooks"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(ev, ms)| {
            let ms = ms
                .as_array()
                .unwrap()
                .iter()
                .map(|m| HookCallbackMatcher {
                    matcher: m["matcher"].as_str().map(String::from),
                    timeout: m.get("timeout").and_then(num),
                    hooks: m["hooks"].as_array().unwrap().iter().map(|h| callback(h.as_str().unwrap())).collect(),
                })
                .collect();
            (ev.clone(), ms)
        })
        .collect();
    let s = &spec["start"];
    let err = |e: SdkError| println!("{}", json!({"err": {"code": e.code, "message": e.message}}));
    let server = match HostedHookServer::new(hooks) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    let opts = HookServerStartOptions {
        host: s["host"].as_str().map(String::from),
        port: s["port"].as_u64().map(|p| p as u16),
        path: s["path"].as_str().map(String::from),
        public_url: s["publicUrl"].as_str().map(String::from),
    };
    match server.start(&opts) {
        Ok(c) => println!("{}", json!({ "ok": c })),
        Err(e) => return err(e),
    }
    for l in lines {
        if l.unwrap().trim() == "close" {
            server.close();
            println!("closed");
            return;
        }
    }
}
