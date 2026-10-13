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

// ---------------------------------------------------------------------------
// run / resume (sdkCli.ts#runTurn + client.ts runs.start / PilotDeckQueryImpl)
// ---------------------------------------------------------------------------

use pilotdeck_gateway_client::transport::StreamItem;
use std::io::{IsTerminal, Read, Write};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn get<'a>(o: &'a JsValue, k: &str) -> Option<&'a JsValue> {
    match o {
        JsValue::Obj(m) => m.get(k),
        _ => None,
    }
}

fn as_record(v: Option<&JsValue>) -> JsValue {
    match v {
        Some(o @ JsValue::Obj(_)) => o.clone(),
        Some(a @ JsValue::Arr(_)) => a.clone(),
        _ => JsValue::Obj(JsObject::new()),
    }
}

fn js_string(v: Option<&JsValue>) -> String {
    match v {
        None | Some(JsValue::Undefined) => "undefined".into(),
        Some(JsValue::Null) => "null".into(),
        Some(JsValue::Str(s)) => s.clone(),
        Some(JsValue::Bool(b)) => b.to_string(),
        Some(JsValue::Num(n)) => pilotdeck_skills::jsjson::js_number_to_string(*n),
        Some(JsValue::Arr(_)) | Some(JsValue::Obj(_)) => "[object Object]".into(),
    }
}

/// `mapPublicMessage(raw, sessionId)`.
fn map_public_message(raw: &serde_json::Value, session_id: &str) -> JsValue {
    let event = json_to_js(raw);
    let ty = match get(&event, "type") {
        None | Some(JsValue::Undefined) | Some(JsValue::Null) => "unknown".to_string(),
        v => js_string(v),
    };
    let mut m = JsObject::new();
    m.set("type", JsValue::Str(ty.clone()));
    if let JsValue::Obj(e) = &event {
        for k in e.keys() {
            m.set(k.clone(), e.get(&k).unwrap().clone());
        }
    }
    m.set("sessionId", JsValue::Str(session_id.to_string()));
    m.set("runId", match get(&event, "runId") { Some(v) => v.clone(), None => JsValue::Undefined });
    m.set("sequence", match get(&event, "sequence") { Some(v) => v.clone(), None => JsValue::Undefined });
    let ok_false = matches!(get(&event, "ok"), Some(JsValue::Bool(false)));
    let mapped = match ty.as_str() {
        "turn_started" => "turn.started",
        "input_accepted" => "user.accepted",
        "assistant_text_delta" => "assistant.message",
        "assistant_thinking_delta" => "assistant.thinking",
        "subagent_text_delta" => "subagent.message",
        "tool_call_started" => "tool.started",
        "tool_progress" => "tool.progress",
        "prompt_suggestion" => "prompt_suggestion",
        "tool_call_finished" => if ok_false { "tool.failed" } else { "tool.completed" },
        "tool_result_detail_available" => "tool.result_detail",
        "permission_request" => "permission.requested",
        "hook_started" => "hook.started",
        "hook_response" => "hook.response",
        "hook_async_result" => "hook.async_result",
        "elicitation_request" => "elicitation.requested",
        "elicitation_cancelled" => "elicitation.cancelled",
        "user_dialog_request" => "user_dialog.requested",
        "user_dialog_cancelled" => "user_dialog.cancelled",
        "structured_output" => "structured_output",
        "context_budget" => "context.usage",
        "turn_completed" => "result",
        "error" => "error",
        _ => "",
    };
    let new_type = if !mapped.is_empty() {
        mapped.to_string()
    } else if ty.contains('.') {
        ty.clone()
    } else {
        format!("pilotdeck.{ty}")
    };
    m.set("type", JsValue::Str(new_type));
    if ty == "turn_completed" {
        m.set("status", JsValue::Str("completed".into()));
        m.set("output", get(&event, "result").cloned().unwrap_or(JsValue::Undefined));
        m.set("finishReason", get(&event, "finishReason").cloned().unwrap_or(JsValue::Undefined));
    }
    if ty == "error" {
        m.set("status", JsValue::Str("failed".into()));
    }
    if ty == "permission_denied" {
        m.set("type", JsValue::Str("permission.denied".into()));
    }
    if ty == "context_budget" {
        let keys = [
            "used", "displayUsed", "budgetUsed", "localEstimateTokens", "displayTokens", "estimateSource", "usageTokens",
            "calibrationActualInputTokens", "calibrationEstimatedInputTokens", "total", "totalContextTokens",
            "maxContextTokens", "effectiveTotal", "effectiveContextTokens", "maxOutputTokens", "reservedOutputTokens",
            "warningRatio", "blockingRatio", "ratio", "state", "source", "exact", "estimatorError", "breakdown",
        ];
        let mut u = JsObject::new();
        for k in keys {
            if let Some(v) = get(&event, k) {
                if !matches!(v, JsValue::Undefined) {
                    u.set(k, v.clone());
                }
            }
        }
        m.set("usage", JsValue::Obj(u));
    }
    JsValue::Obj(m)
}

