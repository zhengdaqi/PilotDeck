//! Port of `handleCronCommand` (src/cli/pilotdeck.ts). The TS command never
//! exits after printing while a server is connected (SRC-DEFECT-001); the Rust
//! port prints identical output and then exits with the code TS would set.

use pilotdeck_gateway_client::{json_to_js, Gateway};
use pilotdeck_skills::jsjson::stringify_pretty;
use serde_json::{json, Map, Value};
use std::process::ExitCode;

fn flag<'a>(argv: &'a [String], f: &str) -> Option<&'a str> {
    let i = argv.iter().position(|a| a == f)?;
    argv.get(i + 1).map(String::as_str)
}

/// `Number.parseInt(value, 10)` → finite or None.
fn parse_int(v: &str) -> Option<f64> {
    let t = pilotdeck_session_search::js::js_trim(v);
    let (neg, rest) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let n: f64 = digits.parse().ok()?;
    Some(if neg { -n } else { n })
}

fn infer_channel_key(session_key: &str) -> String {
    match session_key.find(':') {
        Some(i) if i > 0 => session_key[..i].to_string(),
        _ => "cli".into(),
    }
}

fn num(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        json!(n as i64)
    } else {
        json!(n)
    }
}

fn finish(gw: Gateway, method: &str, params: Value) -> ExitCode {
    let mut gw = gw;
    let r = gw.request(method, params);
    gw.close();
    match r {
        Ok(v) => {
            println!("{}", stringify_pretty(&json_to_js(&v)));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{}", e.message());
            ExitCode::from(1)
        }
    }
}

fn usage(gw: Gateway, msg: &str) -> ExitCode {
    gw.close();
    eprintln!("{msg}");
    ExitCode::from(1)
}

pub fn run_cron_cli(argv: &[String]) -> ExitCode {
    let Some(gw) = Gateway::connect_if_available() else {
        eprintln!("pilotdeck cron requires a running pilotdeck server.");
        return ExitCode::from(1);
    };
    let has = |f: &str| argv.iter().any(|a| a == f);
    match argv.first().map(String::as_str) {
        Some("list") => {
            let mut p = Map::new();
            p.insert("includeHistory".into(), json!(has("--history")));
            if let Some(n) = flag(argv, "--limit").filter(|v| !v.is_empty()).and_then(parse_int) {
                p.insert("limit".into(), num(n));
            }
            finish(gw, "cron_list", Value::Object(p))
        }
        Some("create") => {
            let message = flag(argv, "--message").filter(|v| !v.is_empty());
            let session = flag(argv, "--session").filter(|v| !v.is_empty());
            let once = flag(argv, "--once").filter(|v| !v.is_empty());
            let cron = flag(argv, "--cron").filter(|v| !v.is_empty());
            let (Some(message), Some(session)) = (message, session) else {
                return usage(gw, "Usage: pilotdeck cron create --session <sessionKey> --message <text> (--once <iso> | --cron <expr>)");
            };
            if once.is_none() && cron.is_none() {
                return usage(gw, "Usage: pilotdeck cron create --session <sessionKey> --message <text> (--once <iso> | --cron <expr>)");
            }
            let mut p = Map::new();
            p.insert("message".into(), json!(message));
            p.insert("sessionKey".into(), json!(session));
            p.insert("channelKey".into(), json!(flag(argv, "--channel").map(str::to_string).unwrap_or_else(|| infer_channel_key(session))));
            p.insert(
                "projectKey".into(),
                json!(flag(argv, "--project").map(str::to_string).unwrap_or_else(pilotdeck_session_search::paths::cwd)),
            );
            let schedule = match once {
                Some(o) => json!({"type": "once", "runAt": o}),
                None => json!({"type": "cron", "expression": cron.unwrap()}),
            };
            p.insert("schedule".into(), schedule);
            if let Some(tz) = flag(argv, "--timezone") {
                p.insert("timezone".into(), json!(tz));
            }
            finish(gw, "cron_create", Value::Object(p))
        }
        Some("delete") => {
            let Some(task) = argv.get(1).map(String::as_str).or_else(|| flag(argv, "--task")).filter(|v| !v.is_empty()) else {
                return usage(gw, "Usage: pilotdeck cron delete <taskId> [--stop-running]");
            };
            finish(gw, "cron_delete", json!({"taskId": task, "stopRunning": has("--stop-running")}))
        }
        Some("stop") => {
            let task = argv.get(1).map(String::as_str).or_else(|| flag(argv, "--task"));
            let run = flag(argv, "--run");
            if task.filter(|v| !v.is_empty()).is_none() && run.filter(|v| !v.is_empty()).is_none() {
                return usage(gw, "Usage: pilotdeck cron stop <taskId> or pilotdeck cron stop --run <runId>");
            }
            let mut p = Map::new();
            if let Some(t) = task {
                p.insert("taskId".into(), json!(t));
            }
            if let Some(r) = run {
                p.insert("runId".into(), json!(r));
            }
            finish(gw, "cron_stop", Value::Object(p))
        }
        _ => usage(gw, "Usage: pilotdeck cron <list|create|delete|stop>"),
    }
}
