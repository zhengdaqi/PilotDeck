//! Port of `packages/sdk/src/hook-server.ts` (`HostedHookServer`, `isLoopbackHttpUrl`). It is a blocking HTTP/1.1
//! server, one thread per connection, so callbacks run on their own threads.
//!
//! Callbacks are Rust closures, not JS functions. The constructor's "hooks arrays of functions" check is therefore
//! enforced by the type system. All other validation, routing, merging and error text match TS.
//!
//! Known difference (pending, recorded in the contract): when the HTTP client disconnects in the middle of a call,
//! TS aborts the callback's signal. Rust only notices the disconnect when it writes the response; `close()` still
//! aborts every call.
use crate::js::{number, stringify};
use crate::transport_core::{js_number_to_string, js_order, js_string, truthy, SdkError};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const NATIVE_HOOK_EVENTS: [&str; 21] = [
    "PreToolUse", "PostToolUse", "PostToolUseFailure", "UserPromptSubmit", "PreModelRequest", "SessionStart", "SessionEnd", "Stop", "StopFailure",
    "SubagentStart", "SubagentStop", "PreCompact", "PostCompact", "PermissionRequest", "PermissionDenied", "Setup", "ConfigChange",
    "InstructionsLoaded", "FileChanged", "Elicitation", "ElicitationResult",
];

fn unsupported_message(event: &str) -> Option<&'static str> {
    Some(match event {
        "Notification" => "PilotDeck has no user-notification lifecycle event; Gateway notifications are infrastructure frames, not hooks.",
        "CwdChanged" => "PilotDeck sessions have a fixed working directory; in-session cwd changes have no native lifecycle event.",
        "WorktreeCreate" | "WorktreeRemove" => "PilotDeck only emits this from the Always-On workspace runtime; it is not attached to a per-query Gateway session.",
        _ => return None,
    })
}

/// The third callback argument: `{ signal, asyncHookId }`.
pub struct HookContext {
    pub signal: Arc<AtomicBool>,
    pub async_hook_id: String,
}

/// What a callback can throw.
#[derive(Debug, Clone)]
pub enum HookError {
    /// A `PilotDeckError` (its code reaches the HTTP response).
    Sdk(SdkError),
    /// Any other `Error` (message only).
    Error(String),
    /// A thrown non-Error value.
    Other,
}

/// `(input, toolUseId, { signal, asyncHookId }) => output | undefined`.
pub type HookCallback = Arc<dyn Fn(&Value, Option<&str>, &HookContext) -> Result<Option<Value>, HookError> + Send + Sync>;

#[derive(Clone)]
pub struct HookCallbackMatcher {
    pub matcher: Option<String>,
    /// Seconds.
    pub timeout: Option<f64>,
    pub hooks: Vec<HookCallback>,
}

#[derive(Clone, Debug, Default)]
pub struct HookServerStartOptions {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub path: Option<String>,
    pub public_url: Option<String>,
}

fn sdk(code: &str, message: impl Into<String>) -> SdkError {
    SdkError::new(code, message, false)
}

struct Running {
    stop: Arc<AtomicBool>,
    conns: Arc<Mutex<Vec<TcpStream>>>,
    accept: Option<std::thread::JoinHandle<()>>,
}

pub struct HostedHookServer {
    hooks: Arc<Vec<(String, Vec<HookCallbackMatcher>)>>,
    running: Mutex<Option<(Running, Value)>>,
    active: Arc<Mutex<Vec<Arc<AtomicBool>>>>,
}

impl HostedHookServer {
    /// `new HostedHookServer(hooks)`; the event order is the object's key order.
    pub fn new(hooks: Vec<(String, Vec<HookCallbackMatcher>)>) -> Result<Self, SdkError> {
        for (event, _) in &hooks {
            if let Some(m) = unsupported_message(event) {
                return Err(sdk("unsupported_capability", format!("hooks.{event} is unsupported: {m}")));
            }
            if !NATIVE_HOOK_EVENTS.contains(&event.as_str()) {
                return Err(sdk("unsupported_capability", format!("hooks.{event} has no native PilotDeck lifecycle event.")));
            }
        }
        Ok(HostedHookServer { hooks: Arc::new(hooks), running: Mutex::new(None), active: Arc::default() })
    }

