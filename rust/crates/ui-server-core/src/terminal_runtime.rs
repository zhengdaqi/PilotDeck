//! Ports of `ui/server/services/terminalSessionRegistry.js` and `runtimeCoordination.js`.
//!
//! Both are host-driven: PTYs, sockets, timers and the supervisor IPC channel are supplied by the caller
//! through small traits, so the same state machines run under the axum host and under the oracle test.
use serde_json::{json, Value};

pub const DEFAULT_TERMINAL_SESSION_TIMEOUT_MS: f64 = 30.0 * 60.0 * 1000.0;
pub const DEFAULT_TERMINAL_BUFFER_ENTRIES: f64 = 5000.0;

/// The PTY surface the registry touches. Methods mirror `pty?.kill?.()` (errors swallowed) and the
/// `typeof pty.write === 'function'` checks: `write`/`resize` return false when the PTY has no such method.
pub trait TerminalPty: Clone + PartialEq {
    fn kill(&self);
    fn write(&self, data: &str) -> bool;
    fn resize(&self, columns: f64, rows: f64) -> bool;
}

pub trait TimerHost {
    fn set_timeout(&mut self, ms: f64) -> u64;
    fn clear_timeout(&mut self, id: u64);
}

/// A socket slot: JS distinguishes `undefined` (reconnect without a socket) from `null` (detached).
#[derive(Clone, Debug, PartialEq)]
pub enum WsSlot<W> {
    Undefined,
    Null,
    Ws(W),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalSnapshot<P, W> {
    pub key: String,
    pub pty: P,
    pub ws: WsSlot<W>,
    pub buffer: Vec<String>,
    pub project_path: Option<Value>,
    pub session_id: Option<Value>,
}

struct Session<P, W> {
    pty: P,
    ws: WsSlot<W>,
    buffer: std::collections::VecDeque<String>,
    timeout_id: Option<u64>,
    project_path: Option<Value>,
    session_id: Option<Value>,
}

pub struct TerminalSessionRegistry<P, W> {
    sessions: Vec<(String, Session<P, W>)>,
    disposed: bool,
    timeout_ms: f64,
    max_buffer_entries: usize,
    /// Pending detach timers: id -> (key, pty) captured by the JS closure.
    timers: Vec<(u64, String, P)>,
}

/// A JS `TypeError` raised by the registry.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeError(pub String);

impl<P: TerminalPty, W: Clone + PartialEq> TerminalSessionRegistry<P, W> {
    /// `new TerminalSessionRegistry({ timeoutMs, maxBufferEntries })` (None = default).
    pub fn new(timeout_ms: Option<f64>, max_buffer_entries: Option<f64>) -> Result<Self, TypeError> {
        let t = timeout_ms.unwrap_or(DEFAULT_TERMINAL_SESSION_TIMEOUT_MS);
        let m = max_buffer_entries.unwrap_or(DEFAULT_TERMINAL_BUFFER_ENTRIES);
        if !t.is_finite() || t < 0.0 {
            return Err(TypeError("Terminal session timeout must be a non-negative finite number.".into()));
        }
        if !m.is_finite() || m.trunc() != m || m < 1.0 {
            return Err(TypeError("Terminal session buffer limit must be a positive integer.".into()));
        }
        Ok(TerminalSessionRegistry { sessions: vec![], disposed: false, timeout_ms: t, max_buffer_entries: m as usize, timers: vec![] })
    }

    pub fn size(&self) -> usize {
        self.sessions.len()
    }

    fn idx(&self, key: &str) -> Option<usize> {
        self.sessions.iter().position(|(k, _)| k == key)
    }

    fn snapshot(&self, i: usize) -> TerminalSnapshot<P, W> {
        let (key, s) = &self.sessions[i];
        TerminalSnapshot {
            key: key.clone(),
            pty: s.pty.clone(),
            ws: s.ws.clone(),
            buffer: s.buffer.iter().cloned().collect(),
            project_path: s.project_path.clone(),
            session_id: s.session_id.clone(),
        }
    }

    pub fn get(&self, key: &str) -> Option<TerminalSnapshot<P, W>> {
        self.idx(key).map(|i| self.snapshot(i))
    }

