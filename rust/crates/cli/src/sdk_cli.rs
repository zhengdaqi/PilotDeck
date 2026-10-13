//! Port of `src/cli/sdkCli.ts` for `sessions` and `settings` (run/resume pending: CAP-SDK client port).

use pilotdeck_gateway_client::transport::{Options, Transport};
use pilotdeck_gateway_client::json_to_js;
use pilotdeck_skills::jsjson::{stringify_compact, stringify_pretty, JsObject, JsValue};
use std::process::ExitCode;
use std::time::Duration;

const VALUE_FLAGS: &[&str] = &["--gateway-url", "--auth-token", "--project", "--channel", "--model", "--permission-mode"];

fn read_value(argv: &[String], flag: &str) -> Option<String> {
    let i = argv.iter().position(|a| a == flag)?;
    argv.get(i + 1).filter(|v| !v.is_empty() && !v.starts_with("--")).cloned()
}

fn positional(argv: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let v = &argv[i];
        if VALUE_FLAGS.contains(&v.as_str()) {
            i += 2;
            continue;
        }
        if !v.starts_with("--") {
            out.push(v.clone());
        }
        i += 1;
    }
    out
}

struct Connection {
    url: String,
    token: String,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn read_connection(argv: &[String]) -> Result<Connection, String> {
    let url = read_value(argv, "--gateway-url")
        .or_else(|| env("PILOTDECK_SDK_GATEWAY_URL"))
        .or_else(|| env("PILOTDECK_GATEWAY_URL"))
        .unwrap_or_else(|| format!("ws://127.0.0.1:{}/ws", env("PILOTDECK_GATEWAY_PORT").unwrap_or_else(|| "18789".into())));
    let token = read_value(argv, "--auth-token")
        .or_else(|| env("PILOTDECK_SDK_AUTH_TOKEN"))
        .or_else(|| env("PILOTDECK_TOKEN"))
        .unwrap_or_else(|| pilotdeck_gateway_client::read_token().unwrap_or_default());
    if token.is_empty() {
        return Err("Gateway auth token is required. Set PILOTDECK_TOKEN or pass --auth-token.".into());
    }
    Ok(Connection { url, token })
}

fn check_connection_options(c: &Connection) -> Result<(), String> {
    if c.url.is_empty() || c.token.is_empty() {
        return Err("gatewayUrl and authToken are required; pass them in query options or createPilotDeckClient defaults.".into());
    }
    Ok(())
}

fn connect(c: &Connection, client_version: &str) -> Result<Transport, String> {
    Transport::connect(&Options {
        url: &c.url,
        token: &c.token,
        client_name: "sdk",
        client_version,
        connect_timeout: Duration::from_secs(10),
        request_timeout: Duration::from_secs(30),
    })
}

/// One request over a fresh transport, closed afterwards (TS `finally { close() }`).
fn one_shot(c: &Connection, client_version: &str, method: &str, params: &JsValue) -> Result<serde_json::Value, String> {
    let mut t = connect(c, client_version)?;
    let r = t.request_raw(method, &stringify_compact(params));
    t.close();
    r
}

fn field_or_empty(v: &serde_json::Value, key: &str) -> JsValue {
    match v.get(key) {
        Some(x) if !x.is_null() => json_to_js(x),
        _ => JsValue::Arr(vec![]),
    }
}

fn obj(pairs: Vec<(&str, Option<JsValue>)>) -> JsValue {
    let mut o = JsObject::new();
    for (k, v) in pairs {
        o.set(k, v.unwrap_or(JsValue::Undefined));
    }
    JsValue::Obj(o)
}

fn sessions(argv: &[String]) -> Result<(), String> {
    let c = read_connection(argv)?;
    let project = read_value(argv, "--project").map(JsValue::Str);
    check_connection_options(&c)?;
    let sub = argv.first().map(String::as_str).unwrap_or("list");
    match sub {
        "list" => {
            let r = one_shot(&c, "pilotdeck-cli", "list_sessions", &obj(vec![("projectKey", project)]))?;
            println!("{}", stringify_pretty(&field_or_empty(&r, "sessions")));
            Ok(())
        }
        "messages" => {
            let Some(id) = positional(&argv[1..]).into_iter().next() else {
                return Err("Usage: pilotdeck sessions messages <session-id>".into());
            };
            let r = one_shot(&c, "pilotdeck-cli", "read_session_messages",
                &obj(vec![("sessionKey", Some(JsValue::Str(id))), ("projectKey", project)]))?;
            println!("{}", stringify_pretty(&field_or_empty(&r, "messages")));
            Ok(())
        }
        _ => Err("Usage: pilotdeck sessions <list|messages> ...".into()),
    }
}

fn settings(argv: &[String]) -> Result<(), String> {
    let c = read_connection(argv)?;
    let sub = argv.first().map(String::as_str).unwrap_or("get");
    match sub {
        "get" => {
            check_connection_options(&c)?;
            let r = one_shot(&c, "0.1.0", "resolve_settings", &JsValue::Obj(JsObject::new()))?;
            println!("{}", stringify_pretty(&json_to_js(&r)));
            Ok(())
        }
        "set" => {
            let rest = &argv[1..];
            let raw = read_value(rest, "--json").unwrap_or_else(|| positional(rest).join(" "));
            if raw.is_empty() {
                return Err("Usage: pilotdeck settings set '<json>'".into());
            }
            let settings = pilotdeck_skills::v8json::parse(&raw).map_err(|e| format!("Invalid settings JSON: {e}"))?;
            if !matches!(settings, JsValue::Obj(_)) {
                return Err("updateSettings settings must be an object.".into());
            }
            check_connection_options(&c)?;
            one_shot(&c, "0.1.0", "update_settings",
                &obj(vec![("source", Some(JsValue::Str("localSettings".into()))), ("settings", Some(settings))]))?;
            println!("{{\"updated\":true}}");
            Ok(())
        }
        _ => Err("Usage: pilotdeck settings <get|set> ...".into()),
    }
}

pub fn run(command: &str, argv: &[String]) -> ExitCode {
    let r = match command {
        "sessions" => sessions(argv),
        _ => settings(argv),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(m) => {
            eprintln!("{m}");
            ExitCode::from(1)
        }
    }
}