    /// `start(options)`; a started server returns its existing config.
    pub fn start(&self, options: &HookServerStartOptions) -> Result<Value, SdkError> {
        let mut running = self.running.lock().unwrap();
        if let Some((_, cfg)) = &*running {
            return Ok(cfg.clone());
        }
        let path = options.path.clone().unwrap_or_else(|| "/pilotdeck-sdk-hooks".into());
        if !path.starts_with('/') {
            return Err(sdk("validation_error", "Hook endpoint path must begin with '/'."));
        }
        let token = crate::net::random_uuid();
        let host = options.host.clone().unwrap_or_else(|| "127.0.0.1".into());
        let port = options.port.unwrap_or(0);
        let bind = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
        let listener = TcpListener::bind(&bind).map_err(|e| {
            let what = match e.kind() {
                std::io::ErrorKind::AddrInUse => "EADDRINUSE: address already in use",
                std::io::ErrorKind::AddrNotAvailable => "EADDRNOTAVAIL: address not available",
                std::io::ErrorKind::PermissionDenied => "EACCES: permission denied",
                _ => "ENOTFOUND",
            };
            sdk("server_error", format!("Unable to start SDK hook server: listen {what} {host}:{port}"))
        })?;
        let addr = listener.local_addr().map_err(|_| sdk("server_error", "SDK hook server did not expose a TCP address."))?;
        let ip = addr.ip().to_string();
        let local_url = format!("http://{}:{}{path}", if ip.contains(':') { format!("[{ip}]") } else { ip }, addr.port());
        let url = match &options.public_url {
            Some(p) => normalize_public_endpoint(p)?,
            None => local_url,
        };
        let mut events = Map::new();
        for (event, matchers) in self.hooks.iter() {
            let list = matchers
                .iter()
                .map(|m| {
                    let mut o = Map::new();
                    if let Some(x) = m.matcher.as_ref().filter(|x| !x.is_empty()) {
                        o.insert("matcher".into(), Value::String(x.clone()));
                    }
                    if let Some(t) = m.timeout {
                        o.insert("timeout".into(), serde_json::Number::from_f64(t).map_or(Value::Null, Value::Number));
                    }
                    Value::Object(o)
                })
                .collect();
            events.insert(event.clone(), Value::Array(list));
        }
        let mut headers = Map::new();
        headers.insert("authorization".into(), Value::String(format!("Bearer {token}")));
        let mut cfg = Map::new();
        cfg.insert("url".into(), Value::String(url));
        cfg.insert("headers".into(), Value::Object(headers));
        cfg.insert("events".into(), js_order(Value::Object(events)));
        let cfg = Value::Object(cfg);

        let stop = Arc::new(AtomicBool::new(false));
        let conns: Arc<Mutex<Vec<TcpStream>>> = Arc::default();
        let _ = listener.set_nonblocking(true);
        let ctx = Arc::new(Route { hooks: self.hooks.clone(), path, token, active: self.active.clone() });
        let (stop2, conns2) = (stop.clone(), conns.clone());
        let accept = std::thread::spawn(move || {
            while !stop2.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((s, _)) => {
                        let _ = s.set_nonblocking(false);
                        if let Ok(c) = s.try_clone() {
                            conns2.lock().unwrap().push(c);
                        }
                        let ctx = ctx.clone();
                        std::thread::spawn(move || serve_connection(s, ctx));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        *running = Some((Running { stop, conns, accept: Some(accept) }, cfg.clone()));
        Ok(cfg)
    }

    /// `close()`: aborts active calls, drops all connections and stops listening.
    pub fn close(&self) {
        let r = self.running.lock().unwrap().take();
        for a in self.active.lock().unwrap().drain(..) {
            a.store(true, Ordering::SeqCst);
        }
        if let Some((mut r, _)) = r {
            r.stop.store(true, Ordering::SeqCst);
            for c in r.conns.lock().unwrap().drain(..) {
                let _ = c.shutdown(Shutdown::Both);
            }
            if let Some(h) = r.accept.take() {
                let _ = h.join();
            }
        }
    }
}

impl Drop for HostedHookServer {
    fn drop(&mut self) {
        self.close();
    }
}

struct Route {
    hooks: Arc<Vec<(String, Vec<HookCallbackMatcher>)>>,
    path: String,
    token: String,
    active: Arc<Mutex<Vec<Arc<AtomicBool>>>>,
}

struct Request {
    method: String,
    target: String,
    headers: HashMap<String, String>,
    keep_alive: bool,
}

enum Body {
    Ok(Vec<u8>),
    TooLarge,
    Bad,
}

fn read_request(r: &mut BufReader<TcpStream>) -> Option<Result<Request, ()>> {
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        if !line.trim().is_empty() {
            break;
        }
    }
    let parts: Vec<&str> = line.trim_end_matches(['\r', '\n']).split(' ').collect();
    if parts.len() != 3 || !parts[2].starts_with("HTTP/1.") || parts[0].is_empty() || parts[1].is_empty() {
        return Some(Err(()));
    }
    let mut headers = HashMap::new();
    let mut total = line.len();
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).ok()? == 0 {
            return None;
        }
        total += h.len();
        if total > 16 * 1024 {
            return Some(Err(()));
        }
        let h = h.trim_end_matches(['\r', '\n']);
        if h.is_empty() {
            break;
        }
        let Some((k, v)) = h.split_once(':') else { return Some(Err(())) };
        let k = k.to_ascii_lowercase();
        if k.is_empty() || k.contains([' ', '\t']) {
            return Some(Err(()));
        }
        // Node keeps the first value of a duplicated authorization header.
        headers.entry(k).or_insert_with(|| v.trim_matches([' ', '\t']).to_string());
    }
    let keep_alive = match headers.get("connection").map(|c| c.to_ascii_lowercase()) {
        Some(c) if c.contains("close") => false,
        Some(c) if c.contains("keep-alive") => true,
        _ => parts[2] == "HTTP/1.1",
    };
    Some(Ok(Request { method: parts[0].into(), target: parts[1].into(), headers, keep_alive }))
}