    fn current(&self, key: &str, pty: Option<&P>) -> Option<usize> {
        self.idx(key).filter(|&i| Some(&self.sessions[i].1.pty) == pty)
    }

    fn cancel_timeout(&mut self, h: &mut dyn TimerHost, i: usize) {
        if let Some(id) = self.sessions[i].1.timeout_id.take() {
            h.clear_timeout(id);
            self.timers.retain(|(t, _, _)| *t != id);
        }
    }

    /// `#stop`: remove and kill.
    fn stop(&mut self, h: &mut dyn TimerHost, i: usize) {
        self.cancel_timeout(h, i);
        let (_, s) = self.sessions.remove(i);
        s.pty.kill();
    }

    pub fn register(
        &mut self,
        h: &mut dyn TimerHost,
        key: &str,
        pty: Option<P>,
        ws: WsSlot<W>,
        project_path: Option<Value>,
        session_id: Option<Value>,
    ) -> Result<Option<TerminalSnapshot<P, W>>, TypeError> {
        if key.is_empty() {
            return Err(TypeError("Terminal session key must be a non-empty string.".into()));
        }
        let Some(pty) = pty else { return Err(TypeError("Terminal session requires a PTY.".into())) };
        if self.disposed {
            pty.kill();
            return Ok(None);
        }
        // Map.set on an existing key keeps its slot; the prior session is removed first, so the new one
        // goes to the end exactly like `delete` + `set`.
        if let Some(i) = self.idx(key) {
            self.stop(h, i);
        }
        let ws = if matches!(ws, WsSlot::Undefined) { WsSlot::Null } else { ws };
        self.sessions.push((
            key.into(),
            Session { pty, ws, buffer: Default::default(), timeout_id: None, project_path, session_id },
        ));
        Ok(Some(self.snapshot(self.sessions.len() - 1)))
    }

    pub fn replace(&mut self, h: &mut dyn TimerHost, key: &str) -> bool {
        match self.idx(key) {
            Some(i) => {
                self.stop(h, i);
                true
            }
            None => false,
        }
    }

    pub fn reconnect(&mut self, h: &mut dyn TimerHost, key: &str, pty: Option<&P>, ws: WsSlot<W>) -> Option<TerminalSnapshot<P, W>> {
        let i = self.current(key, pty)?;
        if self.disposed {
            return None;
        }
        self.cancel_timeout(h, i);
        self.sessions[i].1.ws = ws;
        Some(self.snapshot(i))
    }

    pub fn append_output(&mut self, key: &str, pty: Option<&P>, chunk: &str) -> Option<TerminalSnapshot<P, W>> {
        let i = self.current(key, pty)?;
        let max = self.max_buffer_entries;
        let buf = &mut self.sessions[i].1.buffer;
        if buf.len() >= max {
            buf.pop_front();
        }
        buf.push_back(chunk.into());
        Some(self.snapshot(i))
    }

    pub fn complete(&mut self, h: &mut dyn TimerHost, key: &str, pty: Option<&P>) -> Option<TerminalSnapshot<P, W>> {
        let i = self.current(key, pty)?;
        let snap = self.snapshot(i);
        self.cancel_timeout(h, i);
        self.sessions.remove(i);
        Some(snap)
    }

    pub fn write(&mut self, key: &str, pty: Option<&P>, ws: &WsSlot<W>, data: &str) -> bool {
        match self.current(key, pty) {
            Some(i) if self.sessions[i].1.ws == *ws => self.sessions[i].1.pty.write(data),
            _ => false,
        }
    }

    pub fn resize(&mut self, key: &str, pty: Option<&P>, ws: &WsSlot<W>, columns: f64, rows: f64) -> bool {
        match self.current(key, pty) {
            Some(i) if self.sessions[i].1.ws == *ws => self.sessions[i].1.pty.resize(columns, rows),
            _ => false,
        }
    }

    pub fn detach(&mut self, h: &mut dyn TimerHost, key: &str, pty: Option<&P>, ws: &WsSlot<W>) -> Option<TerminalSnapshot<P, W>> {
        let i = self.current(key, pty)?;
        if self.sessions[i].1.ws != *ws {
            return None;
        }
        self.sessions[i].1.ws = WsSlot::Null;
        self.cancel_timeout(h, i);
        let id = h.set_timeout(self.timeout_ms);
        self.sessions[i].1.timeout_id = Some(id);
        self.timers.push((id, key.into(), self.sessions[i].1.pty.clone()));
        Some(self.snapshot(i))
    }