/// `new PilotDeckError({code, message, details})` as seen by JSON.stringify.
fn pilotdeck_error_json(code: &str, details: JsValue) -> JsValue {
    JsValue::Obj(
        JsObject::new()
            .with("code", JsValue::Str(code.into()))
            .with("details", details)
            .with("retryable", JsValue::Bool(false))
            .with("name", JsValue::Str("PilotDeckError".into())),
    )
}

fn result_unknown(session: &str, run_id: &Option<String>) -> JsValue {
    JsValue::Obj(JsObject::new().with("status", JsValue::Str("result_unknown".into())).with(
        "recovery",
        JsValue::Obj(
            JsObject::new()
                .with("sessionId", JsValue::Str(session.into()))
                .with("runId", run_id.clone().map(JsValue::Str).unwrap_or(JsValue::Undefined)),
        ),
    ))
}

fn js_trim_owned(s: &str) -> String {
    pilotdeck_session_search::js::js_trim(s).to_string()
}

fn event_text(ev: &JsValue) -> Option<String> {
    let ty = js_string(get(ev, "type"));
    if !ty.contains("assistant") && ty != "subagent.message" {
        return None;
    }
    if let Some(JsValue::Str(t)) = get(ev, "text") {
        return Some(t.clone());
    }
    if let Some(JsValue::Str(t)) = get(ev, "delta") {
        return Some(t.clone());
    }
    None
}

