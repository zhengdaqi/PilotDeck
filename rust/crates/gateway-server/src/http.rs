//! Port of `src/gateway/server/GatewayServer.ts` (+ `staticAssets.ts`, the unbound public
//! approval/host transports and the `/api/web` router's auth/projects routes).
use crate::backend::{GatewayBackend, GwError};
use crate::conn::GatewayWsConnection;
use crate::{obj, s, stringify_compact, JsValue};
use pilotdeck_session_search::paths;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, Weak};

pub struct ServerOptions {
    pub host: String,
    pub port: u16,
    pub allow_remote_host: bool,
    pub token: Option<String>,
    pub static_assets_path: Option<String>,
    pub server_version: Option<String>,
}

pub struct GatewayServer {
    pub url: String,
    pub ws_url: String,
    pub token: String,
    pub token_path: Option<String>,
    connections: Arc<Mutex<Vec<Weak<GatewayWsConnection>>>>,
}

impl GatewayServer {
    pub fn broadcast_notification(&self, name: &str, payload: JsValue) {
        for c in self.connections.lock().unwrap_or_else(|e| e.into_inner()).iter().filter_map(|w| w.upgrade()) {
            c.send_notification(name, payload.clone());
        }
    }
}

struct Ctx {
    gateway: Arc<dyn GatewayBackend>,
    token: String,
    loopback: bool,
    static_root: Option<String>,
    server_version: String,
    connections: Arc<Mutex<Vec<Weak<GatewayWsConnection>>>>,
}

/// `startGatewayServer`: binds, then serves on background threads.
pub fn start_gateway_server(options: ServerOptions, gateway: Arc<dyn GatewayBackend>) -> Result<GatewayServer, String> {
    let host = options.host.clone();
    let loopback = host == "127.0.0.1" || host == "localhost";
    if !loopback {
        if !options.allow_remote_host {
            return Err("GatewayServer remote binding requires allowRemoteHost=true.".into());
        }
        if options.token.as_deref().map_or(true, |t| t.trim().is_empty()) {
            return Err("GatewayServer remote binding requires an explicit auth token.".into());
        }
    }
    let (token, token_path) = match options.token.clone().filter(|t| !t.is_empty()) {
        Some(t) => (t, None),
        None => {
            let (t, p) = crate::token::ensure_gateway_auth_token().map_err(|e| e.to_string())?;
            (t, Some(p))
        }
    };
    let listener = TcpListener::bind((host.as_str(), options.port)).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let connections = Arc::new(Mutex::new(Vec::new()));
    let ctx = Arc::new(Ctx {
        gateway,
        token: token.clone(),
        loopback,
        static_root: options.static_assets_path.map(|p| paths::resolve(&[&p])),
        server_version: options.server_version.unwrap_or_else(|| "0.1.0".into()),
        connections: connections.clone(),
    });
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let ctx = ctx.clone();
            std::thread::spawn(move || handle_connection(stream, ctx));
        }
    });
    Ok(GatewayServer { url: format!("http://{host}:{port}"), ws_url: format!("ws://{host}:{port}/ws"), token, token_path, connections })
}

struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body_prefix: Vec<u8>,
}
impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        let mut headers = [httparse::EMPTY_HEADER; 128];
        let mut req = httparse::Request::new(&mut headers);
        match req.parse(&buf) {
            Ok(httparse::Status::Complete(len)) => {
                let trim = |b: &[u8]| String::from_utf8_lossy(b).trim_matches(|c| c == ' ' || c == '\t').to_string();
                return Some(Request {
                    method: req.method.unwrap_or("").to_string(),
                    target: req.path.unwrap_or("/").to_string(),
                    headers: req.headers.iter().map(|h| (h.name.to_string(), trim(h.value))).collect(),
                    body_prefix: buf[len..].to_vec(),
                });
            }
            Ok(httparse::Status::Partial) if buf.len() < 1 << 20 => continue,
            _ => {
                let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n");
                return None;
            }
        }
    }
}

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) {
    let reason = match status {
        200 => "OK", 400 => "Bad Request", 401 => "Unauthorized", 403 => "Forbidden", 404 => "Not Found",
        405 => "Method Not Allowed", 413 => "Payload Too Large", 500 => "Internal Server Error", 501 => "Not Implemented",
        502 => "Bad Gateway", 503 => "Service Unavailable", _ => "",
    };
    let head = format!("HTTP/1.1 {status} {reason}\r\ncontent-type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}
fn send_json(stream: &mut TcpStream, status: u16, body: JsValue) {
    respond(stream, status, "application/json", stringify_compact(&body).as_bytes());
}

fn handle_connection(mut stream: TcpStream, ctx: Arc<Ctx>) {
    let Some(req) = read_request(&mut stream) else { return };
    let is_upgrade = req.header("upgrade").is_some()
        && req.header("connection").map_or(false, |c| c.split(',').any(|t| t.trim().eq_ignore_ascii_case("upgrade")));
    if is_upgrade {
        return handle_upgrade(stream, req, ctx);
    }
    handle_http(&mut stream, &req, &ctx);
}

fn handle_http(stream: &mut TcpStream, req: &Request, ctx: &Ctx) {
    let pathname = url_pathname(&req.target);
    let auth = req.header("authorization");
    // handlePublicApprovalHttpRequest (no approval bridge bound)
    if pathname == "/api/module-host/approvals/status" || pathname == "/api/module-host/approvals/resume" {
        if ctx.token.is_empty() || auth != Some(format!("Bearer {}", ctx.token).as_str()) {
            return send_json(stream, 401, obj(vec![("code", s("PUBLIC_HOST_TRANSPORT_UNAUTHORIZED"))]));
        }
        if req.method != "POST" {
            return send_json(stream, 405, obj(vec![("code", s("PUBLIC_HOST_METHOD_NOT_ALLOWED"))]));
        }
        return send_json(stream, 503, obj(vec![("code", s("APPROVAL_BINDING_UNAVAILABLE"))]));
    }
    // handlePublicHostHttpRequest (no provider resolver bound)
    if pathname == "/api/module-host/describe" || pathname == "/api/module-host/call" {
        if auth != Some(format!("Bearer {}", ctx.token).as_str()) {
            return send_json(stream, 401, obj(vec![("code", s("PUBLIC_HOST_TRANSPORT_UNAUTHORIZED"))]));
        }
        if req.method != "POST" {
            return send_json(stream, 405, obj(vec![("code", s("PUBLIC_HOST_METHOD_NOT_ALLOWED"))]));
        }
        return send_json(stream, 501, obj(vec![("code", s("PUBLIC_HOST_PORT_UNAVAILABLE"))]));
    }
    if pathname == "/health" {
        return respond(stream, 200, "application/json", br#"{"ok":true}"#);
    }
    if pathname == "/auth/local-token" {
        if !ctx.loopback {
            return respond(stream, 404, "text/plain; charset=utf-8", b"not found");
        }
        return send_json(stream, 200, obj(vec![("token", s(&ctx.token))]));
    }
    if pathname.starts_with("/api/web/") {
        return handle_web_api(stream, req, ctx, &pathname);
    }
    if let Some(root) = &ctx.static_root {
        if serve_static_asset(root, &pathname, stream) {
            return;
        }
    }
    respond(stream, 404, "text/plain; charset=utf-8", b"not found");
}

/// `/^Bearer\s+(.+)$/i.exec(header)?.[1] === token`
fn web_authorized(header: Option<&str>, token: &str) -> bool {
    let h = header.unwrap_or("");
    if h.len() < 6 || !h.is_char_boundary(6) || !h[..6].eq_ignore_ascii_case("bearer") {
        return false;
    }
    let rest = &h[6..];
    let is_ws = |c: char| c.is_whitespace() && c != '\u{85}' || c == '\u{feff}';
    let is_lt = |c: char| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}');
    if !rest.chars().next().map_or(false, is_ws) || rest.chars().any(is_lt) {
        return false;
    }
    let trimmed = rest.trim_start_matches(is_ws);
    let captured: String = if !trimmed.is_empty() {
        trimmed.to_string()
    } else if rest.chars().count() >= 2 {
        rest.chars().last().map(String::from).unwrap_or_default()
    } else {
        return false;
    };
    captured == token
}

fn error_body(code: &str, e: &GwError) -> JsValue {
    let message = match e {
        GwError::SkillValidation { message, .. } | GwError::SkillManager { message, .. } | GwError::Structured { message, .. } | GwError::Plain { message } => message.clone(),
    };
    obj(vec![("error", obj(vec![("code", s(code)), ("message", s(&message))]))])
}

fn handle_web_api(stream: &mut TcpStream, req: &Request, ctx: &Ctx, pathname: &str) {
    if !web_authorized(req.header("authorization"), &ctx.token) {
        return send_json(stream, 401, obj(vec![("error", obj(vec![("code", s("unauthorized")), ("message", s("missing or invalid token"))]))]));
    }
    // PROJECT_RE = /^\/api\/web\/projects(?:\/([^/]+)(\/.*)?)?$/
    let m: Option<(Option<String>, Option<String>)> = pathname.strip_prefix("/api/web/projects").and_then(|rest| {
        if rest.is_empty() {
            return Some((None, None));
        }
        let rest = rest.strip_prefix('/')?;
        let (key, sub) = match rest.find('/') {
            Some(i) => (&rest[..i], Some(rest[i..].to_string())),
            None => (rest, None),
        };
        if key.is_empty() { None } else { Some((Some(key.to_string()), sub)) }
    });
    if pathname == "/api/web/projects" && req.method == "GET" {
        match ctx.gateway.call("listProjects", None) {
            Ok(r) => return send_json(stream, 200, r),
            // TS: unhandled rejection in the HTTP handler (process-level failure, SRC-DEFECT-005); drop the request.
            Err(_) => return,
        }
    }
    if let Some((Some(key), sub)) = &m {
        if (sub.is_none() || sub.as_deref() == Some("/")) && req.method == "GET" {
            // decodeURIComponent runs inside the try: a URIError becomes a 404 like a gateway failure.
            let r = match decode_uri_component(key) {
                Some(project_key) => ctx.gateway.call("describeProject", Some(obj(vec![("projectKey", s(&project_key))]))),
                None => Err(GwError::Plain { message: "URI malformed".into() }),
            };
            match r {
                Ok(r) => send_json(stream, 200, r),
                Err(e) => send_json(stream, 404, error_body("project_not_found", &e)),
            }
            return;
        }
        if let Some(sub) = sub {
            // TS decodes before its try block: a URIError is an unhandled rejection (SRC-DEFECT-005); drop the request.
            let Some(_project_key) = decode_uri_component(key) else { return };
            let pending = (sub == "/files/tree" && req.method == "GET") || (sub == "/files/read" && req.method == "GET")
                || (sub == "/files/write" && req.method == "POST");
            if pending {
                // PENDING (not dropped): ProjectFileService (src/adapters/web/projectFiles.ts) is not ported yet.
                return send_json(stream, 501, obj(vec![("error", obj(vec![("code", s("RUST_GATEWAY_PENDING")),
                    ("message", s("Rust gateway server: /api/web project files routes are not ported yet (pending)."))]))]));
            }
        }
    }
    send_json(stream, 404, obj(vec![("error", obj(vec![("code", s("not_found")), ("message", s(&format!("No /api/web route for {pathname}")))]))]));
}

/// `decodeURIComponent`; `None` is the URIError throw.
pub fn decode_uri_component(v: &str) -> Option<String> {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = |c: u8| (c as char).to_digit(16);
            let (h, l) = (hex(*b.get(i + 1)?)?, hex(*b.get(i + 2)?)?);
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn serve_static_asset(root: &str, pathname: &str, stream: &mut TcpStream) -> bool {
    let p = if pathname == "/" { "/index.html" } else { pathname };
    let mut n = paths::normalize(p);
    while n.starts_with("../") || n == ".." || n.starts_with("..\\") {
        n = if n == ".." { String::new() } else { n[3..].to_string() };
    }
    let candidate = paths::resolve(&[&paths::join(&[root, &n])]);
    if candidate != root && !candidate.starts_with(&format!("{root}{}", std::path::MAIN_SEPARATOR)) {
        return false;
    }
    let is_file = |p: &str| std::fs::metadata(p).map(|m| m.is_file()).unwrap_or(false);
    let file = if is_file(&candidate) { candidate } else { paths::resolve(&[root, "index.html"]) };
    if !is_file(&file) {
        return false;
    }
    let ext = match file.rfind('.') {
        Some(i) => &file[i..],
        None => &file[file.len().saturating_sub(1)..],
    };
    let ct = match ext {
        ".html" => "text/html; charset=utf-8",
        ".js" => "text/javascript; charset=utf-8",
        ".css" => "text/css; charset=utf-8",
        ".json" => "application/json; charset=utf-8",
        ".svg" => "image/svg+xml",
        _ => "application/octet-stream",
    };
    match std::fs::read(&file) {
        Ok(body) => respond(stream, 200, ct, &body),
        Err(_) => respond(stream, 200, ct, b""),
    }
    true
}

fn handle_upgrade(mut stream: TcpStream, req: Request, ctx: Arc<Ctx>) {
    if url_pathname(&req.target) != "/ws" {
        return;
    }
    let Some(key) = req.header("sec-websocket-key").map(String::from) else { return };
    let resp = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        crate::ws::create_websocket_accept_value(&key));
    if stream.write_all(resp.as_bytes()).is_err() {
        return;
    }
    // TS ignores the upgrade `head` bytes: anything after the request head is dropped.
    let _ = req.body_prefix;
    let Some((sender, reader)) = crate::ws::split(stream) else { return };
    let conn = GatewayWsConnection::new(sender.clone(), ctx.gateway.clone(), ctx.token.clone(), ctx.server_version.clone());
    ctx.connections.lock().unwrap_or_else(|e| e.into_inner()).push(Arc::downgrade(&conn));
    crate::ws::serve(&sender, reader, Vec::new(), |message| conn.handle_message(message));
    conn.handle_close();
    ctx.connections.lock().unwrap_or_else(|e| e.into_inner()).retain(|w| w.strong_count() > 0 && !std::ptr::eq(w.as_ptr(), Arc::as_ptr(&conn)));
}

/// `new URL(target, "http://127.0.0.1").pathname` for request targets (WHATWG path parsing:
/// backslashes, dot segments incl. `%2e`, percent-encoding of the path set).
pub fn url_pathname(target: &str) -> String {
    let t: String = target.trim_matches(|c: char| c <= ' ').chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
    let lower = t.to_ascii_lowercase();
    let mut path: &str = &t;
    for scheme in ["http:", "https:", "ws:", "wss:"] {
        if lower.starts_with(scheme) {
            let rest = &t[scheme.len()..];
            let rest = rest.trim_start_matches(['/', '\\']);
            let end = rest.find(['/', '\\', '?', '#']).unwrap_or(rest.len());
            path = &rest[end..];
            break;
        }
    }
    if path.starts_with("//") || path.starts_with("\\\\") || path.starts_with("/\\") || path.starts_with("\\/") {
        let rest = &path[2..];
        let end = rest.find(['/', '\\', '?', '#']).unwrap_or(rest.len());
        path = &rest[end..];
    }
    let end = path.find(['?', '#']).unwrap_or(path.len());
    let path = &path[..end];
    let mut segs: Vec<String> = Vec::new();
    let body = path.strip_prefix(['/', '\\']).unwrap_or(path);
    let parts: Vec<&str> = body.split(['/', '\\']).collect();
    let last = parts.len() - 1;
    for (i, seg) in parts.iter().enumerate() {
        let l = seg.to_ascii_lowercase();
        let single = l == "." || l == "%2e";
        let double = matches!(l.as_str(), ".." | ".%2e" | "%2e." | "%2e%2e");
        if double {
            segs.pop();
            if i == last {
                segs.push(String::new());
            }
        } else if single {
            if i == last {
                segs.push(String::new());
            }
        } else {
            segs.push(encode_path_segment(seg));
        }
    }
    format!("/{}", segs.join("/"))
}

fn encode_path_segment(seg: &str) -> String {
    let mut out = String::new();
    for ch in seg.chars() {
        let c = ch as u32;
        if c < 0x20 || c == 0x7f || c > 0x7e || matches!(ch, ' ' | '"' | '#' | '<' | '>' | '?' | '`' | '{' | '}') {
            let mut b = [0u8; 4];
            for byte in ch.encode_utf8(&mut b).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        } else {
            out.push(ch);
        }
    }
    out
}

