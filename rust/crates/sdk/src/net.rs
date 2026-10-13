//! Native network layer of `packages/sdk/src/transport.ts` `GatewayTransport`: a blocking API over
//! [`TransportCore`], with one IO thread per socket. Mirrors the TS flow:
//! - connect: `connect` / `openWithRetry` / `open` / `waitForOpen`, the hello timer and the retry backoff;
//! - requests: `request` with its timer;
//! - streams: `stream` (pull with [`GatewayStream::next`]);
//! - `onNotification`, `close`.
//!
//! Intended difference: when TS drops a socket after a failure (`this.socket = undefined`), it never closes it.
//! Rust closes it, so no connection leaks.
use crate::transport_core::{close_error, hello_frame, is_retryable_connect_error, retry_delay_ms, Effect, EventQueue, QueueNext, SdkError, TransportCore};
use serde_json::Value;
use std::collections::HashMap;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

#[derive(Clone, Debug, Default)]
pub struct ReconnectOptions {
    pub max_attempts: Option<f64>,
    pub initial_delay_ms: Option<f64>,
    pub max_delay_ms: Option<f64>,
    pub jitter: Option<f64>,
}

/// `GatewayConnectionOptions`.
#[derive(Clone, Debug, Default)]
pub struct ConnectionOptions {
    pub url: String,
    pub token: String,
    pub client_name: Option<String>,
    pub client_version: Option<String>,
    pub protocol_version: Option<String>,
    pub connect_timeout_ms: Option<u64>,
    pub request_timeout_ms: Option<u64>,
    pub reconnect: Option<ReconnectOptions>,
}

type Listener = Arc<dyn Fn(&Value) + Send + Sync>;

enum Out {
    Text(String),
    Close,
}

#[derive(Default)]
struct St {
    core: TransportCore,
    /// Generation of the current socket; an IO thread whose generation is stale ignores its socket.
    gen: u64,
    out: Option<Sender<Out>>,
    results: HashMap<String, Result<Option<Value>, SdkError>>,
    queues: HashMap<String, EventQueue<Value>>,
    listeners: Vec<(u64, Listener)>,
    next_listener: u64,
    /// `helloReject` / `openReject` outcome for the socket being opened.
    open_error: Option<SdkError>,
}

struct Shared {
    connecting: Mutex<()>,
    st: Mutex<St>,
    cv: Condvar,
}

/// Applies effects under the lock; returns the notifications to deliver after unlocking.
fn apply(st: &mut St, effects: Vec<Effect>) -> Vec<Value> {
    let mut notes = vec![];
    for e in effects {
        match e {
            Effect::Hello(_) => {}
            Effect::Settle { id, result } => {
                st.results.insert(id, result);
            }
            Effect::StreamItem { id, value } => {
                if let Some(q) = st.queues.get_mut(&id) {
                    q.push(value);
                }
            }
            Effect::StreamEnd { id, error } => {
                if let Some(q) = st.queues.get_mut(&id) {
                    match error {
                        Some(e) => q.fail(e),
                        None => q.close(),
                    }
                }
            }
            Effect::Notification(n) => notes.push(n),
            Effect::DropSocket => {
                if let Some(tx) = st.out.take() {
                    let _ = tx.send(Out::Close);
                }
            }
        }
    }
    notes
}

fn deliver(listeners: Vec<Listener>, notes: Vec<Value>) {
    for n in notes {
        for l in &listeners {
            // Application listeners must not break transport semantics.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| l(&n)));
        }
    }
}

fn would_block(e: &tungstenite::Error) -> bool {
    matches!(e, tungstenite::Error::Io(io) if matches!(io.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted))
}