fn run_turn(command: &str, argv: &[String]) -> Result<ExitCode, String> {
    let c = read_connection(argv)?;
    let project = read_value(argv, "--project");
    let channel = read_value(argv, "--channel").unwrap_or_else(|| "cli".into());
    check_connection_options(&c)?;
    let pj = || project.clone().map(JsValue::Str).unwrap_or(JsValue::Undefined);
    // Control transport (client.request → lazily connected, closed by client.close()).
    let mut control: Option<Transport> = None;
    let result = (|| -> Result<ExitCode, String> {
        let pos = positional(argv);
        let session_id: String = if command == "run" {
            let t = control.insert(connect(&c, "pilotdeck-cli")?);
            let created = t.request_raw("new_session", &stringify_compact(&obj(vec![
                ("projectKey", project.clone().map(JsValue::Str)),
                ("channelKey", Some(JsValue::Str(channel.clone()))),
                ("hint", None),
            ])))?;
            created.get("sessionKey").and_then(|v| v.as_str()).map(String::from).unwrap_or_default()
        } else {
            let id = pos.first().cloned().unwrap_or_default();
            if id.is_empty() {
                return Err("Usage: pilotdeck resume <session-id> <prompt>".into());
            }
            let t = control.insert(connect(&c, "pilotdeck-cli")?);
            t.request_raw("resume_session", &stringify_compact(&obj(vec![("sessionKey", Some(JsValue::Str(id.clone())))])))?;
            id
        };
        // readPrompt
        let skip = if command == "resume" { 1 } else { 0 };
        let mut prompt = js_trim_owned(&pos.iter().skip(skip).cloned().collect::<Vec<_>>().join(" "));
        if prompt.is_empty() && !std::io::stdin().is_terminal() {
            let mut buf = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut buf);
            prompt = js_trim_owned(&String::from_utf8_lossy(&buf));
        }
        if prompt.is_empty() {
            return Err("A prompt is required, or provide it through stdin.".into());
        }
        let model = read_value(argv, "--model");
        let permission_mode = read_value(argv, "--permission-mode");
        let json_events = argv.iter().any(|a| a == "--stream-json");
        let requested_run_id = pilotdeck_gateway_client::random_uuid_v4();

        let interrupted = Arc::new(AtomicBool::new(false));
        let _ = signal_hook::flag::register(signal_hook::consts::SIGINT, interrupted.clone());
        let _ = signal_hook::flag::register(signal_hook::consts::SIGTERM, interrupted.clone());

        // Run query transport (PilotDeckQueryImpl.initializeInternalWithoutTimeout).
        let mut rt = connect(&c, "pilotdeck-cli")?;
        let mut run_id = Some(requested_run_id.clone());
        let mut stdout = std::io::stdout().lock();
        let mut streamed_text = false;
        let mut output_chunks: Vec<String> = Vec::new();
        let mut terminal_seen = false;
        let mut final_result: Option<JsValue> = None;
        let outcome = (|| -> Result<(), String> {
            rt.request_raw("resume_session", &stringify_compact(&obj(vec![("sessionKey", Some(JsValue::Str(session_id.clone())))])))?;
            let model_override = match &model {
                None => None,
                Some(m) => {
                    let Some(p) = &project else {
                        return Err("projectKey is required when options.model is set.".into());
                    };
                    let r = rt.request_raw("model_catalog_list", &stringify_compact(&obj(vec![("projectKey", Some(JsValue::Str(p.clone())))])))?;
                    let items = r.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);
                    let matches: Vec<_> = items
                        .iter()
                        .filter(|c| c.get("available") != Some(&serde_json::Value::Bool(false)))
                        .filter(|c| {
                            s(c, "id").as_deref() == Some(m.as_str())
                                || format!("{}/{}", js_string(c.get("provider").map(json_to_js).as_ref()), js_string(c.get("model").map(json_to_js).as_ref())) == *m
                                || s(c, "model").as_deref() == Some(m.as_str())
                        })
                        .collect();
                    if matches.len() != 1 || s(matches[0], "provider").unwrap_or_default().is_empty() || s(matches[0], "model").unwrap_or_default().is_empty() {
                        return Err(format!("Model is not uniquely resolvable: {m}"));
                    }
                    Some(JsValue::Obj(JsObject::new()
                        .with("mode", JsValue::Str("model".into()))
                        .with("provider", JsValue::Str(s(matches[0], "provider").unwrap()))
                        .with("model", JsValue::Str(s(matches[0], "model").unwrap()))))
                }
            };
            let pm = permission_mode.clone();
            let turn_mode = pm.as_deref().map(|m| if matches!(m, "acceptEdits" | "dontAsk" | "auto") { "default".to_string() } else { m.to_string() });
            let base_mode = pm.as_deref().map(|m| if m == "bypassPermissions" { "bypassPermissions" } else { "default" });
            let sdk_pm = pm.as_deref().filter(|m| matches!(*m, "acceptEdits" | "dontAsk"));
            let session_config = match sdk_pm {
                Some(m) => Some(JsValue::Obj(JsObject::new().with("permissionMode", JsValue::Str(m.into())))),
                None if rt.capabilities().iter().any(|c| c == "sdk_session_defaults") => Some(JsValue::Obj(JsObject::new())),
                None => None,
            };
            let params = obj(vec![
                ("sessionKey", Some(JsValue::Str(session_id.clone()))),
                ("channelKey", Some(JsValue::Str("api_server".into()))),
                ("projectKey", Some(pj())),
                ("message", Some(JsValue::Str(prompt.clone()))),
                ("workspaceCwd", None),
                ("maxTurns", None),
                ("maxBudgetUsd", None),
                ("timeoutMs", None),
                ("canPrompt", Some(JsValue::Bool(false))),
                ("canElicit", Some(JsValue::Bool(false))),
                ("mode", turn_mode.map(JsValue::Str)),
                ("basePermissionMode", base_mode.map(|s| JsValue::Str(s.into()))),
                ("sdkPermissionMode", sdk_pm.map(|s| JsValue::Str(s.into()))),
                ("allowedTools", None),
                ("disallowedTools", None),
                ("sdkSessionConfig", session_config),
                ("modelOverride", model_override),
                ("runId", Some(JsValue::Str(requested_run_id.clone()))),
            ]);
            let sid = rt.stream_start("submit_turn", &stringify_compact(&params))?;
            loop {
                match rt.stream_next(&sid, &interrupted)? {
                    StreamItem::End => {
                        if final_result.is_none() {
                            final_result = Some(result_unknown(&session_id, &run_id));
                        }
                        return Ok(());
                    }
                    StreamItem::Interrupted => {
                        // run.abort("CLI interrupted"): abort_turn, then the transport closes under the pending read.
                        rt.request_raw("abort_turn", &stringify_compact(&obj(vec![
                            ("sessionKey", Some(JsValue::Str(session_id.clone()))),
                            ("runId", run_id.clone().map(JsValue::Str)),
                            ("reason", Some(JsValue::Str("CLI interrupted".into()))),
                        ])))?;
                        final_result = Some(JsValue::Obj(JsObject::new()
                            .with("status", JsValue::Str("aborted".into()))
                            .with("reason", JsValue::Str("CLI interrupted".into()))));
                        return Ok(());
                    }
                    StreamItem::Event(raw) => {
                        let mapped = map_public_message(&raw, &session_id);
                        let ty = js_string(get(&mapped, "type"));
                        if ty == "assistant.message" {
                            if let Some(JsValue::Str(t)) = get(&mapped, "text") {
                                output_chunks.push(t.clone());
                            }
                        }
                        if ty == "assistant.message" || ty == "assistant.thinking" {
                            continue;
                        }
                        if ty == "turn.started" {
                            if let Some(JsValue::Str(r)) = get(&mapped, "runId") {
                                run_id = Some(r.clone());
                            }
                        }
                        let mut structured: Option<JsValue> = None;
                        if ty == "structured_output" {
                            structured = Some(get(&mapped, "payload").cloned().unwrap_or(JsValue::Undefined));
                        }
                        let _ = structured;
                        let mut skip = false;
                        if ty == "result" {
                            if terminal_seen {
                                skip = true;
                            } else {
                                terminal_seen = true;
                                let output = STRUCTURED.with(|s| s.borrow().clone()).unwrap_or_else(|| {
                                    if output_chunks.is_empty() { JsValue::Undefined } else { JsValue::Str(output_chunks.concat()) }
                                });
                                final_result = Some(JsValue::Obj(JsObject::new()
                                    .with("status", JsValue::Str("completed".into()))
                                    .with("output", output)
                                    .with("usage", as_record(get(&mapped, "usage")))
                                    .with("finishReason", get(&mapped, "finishReason").cloned().unwrap_or(JsValue::Undefined))));
                            }
                        }
                        if ty == "error" {
                            if terminal_seen {
                                skip = true;
                            } else {
                                terminal_seen = true;
                                let code = match get(&mapped, "code") {
                                    None | Some(JsValue::Undefined) | Some(JsValue::Null) => "server_error".to_string(),
                                    v => js_string(v),
                                };
                                final_result = Some(if code == "result_unknown" {
                                    result_unknown(&session_id, &run_id)
                                } else {
                                    JsValue::Obj(JsObject::new()
                                        .with("status", JsValue::Str("failed".into()))
                                        .with("error", pilotdeck_error_json(&code, as_record(get(&mapped, "detail")))))
                                });
                            }
                        }
                        if ty == "structured_output" {
                            STRUCTURED.with(|s| *s.borrow_mut() = Some(get(&mapped, "payload").cloned().unwrap_or(JsValue::Undefined)));
                        }
                        if skip {
                            continue;
                        }
                        if json_events {
                            let _ = writeln!(stdout, "{}", stringify_compact(&mapped));
                        } else if let Some(t) = event_text(&mapped) {
                            if !t.is_empty() {
                                let _ = write!(stdout, "{t}");
                                let _ = stdout.flush();
                                streamed_text = true;
                            }
                        }
                    }
                }
            }
        })();
        rt.close();
        outcome?;
        let result = final_result.unwrap_or_else(|| result_unknown(&session_id, &run_id));
        let completed = matches!(get(&result, "status"), Some(JsValue::Str(s)) if s == "completed");
        if streamed_text && completed {
            let _ = writeln!(stdout);
        }
        if argv.iter().any(|a| a == "--json") {
            let o = JsValue::Obj(JsObject::new().with("sessionId", JsValue::Str(session_id.clone())).with("result", result.clone()));
            let _ = writeln!(stdout, "{}", stringify_compact(&o));
        } else if !completed || !streamed_text {
            let output = if completed { get(&result, "output").cloned().unwrap_or(JsValue::Undefined) } else { result.clone() };
            match output {
                JsValue::Undefined => {}
                JsValue::Str(s) => {
                    let _ = writeln!(stdout, "{s}");
                }
                other => {
                    let _ = writeln!(stdout, "{}", stringify_compact(&other));
                }
            }
        }
        let _ = stdout.flush();
        Ok(if completed { ExitCode::SUCCESS } else { ExitCode::from(1) })
    })();
    if let Some(t) = control {
        t.close();
    }
    result
}

thread_local! {
    static STRUCTURED: std::cell::RefCell<Option<JsValue>> = const { std::cell::RefCell::new(None) };
}

pub fn run_turn_cli(command: &str, argv: &[String]) -> ExitCode {
    match run_turn(command, argv) {
        Ok(code) => code,
        Err(m) => {
            eprintln!("{m}");
            ExitCode::from(1)
        }
    }
}