fn read_body(r: &mut BufReader<TcpStream>, req: &Request) -> Body {
    const LIMIT: usize = 1_000_000;
    let chunked = req.headers.get("transfer-encoding").is_some_and(|t| t.to_ascii_lowercase().contains("chunked"));
    let mut body = vec![];
    if chunked {
        loop {
            let mut size = String::new();
            if r.read_line(&mut size).unwrap_or(0) == 0 {
                return Body::Bad;
            }
            let Ok(n) = usize::from_str_radix(size.trim().split(';').next().unwrap_or("").trim(), 16) else { return Body::Bad };
            if n == 0 {
                let mut t = String::new();
                while r.read_line(&mut t).unwrap_or(0) > 0 && t.trim_end_matches(['\r', '\n']).len() > 0 {
                    t.clear();
                }
                return Body::Ok(body);
            }
            let mut chunk = vec![0; n];
            if r.read_exact(&mut chunk).is_err() {
                return Body::Bad;
            }
            body.extend(chunk);
            if body.len() > LIMIT {
                return Body::TooLarge;
            }
            let mut crlf = String::new();
            let _ = r.read_line(&mut crlf);
        }
    }
    let Some(len) = req.headers.get("content-length") else { return Body::Ok(body) };
    let Ok(len) = len.parse::<usize>() else { return Body::Bad };
    let mut left = len;
    let mut buf = [0u8; 65536];
    while left > 0 {
        let n = match r.read(&mut buf[..left.min(65536)]) {
            Ok(0) | Err(_) => return Body::Bad,
            Ok(n) => n,
        };
        body.extend_from_slice(&buf[..n]);
        left -= n;
        if body.len() > LIMIT {
            return Body::TooLarge;
        }
    }
    Body::Ok(body)
}

fn respond(s: &mut TcpStream, status: u16, body: &str, keep_alive: bool) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    let conn = if keep_alive { "keep-alive" } else { "close" };
    write!(s, "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\nContent-Length: {}\r\nConnection: {conn}\r\n\r\n{body}", body.len())?;
    s.flush()
}

fn error_json(message: &str, code: Option<&str>) -> String {
    let mut m = Map::new();
    m.insert("error".into(), Value::String(message.into()));
    if let Some(c) = code {
        m.insert("code".into(), Value::String(c.into()));
    }
    stringify(&Value::Object(m))
}

