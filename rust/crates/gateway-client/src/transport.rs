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
    /// `hello_ok` frame.
    pub hello: Value,
    /// Frames for other ids received while waiting for a response (stream demux).
    backlog: std::collections::VecDeque<Value>,
    /// Streams: id → last seq.
    streams: std::collections::HashMap<String, i64>,
}

pub enum StreamItem {
    Event(Value),
    End,
    Interrupted,
}

/// WHATWG-ish URL checks performed by Node's `new WebSocket(url)`; returns (host:port, ws-url).
pub fn parse_ws_url(url: &str) -> Result<(String, String), String> {
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
                            return Ok(Transport {
                                ws,
                                request_timeout: o.request_timeout,
                                hello: v,
                                backlog: Default::default(),
                                streams: Default::default(),
                            });
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
                Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::Interrupted => continue,
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
                if v.get("id").and_then(Value::as_str).is_some_and(|i| self.streams.contains_key(i)) {
                    self.backlog.push_back(v);
                }
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

    /// `stream(method, params)`: sends the request and registers the stream id.
    pub fn stream_start(&mut self, method: &str, params_json: &str) -> Result<String, String> {
        let id = crate::random_uuid_v4();
        self.streams.insert(id.clone(), -1);
        let frame = format!(
            "{{\"type\":\"request\",\"id\":\"{id}\",\"method\":{},\"params\":{params_json}}}",
            serde_json::to_string(method).unwrap()
        );
        self.ws.send(Message::text(frame)).map_err(|_| "Gateway WebSocket is not connected.".to_string())?;
        Ok(id)
    }

    /// Next item of stream `id` (seq-validated, `{...event, sequence}` like the TS queue).
    pub fn stream_next(&mut self, id: &str, interrupt: &std::sync::atomic::AtomicBool) -> Result<StreamItem, String> {
        self.stream_next_inner(id, &|| interrupt.load(std::sync::atomic::Ordering::SeqCst), Duration::from_millis(100))
    }

    /// Non-blocking-ish poll for UIs: returns `Interrupted` once `deadline` passes with no frame.
    pub fn stream_poll(&mut self, id: &str, deadline: Instant) -> Result<StreamItem, String> {
        let left = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1));
        self.stream_next_inner(id, &|| Instant::now() >= deadline, left.min(Duration::from_millis(100)))
    }

    fn stream_next_inner(&mut self, id: &str, stop: &dyn Fn() -> bool, read_timeout: Duration) -> Result<StreamItem, String> {
        loop {
            let v = if let Some(v) = self.backlog.pop_front() {
                v
            } else {
                if stop() {
                    return Ok(StreamItem::Interrupted);
                }
                let _ = self.ws.get_ref().set_read_timeout(Some(read_timeout));
                let msg = match self.ws.read() {
                    Ok(m) => m,
                    Err(tungstenite::Error::Io(e))
                        if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted) =>
                    {
                        continue
                    }
                    Err(_) => return Err("Gateway WebSocket closed.".into()),
                };
                match msg {
                    Message::Text(t) => match serde_json::from_str::<Value>(&t) {
                        Ok(v) => v,
                        Err(_) => return Err("Invalid Gateway JSON frame.".into()),
                    },
                    Message::Close(f) => return Err(close_error(f)),
                    _ => continue,
                }
            };
            if v.get("id").and_then(Value::as_str) != Some(id) {
                continue;
            }
            match v.get("type").and_then(Value::as_str) {
                Some("response") => {
                    self.streams.remove(id);
                    if v.get("ok") == Some(&Value::Bool(true)) {
                        return Ok(StreamItem::End);
                    }
                    return Err(v
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(Value::as_str)
                        .unwrap_or("Gateway stream failed.")
                        .to_string());
                }
                Some("event") => {
                    let prev = *self.streams.get(id).unwrap_or(&-1);
                    let seq = v.get("seq").and_then(Value::as_f64).unwrap_or(-1.0);
                    if seq.fract() != 0.0 || seq < 0.0 {
                        self.streams.remove(id);
                        return Err(format!("Gateway event sequence must be a non-negative integer for {id}."));
                    }
                    let seq = seq as i64;
                    if seq <= prev {
                        continue;
                    }
                    if seq != prev + 1 {
                        self.streams.remove(id);
                        return Err(format!("Gateway event sequence gap for {id}: expected {}, received {seq}.", prev + 1));
                    }
                    self.streams.insert(id.to_string(), seq);
                    let mut ev = match v.get("event") {
                        Some(Value::Object(m)) => m.clone(),
                        _ => serde_json::Map::new(),
                    };
                    ev.insert("sequence".into(), Value::from(seq));
                    if v.get("final") == Some(&Value::Bool(true)) {
                        self.streams.remove(id);
                        self.backlog.push_front(serde_json::json!({"type":"response","id":id,"ok":true}));
                        self.streams.insert(id.to_string(), seq);
                    }
                    return Ok(StreamItem::Event(Value::Object(ev)));
                }
                _ => continue,
            }
        }
    }

    pub fn capabilities(&self) -> Vec<String> {
        self.hello
            .get("serverInfo")
            .and_then(|i| i.get("capabilities"))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default()
    }

    pub fn close(mut self) {
        let _ = self.ws.close(None);
        let _ = self.ws.flush();
    }
}
