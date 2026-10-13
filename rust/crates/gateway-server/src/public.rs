//! Public module-host HTTP transports in their bound configuration: ports of
//! `src/composition/publicApprovalHttpTransport.ts` and `publicHostHttpTransport.ts`.
//! The bound `PublicApprovalBridge` / `PublicHostProviderResolver` live in the Gateway host and are reached
//! through `GatewayBackend` (`__approvals.*`, `__host.*`); this module owns auth, method, size, input
//! validation, scope checks, status/header policy, stream framing and error mapping.
use crate::backend::GatewayBackend;
use crate::{get, obj, s, stringify_compact, JsValue};
use std::io::Write;
use std::net::TcpStream;

pub const WIRE_OPERATIONS: [&str; 22] = ["list_tools", "create_tool", "update_tool", "test_tool", "probe_unsaved_tool", "remove_tool",
    "list_general_skills", "import_general_skill", "publish_general_skill", "archive_general_skill", "test_general_skill",
    "list_model_catalog", "extract_sop_text", "model_prepare", "model_stream", "file_parse",
    "task_start", "task_status", "task_result", "task_cancel", "task_events", "read_sop_authority"];

pub enum BodyRead {
    Ok(Vec<u8>),
    TooLarge,
    Failed,
}

pub fn write_head(stream: &mut TcpStream, status: u16, headers: &[(String, String)], body_len: Option<usize>) {
    let mut h = format!("HTTP/1.1 {status} {}\r\n", crate::http::reason(status));
    for (k, v) in headers {
        h.push_str(&format!("{k}: {v}\r\n"));
    }
    match body_len {
        Some(n) => h.push_str(&format!("Content-Length: {n}\r\n")),
        None => h.push_str("Transfer-Encoding: chunked\r\n"),
    }
    h.push_str("Connection: close\r\n\r\n");
    let _ = stream.write_all(h.as_bytes());
}

fn send(stream: &mut TcpStream, status: u16, body: JsValue) {
    let b = stringify_compact(&body);
    write_head(stream, status, &[("content-type".into(), "application/json".into())], Some(b.len()));
    let _ = stream.write_all(b.as_bytes());
}

fn js_trim_nonempty(v: &JsValue) -> bool {
    matches!(v, JsValue::Str(x) if !x.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}').is_empty())
}

fn status_of(v: &JsValue) -> Option<u16> {
    match v {
        JsValue::Num(n) if n.fract() == 0.0 && *n >= 100.0 && *n <= 999.0 => Some(*n as u16),
        _ => None,
    }
}

fn parse_json(bytes: &[u8]) -> Option<JsValue> {
    pilotdeck_skills::v8json::parse(&String::from_utf8_lossy(bytes)).ok()
}

pub fn handle_approvals(stream: &mut TcpStream, gw: &dyn GatewayBackend, token: &str, path: &str, method: &str,
    auth: Option<&str>, approver: Option<&str>, read: &mut dyn FnMut(usize) -> BodyRead) {
    if token.is_empty() || auth != Some(format!("Bearer {token}").as_str()) {
        return send(stream, 401, obj(vec![("code", s("PUBLIC_HOST_TRANSPORT_UNAUTHORIZED"))]));
    }
    if method != "POST" {
        return send(stream, 405, obj(vec![("code", s("PUBLIC_HOST_METHOD_NOT_ALLOWED"))]));
    }
    let fail = |stream: &mut TcpStream, status: u16, code: &str| send(stream, status, obj(vec![("code", s(code))]));
    if !gw.has("__approvals") {
        return fail(stream, 503, "APPROVAL_BINDING_UNAVAILABLE");
    }
    let bytes = match read(64 * 1024) {
        BodyRead::Ok(b) => b,
        // Intended difference from TS (SRC-DEFECT-008, ADR-0004): TS means to send 413 APPROVAL_INPUT_TOO_LARGE, but its
        // abort path destroys the response, so the client gets none. Rust sends the 413.
        BodyRead::TooLarge => return fail(stream, 413, "APPROVAL_INPUT_TOO_LARGE"),
        BodyRead::Failed => return, // request aborted: TS destroys the response
    };
    let Some(body) = parse_json(&bytes) else { return fail(stream, 400, "APPROVAL_INPUT_INVALID") };
    let JsValue::Obj(o) = &body else { return fail(stream, 400, "APPROVAL_INPUT_INVALID") };
    let status_op = path.ends_with("/status");
    let mut allowed = vec!["tenantId", "agentId", "sessionKey", "projectKey"];
    if !status_op {
        allowed.extend(["source", "requestId", "waitId", "expectedRevision", "message", "slotUpdates"]);
    }
    if o.iter().any(|(k, _)| !allowed.contains(&k.as_str())) {
        return fail(stream, 400, "APPROVAL_AUTHORITY_OVERRIDE");
    }
    let binding = match gw.call("__approvals.binding", None) {
        Ok(b) => b,
        Err(_) => return fail(stream, 502, "PUBLIC_APPROVAL_BRIDGE_FAILED"),
    };
    if get(&body, "tenantId") != get(&binding, "tenantId") || get(&body, "agentId") != get(&binding, "agentId")
        || !matches!(get(&body, "tenantId"), JsValue::Str(_)) || !matches!(get(&body, "agentId"), JsValue::Str(_)) {
        return fail(stream, 403, "APPROVAL_SUBJECT_MISMATCH");
    }
    let mut input = o.clone();
    input.set("approverAuthorization", approver.map_or(JsValue::Undefined, s));
    let r = gw.call(if status_op { "__approvals.status" } else { "__approvals.resume" }, Some(obj(vec![("input", JsValue::Obj(input))])));
    let r = match r {
        Ok(r) => r,
        Err(_) => return fail(stream, 502, "PUBLIC_APPROVAL_BRIDGE_FAILED"),
    };
    let err = get(&r, "error");
    if !matches!(err, JsValue::Undefined) {
        let code = get(err, "code").clone();
        let status = status_of(get(err, "status"))
            .unwrap_or(if code == s("SOP_RESUME_REQUEST_CONFLICT") { 409 } else { 502 });
        let code = if matches!(code, JsValue::Undefined | JsValue::Null) { s("PUBLIC_APPROVAL_BRIDGE_FAILED") } else { code };
        return send(stream, status, obj(vec![("code", code)]));
    }
    let value = get(&r, "value").clone();
    if status_op {
        return send(stream, 200, obj(vec![("status", value)]));
    }
    // res.end(JSON.stringify(undefined)) sends an empty body.
    let b = if matches!(value, JsValue::Undefined) { String::new() } else { stringify_compact(&value) };
    write_head(stream, 200, &[("content-type".into(), "application/json".into())], Some(b.len()));
    let _ = stream.write_all(b.as_bytes());
}