fn serve_connection(stream: TcpStream, ctx: Arc<Route>) {
    let Ok(mut w) = stream.try_clone() else { return };
    let mut r = BufReader::new(stream);
    loop {
        let req = match read_request(&mut r) {
            None => return,
            Some(Err(())) => {
                let _ = w.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n");
                let _ = w.shutdown(Shutdown::Both);
                return;
            }
            Some(Ok(req)) => req,
        };
        let body = read_body(&mut r, &req);
        if matches!(body, Body::Bad) {
            let _ = w.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n");
            let _ = w.shutdown(Shutdown::Both);
            return;
        }
        let (status, text) = match handle(&ctx, &req, body) {
            Some(x) => x,
            None => {
                // Payload too large: TS destroys the request socket without answering.
                let _ = w.shutdown(Shutdown::Both);
                return;
            }
        };
        if respond(&mut w, status, &text, req.keep_alive).is_err() || !req.keep_alive {
            let _ = w.shutdown(Shutdown::Both);
            return;
        }
    }
}

/// `new URL(request.url, "http://localhost")`.
fn request_url(target: &str) -> Option<url::Url> {
    url::Url::parse("http://localhost").ok()?.join(target).ok()
}

fn handle(ctx: &Route, req: &Request, body: Body) -> Option<(u16, String)> {
    let url = request_url(&req.target);
    if req.method != "POST" || url.as_ref().map(|u| u.path()) != Some(ctx.path.as_str()) {
        return Some((404, error_json("PilotDeck SDK hook endpoint not found.", None)));
    }
    let url = url.unwrap();
    if req.headers.get("authorization").map(String::as_str) != Some(&format!("Bearer {}", ctx.token)) {
        return Some((401, error_json("Unauthorized PilotDeck SDK hook request.", None)));
    }
    let get = |name: &str| url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned());
    let event = get("event").filter(|e| !e.is_empty());
    let idx = match get("matcher") {
        Some(m) => number(&m),
        None => 0.0, // Number(null)
    };
    let matcher = event.as_ref().and_then(|e| {
        let ms = &ctx.hooks.iter().find(|(k, _)| k == e)?.1;
        if idx.is_finite() && idx.fract() == 0.0 && idx >= 0.0 {
            ms.get(idx as usize)
        } else {
            None
        }
    });
    let (Some(event), Some(matcher)) = (event, matcher) else {
        return Some((400, error_json("Invalid SDK hook endpoint selector.", None)));
    };
    let signal = Arc::new(AtomicBool::new(false));
    ctx.active.lock().unwrap().push(signal.clone());
    let result = (|| -> Result<Value, HookError> {
        let bytes = match body {
            Body::Ok(b) => b,
            Body::TooLarge => return Err(HookError::Sdk(sdk("validation_error", "SDK hook payload exceeds 1MB."))),
            Body::Bad => unreachable!(),
        };
        let parsed: Value = serde_json::from_str(&String::from_utf8_lossy(&bytes))
            .map_err(|_| HookError::Sdk(sdk("validation_error", "SDK hook payload is not valid JSON.")))?;
        let input = to_sdk_hook_input(js_order(parsed)).map_err(HookError::Sdk)?;
        let async_hook_id = crate::net::random_uuid();
        let out = run_matcher(matcher, &input, &signal, &async_hook_id)?;
        if out.get("async") == Some(&Value::Bool(true)) {
            let mut m = Map::new();
            m.insert("async".into(), Value::Bool(true));
            m.insert("asyncHookId".into(), Value::String(async_hook_id));
            if let Some(t) = out.get("asyncTimeout") {
                m.insert("asyncTimeout".into(), t.clone());
            }
            Ok(Value::Object(m))
        } else {
            to_native_hook_output(out, &event).map_err(HookError::Sdk)
        }
    })();
    ctx.active.lock().unwrap().retain(|a| !Arc::ptr_eq(a, &signal));
    Some(match result {
        Ok(v) => (200, stringify(&v)),
        Err(HookError::Sdk(e)) if e.message == "SDK hook payload exceeds 1MB." => return None,
        Err(HookError::Sdk(e)) => (500, error_json(&e.message, Some(&e.code))),
        Err(HookError::Error(m)) => (500, error_json(&m, None)),
        Err(HookError::Other) => (500, error_json("SDK hook callback failed.", None)),
    })
}