    /// A detach timer fired.
    pub fn fire_timeout(&mut self, h: &mut dyn TimerHost, id: u64) {
        let Some(t) = self.timers.iter().position(|(x, _, _)| *x == id) else { return };
        let (_, key, pty) = self.timers.remove(t);
        let Some(i) = self.current(&key, Some(&pty)) else { return };
        if matches!(self.sessions[i].1.ws, WsSlot::Ws(_)) {
            return;
        }
        self.stop(h, i);
    }

    pub fn dispose(&mut self, h: &mut dyn TimerHost) {
        if self.disposed {
            return;
        }
        self.disposed = true;
        while !self.sessions.is_empty() {
            self.stop(h, 0);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// runtimeCoordination

const CONFIGURATION_MESSAGE: &str = "pilotdeck:configuration-state";
const GATEWAY_STATE_MESSAGE: &str = "pilotdeck:gateway-state";
const GATEWAY_RETRY_MESSAGE: &str = "pilotdeck:retry-gateway";

/// The `process`-like IPC channel of a supervised UI server.
pub trait SupervisorChannel {
    /// `typeof processLike.send === 'function'`.
    fn can_send(&self) -> bool;
    /// `processLike.connected === false`.
    fn disconnected(&self) -> bool;
    /// `processLike.send(message)`; Err when it throws.
    fn send(&mut self, message: &Value) -> Result<(), ()>;
    /// `processLike.on?.('message', ...)` / `off?.(...)`.
    fn subscribe(&mut self);
    fn unsubscribe(&mut self);
}

pub struct RuntimeCoordination {
    supervised: bool,
    gateway: Value,
    configuration: Value,
}

impl RuntimeCoordination {
    /// `createRuntimeCoordination({ processLike, env })`; `supervised_env` is `env.PILOTDECK_RUNTIME_SUPERVISED`.
    pub fn new(channel: &mut dyn SupervisorChannel, supervised_env: Option<&str>) -> Self {
        let supervised = supervised_env == Some("1") && channel.can_send();
        if supervised {
            channel.subscribe();
        }
        RuntimeCoordination {
            supervised,
            gateway: json!({ "state": if supervised { "stopped" } else { "unmanaged" } }),
            configuration: Value::Null,
        }
    }

    /// The `'message'` listener (only subscribed when supervised; the host routes messages here).
    pub fn on_message(&mut self, message: &Value) {
        if message.get("type").and_then(Value::as_str) != Some(GATEWAY_STATE_MESSAGE) {
            return;
        }
        let Some(state) = message.get("state").and_then(Value::as_str).filter(|s| ["stopped", "starting", "ready", "error"].contains(s)) else {
            return;
        };
        let mut g = serde_json::Map::new();
        g.insert("state".into(), state.into());
        if let Some(e) = message.get("error").and_then(Value::as_str).filter(|e| !e.is_empty()) {
            g.insert("error".into(), e.into());
        }
        self.gateway = Value::Object(g);
    }

    fn send(&self, channel: &mut dyn SupervisorChannel, message: Value) -> bool {
        if !self.supervised || channel.disconnected() {
            return false;
        }
        channel.send(&message).is_ok()
    }

    pub fn gateway_state(&self) -> Value {
        self.gateway.clone()
    }

    pub fn configuration_state(&self) -> Value {
        self.configuration.clone()
    }

    pub fn publish_configuration_state(&mut self, channel: &mut dyn SupervisorChannel, state: Value) -> Value {
        self.configuration = state;
        self.send(channel, json!({ "type": CONFIGURATION_MESSAGE, "configuration": self.configuration }));
        self.configuration.clone()
    }

    pub fn request_gateway_retry(&mut self, channel: &mut dyn SupervisorChannel) -> bool {
        let requested = self.send(channel, json!({ "type": GATEWAY_RETRY_MESSAGE }));
        if requested {
            self.gateway = json!({ "state": "starting" });
        }
        requested
    }

    pub fn dispose(&mut self, channel: &mut dyn SupervisorChannel) {
        if self.supervised {
            channel.unsubscribe();
        }
    }
}
