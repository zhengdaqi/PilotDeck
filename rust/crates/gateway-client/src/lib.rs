//! Minimal blocking client for the PilotDeck Gateway (protocol 1.2):
//! `GET /health` probe → read `<pilotHome>/server-token` → WS `/ws` hello →
//! request/response. Mirrors `connectRemoteGatewayIfAvailable` and
//! `GatewayWsClient.request` for one-shot CLI commands.

use pilotdeck_session_search::paths;
use pilotdeck_skills::jsjson::{JsObject, JsValue};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

pub const PROTOCOL_VERSION: &str = "1.2";
pub const DEFAULT_URL_HOST: &str = "127.0.0.1:18789";

/// Convert a parsed JSON value into the JS object model (key order as JS would see it).
pub fn json_to_js(v: &Value) -> JsValue {
    match v {
        Value::Null => JsValue::Null,
        Value::Bool(b) => JsValue::Bool(*b),
        Value::Number(n) => JsValue::Num(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => JsValue::Str(s.clone()),
        Value::Array(a) => JsValue::Arr(a.iter().map(json_to_js).collect()),
        Value::Object(m) => {
            let mut o = JsObject::new();
            for (k, v) in m {
                o.set(k.clone(), json_to_js(v));
            }
            JsValue::Obj(o)
        }
    }
}

fn health_ok(host: &str, timeout: Duration) -> bool {
    let Some(addr) = host.to_socket_addrs().ok().and_then(|mut a| a.next()) else { return false };
    let deadline = Instant::now() + timeout;
    let Ok(mut s) = TcpStream::connect_timeout(&addr, timeout) else { return false };
    let left = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1));
    let _ = s.set_read_timeout(Some(left));
    let _ = s.set_write_timeout(Some(left));
    if s.write_all(format!("GET /health HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes()).is_err() {
        return false;
    }
    let mut buf = [0u8; 64];
    let mut got = Vec::new();
    while got.len() < 12 {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
        }
        if Instant::now() > deadline {
            break;
        }
    }
    // response.ok: status 200-299
    let line = String::from_utf8_lossy(&got);
    line.split(' ').nth(1).and_then(|c| c.get(..1)).is_some_and(|c| c == "2")
}

/// `readGatewayAuthToken()` using `PILOT_HOME`.
pub fn read_token() -> Option<String> {
    let path = paths::resolve(&[&paths::resolve_pilot_home_from_env(), "server-token"]);
    let raw = std::fs::read(path).ok()?;
    let t = pilotdeck_session_search::js::js_trim(&String::from_utf8_lossy(&raw)).to_string();
    (!t.is_empty()).then_some(t)
}

pub struct Gateway {
    ws: WebSocket<TcpStream>,
}

#[derive(Debug)]
pub enum RequestError {
    /// Server rejected: message as `GatewayRequestError.message`.
    Gateway(String),
    /// Transport failure (`Gateway WebSocket closed.` etc).
    Transport(String),
}

impl RequestError {
    pub fn message(&self) -> &str {
        match self {
            RequestError::Gateway(m) | RequestError::Transport(m) => m,
        }
    }
}

impl Gateway {
    /// `connectRemoteGatewayIfAvailable()`: None when no usable server.
    pub fn connect_if_available() -> Option<Gateway> {
        if !health_ok(DEFAULT_URL_HOST, Duration::from_millis(200)) {
            return None;
        }
        let token = read_token()?;
        let stream = TcpStream::connect(DEFAULT_URL_HOST).ok()?;
        let (mut ws, _) = tungstenite::client(format!("ws://{DEFAULT_URL_HOST}/ws"), stream).ok()?;
        let hello = serde_json::json!({
            "type": "hello",
            "protocolVersion": PROTOCOL_VERSION,
            "clientName": "cli",
            "clientVersion": "0.1.0",
            "token": token,
        });
        ws.send(Message::text(hello.to_string())).ok()?;
        let _ = ws.get_ref().set_read_timeout(Some(Duration::from_secs(10)));
        loop {
            match ws.read() {
                Ok(Message::Text(t)) => {
                    let v: Value = serde_json::from_str(&t).ok()?;
                    if v.get("type").and_then(|x| x.as_str()) == Some("hello_ok") {
                        let _ = ws.get_ref().set_read_timeout(None);
                        return Some(Gateway { ws });
                    }
                }
                Ok(Message::Close(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, RequestError> {
        let id = random_uuid_v4();
        let frame = serde_json::json!({ "type": "request", "id": id, "method": method, "params": params });
        self.ws
            .send(Message::text(frame.to_string()))
            .map_err(|_| RequestError::Transport("Gateway WebSocket is not connected.".into()))?;
        loop {
            let msg = self.ws.read().map_err(|_| RequestError::Transport("Gateway WebSocket closed.".into()))?;
            let Message::Text(t) = msg else {
                if matches!(msg, Message::Close(_)) {
                    return Err(RequestError::Transport("Gateway WebSocket closed.".into()));
                }
                continue;
            };
            let Ok(v) = serde_json::from_str::<Value>(&t) else { continue };
            if v.get("type").and_then(|x| x.as_str()) != Some("response") || v.get("id").and_then(|x| x.as_str()) != Some(&id) {
                continue;
            }
            if v.get("ok") == Some(&Value::Bool(true)) {
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
            let msg = v
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Gateway request failed.")
                .to_string();
            return Err(RequestError::Gateway(msg));
        }
    }

    pub fn close(mut self) {
        let _ = self.ws.close(None);
        let _ = self.ws.flush();
    }
}

/// RFC 4122 v4 UUID from the OS CSPRNG (equivalent of `crypto.randomUUID()`).
fn random_uuid_v4() -> String {
    let mut b = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut b);
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}