fn camel_to_snake(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            o.push('_');
            o.push(c.to_ascii_lowercase());
        } else {
            o.push(c);
        }
    }
    o
}

fn to_sdk_hook_input(v: Value) -> Result<Value, SdkError> {
    let Value::Object(m) = v else { return Err(sdk("validation_error", "SDK hook payload must be an object.")) };
    let mut input = Map::new();
    for (k, x) in m {
        input.insert(camel_to_snake(&k), x);
    }
    let input = js_order(Value::Object(input));
    let ev = input.get("hook_event_name").and_then(Value::as_str);
    if !ev.is_some_and(|e| NATIVE_HOOK_EVENTS.contains(&e)) {
        return Err(sdk("validation_error", "SDK hook payload has no supported native hook_event_name."));
    }
    if !["session_id", "transcript_path", "cwd"].iter().all(|k| input.get(*k).is_some_and(Value::is_string)) {
        return Err(sdk("validation_error", "SDK hook payload is missing session_id, transcript_path, or cwd."));
    }
    Ok(input)
}

/// `{ ...v }` entries for any JS value.
fn spread(v: &Value) -> Vec<(String, Value)> {
    match v {
        Value::Object(m) => m.iter().map(|(k, x)| (k.clone(), x.clone())).collect(),
        Value::Array(a) => a.iter().enumerate().map(|(i, x)| (i.to_string(), x.clone())).collect(),
        Value::String(s) => s.encode_utf16().enumerate().map(|(i, u)| (i.to_string(), Value::String(String::from_utf16_lossy(&[u])))).collect(),
        _ => vec![],
    }
}

fn set(m: &mut Map<String, Value>, k: &str, v: Value) {
    m.insert(k.into(), v);
}

fn merge_output(previous: &Map<String, Value>, next: &Value) -> Map<String, Value> {
    let prev_specific = previous.get("hookSpecificOutput").filter(|v| truthy(Some(v)));
    let next_specific = next.get("hookSpecificOutput").filter(|v| truthy(Some(v)));
    let specific = match (prev_specific, next_specific) {
        (Some(p), Some(n)) => {
            let mut m = Map::new();
            for (k, v) in spread(p).into_iter().chain(spread(n)) {
                m.insert(k, v);
            }
            Some(js_order(Value::Object(m)))
        }
        (Some(p), None) => Some(p.clone()),
        (None, n) => n.cloned(),
    };
    let mut out = previous.clone();
    for (k, v) in spread(next) {
        out.insert(k, v);
    }
    if previous.get("continue") == Some(&Value::Bool(false)) || next.get("continue") == Some(&Value::Bool(false)) {
        set(&mut out, "continue", Value::Bool(false));
    }
    if previous.get("decision").and_then(Value::as_str) == Some("block") || next.get("decision").and_then(Value::as_str) == Some("block") {
        set(&mut out, "decision", Value::String("block".into()));
    }
    if let (Some(a), Some(b)) = (previous.get("systemMessage").filter(|v| truthy(Some(v))), next.get("systemMessage").filter(|v| truthy(Some(v)))) {
        set(&mut out, "systemMessage", Value::String(format!("{}\n{}", js_string(a), js_string(b))));
    }
    if let Some(s) = specific {
        set(&mut out, "hookSpecificOutput", s);
    }
    match js_order(Value::Object(out)) {
        Value::Object(m) => m,
        _ => unreachable!(),
    }
}

const MIXED: &str = "A hook matcher cannot combine an async output with synchronous hook effects.";