fn transport_fail(stream: &mut TcpStream, err: &JsValue) {
    let status = status_of(get(err, "status")).unwrap_or(502);
    let code = match get(err, "code") {
        JsValue::Undefined | JsValue::Null => s("PUBLIC_HOST_TRANSPORT_FAILED"),
        c => c.clone(),
    };
    send(stream, status, obj(vec![("code", code), ("message", get(err, "message").clone())]));
}

pub fn handle_host(stream: &mut TcpStream, gw: &dyn GatewayBackend, token: &str, path: &str, method: &str,
    auth: Option<&str>, read: &mut dyn FnMut(usize) -> BodyRead) {
    if auth != Some(format!("Bearer {token}").as_str()) {
        return send(stream, 401, obj(vec![("code", s("PUBLIC_HOST_TRANSPORT_UNAUTHORIZED"))]));
    }
    if method != "POST" {
        return send(stream, 405, obj(vec![("code", s("PUBLIC_HOST_METHOD_NOT_ALLOWED"))]));
    }
    let code = |stream: &mut TcpStream, st: u16, c: &str| send(stream, st, obj(vec![("code", s(c))]));
    if !gw.has("__host") {
        return code(stream, 501, "PUBLIC_HOST_PORT_UNAVAILABLE");
    }
    let bytes = match read(32 * 1024 * 1024) {
        BodyRead::Ok(b) => b,
        BodyRead::TooLarge => return code(stream, 413, "PUBLIC_HOST_INPUT_TOO_LARGE"),
        BodyRead::Failed => return,
    };
    let Some(input) = parse_json(&bytes) else { return code(stream, 400, "PUBLIC_HOST_INPUT_INVALID") };
    let principal = match &input { JsValue::Obj(_) => get(&input, "principal").clone(), _ => JsValue::Undefined };
    if !["pilotDeckUserId", "tenantId", "actorUserId", "agentId"].iter().all(|k| js_trim_nonempty(get(&principal, k))) {
        return code(stream, 400, "PUBLIC_HOST_PRINCIPAL_REQUIRED");
    }
    let resolved = match gw.call("__host.resolve", Some(obj(vec![("principal", principal.clone())]))) {
        Ok(r) => r,
        Err(e) => return transport_fail(stream, &obj(vec![("message", s(&format!("{e:?}")))])),
    };
    if !matches!(get(&resolved, "error"), JsValue::Undefined) {
        return transport_fail(stream, get(&resolved, "error"));
    }
    let handle = get(&resolved, "handle").clone();
    let release = || { let _ = gw.call("__host.release", Some(obj(vec![("handle", handle.clone())]))); };
    let ops: Vec<JsValue> = match get(&resolved, "operations") {
        JsValue::Arr(a) => a.iter().filter(|o| matches!(o, JsValue::Str(x) if WIRE_OPERATIONS.contains(&x.as_str()))).cloned().collect(),
        _ => Vec::new(),
    };
    if path.ends_with("/describe") {
        release();
        return send(stream, 200, obj(vec![("operations", JsValue::Arr(ops))]));
    }
    let operation = get(&input, "operation").clone();
    if !ops.contains(&operation) {
        release();
        return code(stream, 409, "PILOTDECK_HOST_CAPABILITY_UNAVAILABLE");
    }
    let call_input = get(&input, "input").clone();
    let JsValue::Obj(ci) = &call_input else {
        release();
        return code(stream, 400, "PUBLIC_HOST_INPUT_INVALID");
    };
    if ["tenantId", "tenant_id", "actorUserId", "actor_user_id", "credentialId", "agentId", "agent_id"].iter().any(|k| ci.get(k).is_some()) {
        release();
        return code(stream, 400, "PUBLIC_SCOPE_OVERRIDE");
    }
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    // State machine over the host's events: head, then chunks (stream mode only).
    let mut decided: Option<bool> = None; // Some(true) = streaming headers sent; Some(false) = response complete
    let mut broken = false;
    let params = obj(vec![("handle", handle.clone()), ("operation", operation), ("input", call_input.clone()), ("principal", principal)]);
    let result = gw.call_events("__host.call", params, &mut |ev| {
        if broken || decided == Some(false) {
            return;
        }
        if let JsValue::Obj(_) = get(&ev, "head") {
            let head = get(&ev, "head");
            let Some(st) = status_of(get(head, "status")).filter(|v| *v <= 599) else {
                transport_fail(stream, &obj(vec![("status", JsValue::Num(502.0)), ("message", s("Invalid host status"))]));
                decided = Some(false);
                return;
            };
            if let JsValue::Obj(_) = get(head, "headersError") {
                transport_fail(stream, &obj(vec![("message", get(get(head, "headersError"), "message").clone())]));
                decided = Some(false);
                return;
            }
            let ct = match get(head, "contentType") { JsValue::Str(c) => Some(c.clone()), _ => None };
            let streaming_ct = ct.as_deref().is_some_and(|c| c.starts_with("application/x-ndjson") || c.starts_with("text/event-stream"));
            if get(head, "isStream") == &JsValue::Bool(true) && !streaming_ct {
                transport_fail(stream, &obj(vec![("status", JsValue::Num(502.0)), ("code", s("PILOTDECK_HOST_RESPONSE_INVALID")),
                    ("message", s("Host stream content-type is required."))]));
                decided = Some(false);
                return;
            }
            let mut headers: Vec<(String, String)> = match get(head, "headers") {
                JsValue::Arr(a) => a.iter().filter_map(|kv| match kv { JsValue::Arr(p) if p.len() == 2 => match (&p[0], &p[1]) {
                    (JsValue::Str(k), JsValue::Str(v)) => Some((k.clone(), v.clone())), _ => None }, _ => None }).collect(),
                _ => Vec::new(),
            };
            match get(head, "mode") {
                JsValue::Str(m) if m == "stream" => {
                    write_head(stream, st, &headers, None);
                    decided = Some(true);
                }
                JsValue::Str(m) if m == "missing_stream" => {
                    transport_fail(stream, &obj(vec![("status", JsValue::Num(502.0)), ("message", s("Missing host stream"))]));
                    decided = Some(false);
                }
                mode => {
                    if mode == &s("json") && ct.is_none() {
                        headers.push(("content-type".into(), "application/json".into()));
                    }
                    let body = match get(head, "body") { JsValue::Str(b) => b64.decode(b).unwrap_or_default(), _ => Vec::new() };
                    write_head(stream, st, &headers, Some(body.len()));
                    let _ = stream.write_all(&body);
                    decided = Some(false);
                }
            }
        } else if let JsValue::Str(c) = get(&ev, "chunk") {
            if decided == Some(true) {
                let bytes = b64.decode(c).unwrap_or_default();
                if !bytes.is_empty() {
                    let ok = write!(stream, "{:x}\r\n", bytes.len()).and_then(|_| stream.write_all(&bytes)).and_then(|_| stream.write_all(b"\r\n"));
                    if ok.is_err() {
                        broken = true;
                    }
                }
            }
        }
    });
    match result {
        Ok(r) => {
            if !matches!(get(&r, "error"), JsValue::Undefined) {
                // provider.call threw before any head
                return transport_fail(stream, get(&r, "error"));
            }
            if decided == Some(true) {
                if matches!(get(&r, "streamError"), JsValue::Undefined) && !broken {
                    let _ = stream.write_all(b"0\r\n\r\n");
                } else {
                    // headers already sent: TS destroys the response mid-stream
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                }
            }
        }
        Err(e) => {
            if decided.is_none() {
                transport_fail(stream, &obj(vec![("message", s(&format!("{e:?}")))]));
            }
        }
    }
}