fn io_loop(shared: Arc<Shared>, gen: u64, mut ws: WebSocket<TcpStream>, rx: Receiver<Out>) {
    let _ = ws.get_ref().set_read_timeout(Some(Duration::from_millis(5)));
    loop {
        loop {
            match rx.try_recv() {
                Ok(Out::Text(t)) => {
                    let _ = ws.send(Message::text(t));
                }
                Ok(Out::Close) | Err(TryRecvError::Disconnected) => {
                    let _ = ws.close(None);
                    let _ = ws.flush();
                    return;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        let (code, reason) = match ws.read() {
            Ok(Message::Text(t)) => {
                let mut st = shared.st.lock().unwrap();
                if st.gen != gen {
                    return;
                }
                let effects = st.core.handle_message(t.as_str());
                if effects.iter().any(|e| matches!(e, Effect::DropSocket)) && st.open_error.is_none() {
                    st.open_error = Some(SdkError::new("validation_error", "Invalid Gateway JSON frame.", false));
                }
                let notes = apply(&mut st, effects);
                let listeners = st.listeners.iter().map(|l| l.1.clone()).collect();
                drop(st);
                shared.cv.notify_all();
                deliver(listeners, notes);
                continue;
            }
            Ok(Message::Close(f)) => f.map_or((1005, String::new()), |f| (u16::from(f.code), f.reason.to_string())),
            Ok(_) => continue,
            Err(e) if would_block(&e) => continue,
            Err(_) => (1006, String::new()),
        };
        let mut st = shared.st.lock().unwrap();
        if st.gen == gen && st.core.connected {
            if st.open_error.is_none() {
                st.open_error = Some(close_error(code, &reason));
            }
            let effects = st.core.handle_close(code, &reason);
            apply(&mut st, effects);
        }
        drop(st);
        shared.cv.notify_all();
        let _ = ws.flush();
        return;
    }
}

/// `GatewayTransport` (WebSocket).
#[derive(Clone)]
pub struct GatewayTransport {
    options: ConnectionOptions,
    shared: Arc<Shared>,
}

/// One stream returned by [`GatewayTransport::stream`] (`AsyncEventQueue` iteration).
pub struct GatewayStream {
    shared: Arc<Shared>,
    id: String,
}

impl GatewayStream {
    /// Blocks for the next item; `None` when the stream ended normally.
    pub fn next(&mut self) -> Option<Result<Value, SdkError>> {
        let mut st = self.shared.st.lock().unwrap();
        loop {
            let q = st.queues.get_mut(&self.id)?;
            match q.next() {
                QueueNext::Item(v) => return Some(Ok(v)),
                QueueNext::Done => return None,
                QueueNext::Error(e) => return Some(Err(e)),
                QueueNext::Pending => st = self.shared.cv.wait(st).unwrap(),
            }
        }
    }
}

impl Iterator for GatewayStream {
    type Item = Result<Value, SdkError>;
    fn next(&mut self) -> Option<Self::Item> {
        GatewayStream::next(self)
    }
}

pub fn random_uuid() -> String {
    pilotdeck_gateway_client::random_uuid_v4()
}

fn math_random() -> f64 {
    let mut b = [0u8; 8];
    let _ = getrandom::getrandom(&mut b);
    (u64::from_le_bytes(b) >> 11) as f64 / (1u64 << 53) as f64
}

impl GatewayTransport {
    pub fn new(options: ConnectionOptions) -> Self {
        GatewayTransport { options, shared: Arc::new(Shared { connecting: Mutex::new(()), st: Mutex::new(St::default()), cv: Condvar::new() }) }
    }

    fn connect_timeout(&self) -> Duration {
        Duration::from_millis(self.options.connect_timeout_ms.unwrap_or(10_000))
    }

    /// `connect()`. Concurrent callers are serialized; a caller that finds the transport connected gets the cached
    /// server info (TS shares the in-flight promise; the outcome is the same).
    pub fn connect(&self) -> Result<Value, SdkError> {
        let _g = self.shared.connecting.lock().unwrap_or_else(|e| e.into_inner());
        {
            let st = self.shared.st.lock().unwrap();
            if let (Some(h), true) = (&st.core.hello, st.core.connected) {
                return Ok(TransportCore::server_info(h));
            }
        }
        self.open_with_retry()
    }

    fn open_with_retry(&self) -> Result<Value, SdkError> {
        let p = self.options.reconnect.clone().unwrap_or_default();
        let max_attempts = p.max_attempts.unwrap_or(1.0).floor().max(1.0);
        let initial = p.initial_delay_ms.unwrap_or(100.0).max(0.0);
        let max_delay = p.max_delay_ms.unwrap_or(2_000.0).max(initial);
        let jitter = p.jitter.unwrap_or(0.2).clamp(0.0, 1.0);
        let mut attempt = 0u32;
        loop {
            if self.shared.st.lock().unwrap().core.closed {
                return Err(SdkError::new("transport_error", "Gateway client closed.", false));
            }
            attempt += 1;
            match self.open() {
                Ok(v) => return Ok(v),
                Err(e) => {
                    let closed = self.shared.st.lock().unwrap().core.closed;
                    if attempt as f64 >= max_attempts || !is_retryable_connect_error(&e) || closed {
                        return Err(e);
                    }
                    {
                        let mut st = self.shared.st.lock().unwrap();
                        st.core.connected = false;
                        if let Some(tx) = st.out.take() {
                            let _ = tx.send(Out::Close);
                        }
                    }
                    let factor = if jitter == 0.0 { 1.0 } else { 1.0 + (math_random() * 2.0 - 1.0) * jitter };
                    std::thread::sleep(Duration::from_millis(retry_delay_ms(attempt, initial, max_delay, factor) as u64));
                }
            }
        }
    }

    fn dial(&self) -> Result<WebSocket<TcpStream>, SdkError> {
        let failed = || SdkError::new("transport_error", "Gateway connection failed.", true);
        let timed_out = || SdkError::new("timeout", "Gateway connection timed out.", true);
        let (hostport, ws_url) = pilotdeck_gateway_client::transport::parse_ws_url(&self.options.url).map_err(|m| {
            if m == "Gateway connection failed." {
                failed()
            } else {
                SdkError::new("transport_error", m, true)
            }
        })?;
        let deadline = Instant::now() + self.connect_timeout();
        let addrs: Vec<_> = hostport.to_socket_addrs().map_err(|_| failed())?.collect();
        let mut stream = None;
        for a in addrs {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(timed_out());
            }
            match TcpStream::connect_timeout(&a, left) {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => return Err(timed_out()),
                Err(_) => {}
            }
        }
        let stream = stream.ok_or_else(failed)?;
        let left = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1));
        let _ = stream.set_read_timeout(Some(left));
        let _ = stream.set_nodelay(true);
        match tungstenite::client(ws_url, stream) {
            Ok((ws, _)) => Ok(ws),
            Err(tungstenite::HandshakeError::Failure(ref e)) if would_block(e) => Err(timed_out()),
            Err(_) => Err(failed()),
        }
    }

    fn open(&self) -> Result<Value, SdkError> {
        self.shared.st.lock().unwrap().core.hello = None;
        let ws = self.dial()?;
        let (tx, rx) = channel();
        let gen;
        {
            let mut st = self.shared.st.lock().unwrap();
            if st.core.closed {
                return Err(SdkError::new("transport_error", "Gateway client closed.", false));
            }
            st.gen += 1;
            gen = st.gen;
            st.core.connected = true;
            st.open_error = None;
            let o = &self.options;
            let _ = tx.send(Out::Text(hello_frame(o.protocol_version.as_deref(), o.client_name.as_deref(), o.client_version.as_deref(), &o.token)));
            st.out = Some(tx);
        }
        let shared = self.shared.clone();
        std::thread::spawn(move || io_loop(shared, gen, ws, rx));
        let deadline = Instant::now() + self.connect_timeout();
        let mut st = self.shared.st.lock().unwrap();
        loop {
            if st.gen != gen {
                return Err(SdkError::new("transport_error", "Gateway client closed.", false));
            }
            if let Some(h) = &st.core.hello {
                return Ok(TransportCore::server_info(h));
            }
            if let Some(e) = st.open_error.take() {
                return Err(e);
            }
            if !st.core.connected {
                return Err(SdkError::new("transport_error", "Gateway disconnected during hello.", true));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(SdkError::new("timeout", "Gateway hello timed out.", true));
            }
            st = self.shared.cv.wait_timeout(st, left).unwrap().0;
        }
    }

    /// `request(method, params)`. `Ok(None)` is an `undefined` result.
    pub fn request(&self, method: &str, params: &Value) -> Result<Option<Value>, SdkError> {
        let id = random_uuid();
        let timeout = Duration::from_millis(self.options.request_timeout_ms.unwrap_or(30_000));
        let deadline = Instant::now() + timeout;
        let mut st = self.shared.st.lock().unwrap();
        let frame = st.core.begin_request(&id, method, params)?;
        if let Some(tx) = &st.out {
            let _ = tx.send(Out::Text(frame));
        }
        loop {
            if let Some(r) = st.results.remove(&id) {
                return r;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(st.core.timeout_request(&id, method).unwrap_or_else(|| SdkError::new("timeout", format!("Gateway request timed out: {method}"), true)));
            }
            st = self.shared.cv.wait_timeout(st, left).unwrap().0;
        }
    }

    /// `stream(method, params)`.
    pub fn stream(&self, method: &str, params: &Value) -> GatewayStream {
        let id = random_uuid();
        let mut st = self.shared.st.lock().unwrap();
        let mut q = EventQueue::default();
        match st.core.begin_stream(&id, method, params) {
            Ok(frame) => {
                if let Some(tx) = &st.out {
                    let _ = tx.send(Out::Text(frame));
                }
            }
            Err(e) => q.fail(e),
        }
        st.queues.insert(id.clone(), q);
        GatewayStream { shared: self.shared.clone(), id }
    }

    /// `onNotification(listener)`; returns the id to pass to [`GatewayTransport::off_notification`].
    pub fn on_notification(&self, listener: impl Fn(&Value) + Send + Sync + 'static) -> u64 {
        let mut st = self.shared.st.lock().unwrap();
        st.next_listener += 1;
        let id = st.next_listener;
        st.listeners.push((id, Arc::new(listener)));
        id
    }

    pub fn off_notification(&self, id: u64) -> bool {
        let mut st = self.shared.st.lock().unwrap();
        let n = st.listeners.len();
        st.listeners.retain(|l| l.0 != id);
        n != st.listeners.len()
    }

    /// `close()`.
    pub fn close(&self) {
        let mut st = self.shared.st.lock().unwrap();
        if st.core.closed {
            return;
        }
        if st.open_error.is_none() {
            st.open_error = Some(SdkError::new("transport_error", "Gateway client closed.", false));
        }
        let effects = st.core.close();
        apply(&mut st, effects);
        drop(st);
        self.shared.cv.notify_all();
    }
}