fn run_matcher(matcher: &HookCallbackMatcher, input: &Value, signal: &Arc<AtomicBool>, async_hook_id: &str) -> Result<Value, HookError> {
    let mut merged = Map::new();
    let mut deferred: Option<Value> = None;
    let tool_use_id = input.get("tool_use_id").and_then(Value::as_str).or_else(|| input.get("tool_call_id").and_then(Value::as_str)).map(String::from);
    for cb in &matcher.hooks {
        let output = invoke_callback(cb.clone(), input, tool_use_id.as_deref(), matcher.timeout, signal, async_hook_id)?;
        let Some(output) = output.filter(|o| truthy(Some(o))) else { continue };
        let output = js_order(output);
        if output.get("async") == Some(&Value::Bool(true)) {
            if deferred.is_some() || !merged.is_empty() {
                return Err(HookError::Sdk(sdk("validation_error", MIXED)));
            }
            if let Some(t) = output.get("asyncTimeout") {
                if !t.as_f64().is_some_and(|t| t.is_finite() && t > 0.0) {
                    return Err(HookError::Sdk(sdk("validation_error", "asyncTimeout must be a positive finite number of seconds.")));
                }
            }
            deferred = Some(output);
            continue;
        }
        if deferred.is_some() {
            return Err(HookError::Sdk(sdk("validation_error", MIXED)));
        }
        merged = merge_output(&merged, &output);
    }
    Ok(deferred.unwrap_or(Value::Object(merged)))
}

fn to_native_hook_output(output: Value, event: &str) -> Result<Value, SdkError> {
    let Some(specific) = output.get("hookSpecificOutput").filter(|v| truthy(Some(v))).cloned() else { return Ok(output) };
    let name = specific.get("hookEventName");
    if name.and_then(Value::as_str) != Some(event) {
        let shown = name.map_or("undefined".to_string(), js_string);
        return Err(sdk("validation_error", format!("Hook {event} returned hookSpecificOutput for {shown}.")));
    }
    let mut s = Map::new();
    for (k, v) in spread(&specific) {
        s.insert(k, v);
    }
    if s.get("permissionDecision").and_then(Value::as_str) == Some("defer") {
        s.insert("permissionDecision".into(), Value::String("passthrough".into()));
    }
    let Value::Object(mut out) = output else { unreachable!() };
    out.insert("hookSpecificOutput".into(), js_order(Value::Object(s)));
    Ok(Value::Object(out))
}

fn invoke_callback(cb: HookCallback, input: &Value, tool_use_id: Option<&str>, seconds: Option<f64>, parent: &Arc<AtomicBool>, async_hook_id: &str) -> Result<Option<Value>, HookError> {
    let aborted = || HookError::Sdk(sdk("transport_error", "SDK hook callback was aborted."));
    if parent.load(Ordering::SeqCst) {
        return Err(aborted());
    }
    if let Some(s) = seconds {
        if !s.is_finite() || s <= 0.0 {
            return Err(HookError::Sdk(sdk("validation_error", "Hook matcher timeout must be a positive finite number of seconds.")));
        }
    }
    let child = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    let (input, tid, ctx) = (input.clone(), tool_use_id.map(String::from), HookContext { signal: child.clone(), async_hook_id: async_hook_id.into() });
    std::thread::spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cb(&input, tid.as_deref(), &ctx))).unwrap_or(Err(HookError::Other));
        let _ = tx.send(r);
    });
    let deadline = seconds.map(|s| Instant::now() + Duration::from_secs_f64(s));
    loop {
        if parent.load(Ordering::SeqCst) {
            child.store(true, Ordering::SeqCst);
            if seconds.is_some() {
                return Err(aborted());
            }
        }
        match rx.recv_timeout(Duration::from_millis(5)) {
            Ok(r) => return r,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Err(HookError::Other),
            Err(_) => {}
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            child.store(true, Ordering::SeqCst);
            return Err(HookError::Sdk(sdk("timeout", format!("SDK hook callback timed out after {}s.", js_number_to_string(seconds.unwrap())))));
        }
    }
}

fn normalize_public_endpoint(value: &str) -> Result<String, SdkError> {
    let u = url::Url::parse(value).map_err(|_| sdk("validation_error", "Hook publicUrl must be an absolute HTTP(S) endpoint URL."))?;
    if u.scheme() != "http" && u.scheme() != "https" {
        return Err(sdk("validation_error", "Hook publicUrl must use http: or https:."));
    }
    Ok(u.to_string())
}

/// `isLoopbackHttpUrl(value)`.
pub fn is_loopback_http_url(value: &str) -> bool {
    match url::Url::parse(value) {
        Ok(u) => matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "::1" | "[::1]")),
        Err(_) => false,
    }
}
