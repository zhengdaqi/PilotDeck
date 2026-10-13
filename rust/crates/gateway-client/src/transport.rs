//! Port of `packages/sdk/src/transport.ts#GatewayTransport` (request/response subset, blocking).
//! Error messages are the SDK's `PilotDeckError.message` values.

use serde_json::Value;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::{Message, WebSocket};

pub struct Options<'a> {
    pub url: &'a str,
    pub token: &'a str,
    pub client_name: &'a str,
    pub client_version: &'a str,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
}

pub struct Transport {
    ws: WebSocket<TcpStream>,
    request_timeout: Duration,
}

/// WHATWG-ish URL checks performed by Node's `new WebSocket(url)`; returns (host:port, ws-url).
fn parse_ws_url(url: &str) -> Result<(String, String), String> {
    let invalid = || "TypeError: Invalid URL".to_string();
    let Some(colon) = url.find(':') else { return Err(invalid()) };
    let scheme = &url[..colon];
    let ok_scheme = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    if !ok_scheme {
        return Err(invalid());
    }
    let scheme = scheme.to_ascii_lowercase();
    let rest = &url[colon + 1..];
    let (ws_scheme, default_port) = match scheme.as_str() {
        "ws" | "http" => ("ws", 80),
        "wss" | "https" => ("wss", 443),
        other => {
            if !rest.starts_with("//") && !["file"].contains(&other) && rest.is_empty() {
                return Err(invalid());
            }
            return Err(format!("Expected a ws: or wss: protocol, got {other}:"));
        }
    };
    if url.contains('#') {
        return Err("Got fragment".into());
    }
    let after = rest.trim_start_matches(['/', '\\']);
    let end = after.find(['/', '?', '\\']).unwrap_or(after.len());
    let authority = &after[..end];
    let hostport = authority.rsplit('@').next().unwrap_or("");
    if hostport.is_empty() {
        return Err(invalid());
    }
    let has_port = hostport.rsplit_once(':').is_some_and(|(_, p)| !p.contains(']'));
    let hp = if has_port { hostport.to_string() } else { format!("{hostport}:{default_port}") };
    if ws_scheme == "wss" {
        // TLS is not linked into this build yet (tracked gap in CAP-CLI-SDK contract).
        return Err("Gateway connection failed.".into());
    }
    let path = &after[end..];
    Ok((hp.clone(), format!("ws://{hp}{}", if path.is_empty() { "/" } else { path })))
}

fn close_error(frame: Option<tungstenite::protocol::CloseFrame>) -> String {
    match frame {
        Some(f) => {
            let reason = f.reason.to_string();
            if !reason.is_empty() {
                reason
            } else {
                let _ = f.code == CloseCode::Library(4003);
                "Gateway WebSocket closed.".into()
            }
        }
        None => "Gateway WebSocket closed.".into(),
    }
}

impl Transport {
    pub fn connect(o: &Options) -> Result<Transport, String> {
        let (hostport, ws_url) = parse_ws_url(o.url)?;
        let deadline = Instant::now() + o.connect_timeout;
        let addrs: Vec<_> = hostport.to_socket_addrs().map_err(|_| "Gateway connection failed.".to_string())?.collect();
        let mut stream = None;
        for a in addrs {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("Gateway connection timed out.".into());
            }
            match TcpStream::connect_timeout(&a, left) {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => return Err("Gateway connection timed out.".into()),
                Err(_) => {}
            }
        }
        let stream = stream.ok_or_else(|| "Gateway connection failed.".to_string())?;
        let left = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1));
        let _ = stream.set_read_timeout(Some(left));
        let (mut ws, _) = tungstenite::client(ws_url, stream).map_err(|e| match e {
            tungstenite::HandshakeError::Failure(tungstenite::Error::Io(ref io))
                if matches!(io.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) =>
            {
                "Gateway connection timed out.".to_string()
            }
            _ => "Gateway connection failed.".to_string(),
        })?;
        let hello = serde_json::json!({
            "type": "hello", "protocolVersion": crate::PROTOCOL_VERSION, "clientName": o.client_name,
            "clientVersion": o.client_version, "token": o.token,
        });
        ws.send(Message::text(hello.to_string())).map_err(|_| "Gateway WebSocket closed.".to_string())?;
        let hello_deadline = Instant::now() + o.connect_timeout;
        loop {
            let left = hello_deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("Gateway hello timed out.".into());
            }
            let _ = ws.get_ref().set_read_timeout(Some(left));
            match ws.read() {
                Ok(Message::Text(t)) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&t) {
                        if v.get("type").and_then(Value::as_str) == Some("hello_ok") {
                            return Ok(Transport { ws, request_timeout: o.request_timeout });
                        }
                    }
                }
                Ok(Message::Close(f)) => return Err(close_error(f)),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                    return Err("Gateway hello timed out.".into())
                }
                Err(_) => return Err("Gateway WebSocket closed.".into()),
            }
        }
    }

    /// `request(method, params)`: params is the already-serialized JSON text (JS stringify semantics).
    pub fn request_raw(&mut self, method: &str, params_json: &str) -> Result<Value, String> {
        let id = crate::random_uuid_v4();
        let frame = format!(
            "{{\"type\":\"request\",\"id\":\"{id}\",\"method\":{},\"params\":{params_json}}}",
            serde_json::to_string(method).unwrap()
        );
        self.ws.send(Message::text(frame)).map_err(|_| "Gateway WebSocket is not connected.".to_string())?;
        let deadline = Instant::now() + self.request_timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!("Gateway request timed out: {method}"));
            }
            let _ = self.ws.get_ref().set_read_timeout(Some(left));
            let msg = match self.ws.read() {
                Ok(m) => m,
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                    return Err(format!("Gateway request timed out: {method}"))
                }
                Err(_) => return Err("Gateway WebSocket closed.".into()),
            };
            let t = match msg {
                Message::Text(t) => t,
                Message::Close(f) => return Err(close_error(f)),
                _ => continue,
            };
            let Ok(v) = serde_json::from_str::<Value>(&t) else {
                return Err("Invalid Gateway JSON frame.".into());
            };
            if v.get("type").and_then(Value::as_str) != Some("response") || v.get("id").and_then(Value::as_str) != Some(&id) {
                continue;
            }
            if v.get("ok") == Some(&Value::Bool(true)) {
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
            return Err(v
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("Gateway request failed.")
                .to_string());
        }
    }

    pub fn close(mut self) {
        let _ = self.ws.close(None);
        let _ = self.ws.flush();
    }
}
