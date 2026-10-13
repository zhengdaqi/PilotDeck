//! Port of `src/gateway/server/websocket.ts`: server-side text WebSocket over a raw socket.
use base64::Engine;
use sha1::{Digest, Sha1};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{Arc, Mutex};

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

pub fn create_websocket_accept_value(key: &str) -> String {
    let mut h = Sha1::new();
    h.update(key.as_bytes());
    h.update(WS_GUID.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(h.finalize())
}

fn server_frame_header(len: usize, opcode: u8) -> Vec<u8> {
    if len < 126 {
        vec![0x80 | opcode, len as u8]
    } else if len <= 0xffff {
        vec![0x80 | opcode, 126, (len >> 8) as u8, len as u8]
    } else {
        let mut h = vec![0x80 | opcode, 127];
        h.extend_from_slice(&(len as u64).to_be_bytes());
        h
    }
}

pub enum Frame {
    Frame { opcode: u8, payload: Vec<u8>, consumed: usize },
    Incomplete,
}

/// `readClientFrame`. `Err` is the TS throw (unmasked / oversized frame), see SRC-DEFECT-004.
pub fn read_client_frame(buf: &[u8]) -> Result<Frame, &'static str> {
    let (first, second) = (buf[0], buf[1]);
    let opcode = first & 0x0f;
    let masked = second & 0x80 != 0;
    let mut len = (second & 0x7f) as u64;
    let mut off = 2usize;
    if len == 126 {
        if buf.len() < off + 2 {
            return Ok(Frame::Incomplete);
        }
        len = u16::from_be_bytes([buf[2], buf[3]]) as u64;
        off += 2;
    } else if len == 127 {
        if buf.len() < off + 8 {
            return Ok(Frame::Incomplete);
        }
        let mut b = [0u8; 8];
        b.copy_from_slice(&buf[2..10]);
        len = u64::from_be_bytes(b);
        if len > 9007199254740991 {
            return Err("WebSocket frame is too large.");
        }
        off += 8;
    }
    if !masked {
        return Err("Client WebSocket frames must be masked.");
    }
    if (buf.len() as u64) < off as u64 + 4 + len {
        return Ok(Frame::Incomplete);
    }
    let len = len as usize;
    let mask = [buf[off], buf[off + 1], buf[off + 2], buf[off + 3]];
    off += 4;
    let payload: Vec<u8> = buf[off..off + len].iter().enumerate().map(|(i, b)| b ^ mask[i % 4]).collect();
    Ok(Frame::Frame { opcode, payload, consumed: off + len })
}

struct WsState {
    stream: TcpStream,
    closed: bool,
}

/// Write half of a `TextWebSocketConnection`, shareable across request threads.
#[derive(Clone)]
pub struct WsSender(Arc<Mutex<WsState>>);

impl WsSender {
    fn lock(&self) -> std::sync::MutexGuard<'_, WsState> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn send_text(&self, message: &str) {
        let mut st = self.lock();
        if st.closed {
            return;
        }
        let mut out = server_frame_header(message.len(), 0x1);
        out.extend_from_slice(message.as_bytes());
        let _ = st.stream.write_all(&out);
    }
    pub fn close(&self, code: u16, reason: &str) {
        let mut st = self.lock();
        if st.closed {
            return;
        }
        st.closed = true;
        let mut payload = code.to_be_bytes().to_vec();
        payload.extend_from_slice(reason.as_bytes());
        let mut out = server_frame_header(payload.len(), 0x8);
        out.extend_from_slice(&payload);
        let _ = st.stream.write_all(&out);
        let _ = st.stream.shutdown(Shutdown::Write);
    }
    fn pong(&self, payload: &[u8]) {
        let mut st = self.lock();
        let mut out = server_frame_header(payload.len(), 0xa);
        out.extend_from_slice(payload);
        let _ = st.stream.write_all(&out);
    }
    fn mark_closed(&self) {
        self.lock().closed = true;
    }
    fn destroy(&self) {
        let mut st = self.lock();
        st.closed = true;
        let _ = st.stream.shutdown(Shutdown::Both);
    }
}

/// Wrap an upgraded socket: returns the shareable write half and the read half.
pub fn split(stream: TcpStream) -> Option<(WsSender, TcpStream)> {
    let reader = stream.try_clone().ok()?;
    Some((WsSender(Arc::new(Mutex::new(WsState { stream, closed: false }))), reader))
}

/// Read loop (`handleData`): invokes `on_message` for every text frame, answers pings and
/// close frames, and returns once the socket closed (TS `emitClose`).
pub fn serve(sender: &WsSender, mut reader: TcpStream, initial: Vec<u8>, mut on_message: impl FnMut(String)) {
    let mut buf = initial;
    let mut chunk = vec![0u8; 64 * 1024];
    'outer: loop {
        while buf.len() >= 2 {
            match read_client_frame(&buf) {
                Ok(Frame::Incomplete) => break,
                Err(_) => {
                    // SRC-DEFECT-004: TS throws inside the socket 'data' listener, which is an
                    // uncaught exception that terminates the whole server process. Bounded fix
                    // (pending user decision): drop only this connection.
                    sender.destroy();
                    break 'outer;
                }
                Ok(Frame::Frame { opcode, payload, consumed }) => {
                    buf.drain(..consumed);
                    match opcode {
                        0x8 => sender.close(1000, ""),
                        0x9 => sender.pong(&payload),
                        0x1 => on_message(String::from_utf8_lossy(&payload).into_owned()),
                        _ => {}
                    }
                    if opcode == 0x8 {
                        // TS returns from handleData after a close frame; bytes left in the buffer
                        // are only parsed on the next 'data' event.
                        break;
                    }
                }
            }
        }
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    sender.mark_closed();
}
