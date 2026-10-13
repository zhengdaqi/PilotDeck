//! Native port of the PilotDeck Gateway server edge (`src/gateway/server/*`).
//!
//! Ported: `GatewayServer` HTTP routing (/health, /auth/local-token, the public approval/host
//! transports in their unbound configuration, `/api/web` auth + projects routes, static assets),
//! the WebSocket upgrade and text-frame transport (`websocket.ts`), the full
//! `GatewayWsConnection` protocol (hello/auth, request dispatch, submit_turn streaming,
//! permission notifications, close cleanup) and `ensureGatewayAuthToken`.
//! The `Gateway` itself (InProcessGateway: sessions, agent loop host, tools...) is NOT ported:
//! it sits behind [`backend::GatewayBackend`], served today by a host process over JSON lines
//! (`backend::ProcessGateway`). See docs/rust-migration/contracts/CAP-GATEWAY-SERVER.contract.md.
pub mod backend;
pub mod conn;
pub mod files;
pub mod public;
pub mod safe_write;
pub mod http;
pub mod token;
pub mod ws;

pub use pilotdeck_skills::jsjson::{stringify_compact, JsObject, JsValue};

pub const PILOTDECK_GATEWAY_PROTOCOL_VERSION: &str = "1.2";

pub fn get<'a>(v: &'a JsValue, k: &str) -> &'a JsValue {
    static UNDEF: JsValue = JsValue::Undefined;
    match v {
        JsValue::Obj(o) => o.get(k).unwrap_or(&UNDEF),
        _ => &UNDEF,
    }
}

pub fn obj(entries: Vec<(&str, JsValue)>) -> JsValue {
    let mut o = JsObject::new();
    for (k, v) in entries {
        o.set(k, v);
    }
    JsValue::Obj(o)
}

pub fn s(v: &str) -> JsValue {
    JsValue::Str(v.to_string())
}

/// `{ ...value }` for an arbitrary JS value (objects copy, arrays/strings spread indexes).
pub fn js_spread(v: &JsValue) -> JsObject {
    match v {
        JsValue::Obj(o) => o.clone(),
        JsValue::Arr(a) => {
            let mut o = JsObject::new();
            for (i, x) in a.iter().enumerate() {
                o.set(i.to_string(), x.clone());
            }
            o
        }
        JsValue::Str(st) => {
            let mut o = JsObject::new();
            for (i, u) in st.encode_utf16().enumerate() {
                o.set(i.to_string(), JsValue::Str(String::from_utf16_lossy(&[u])));
            }
            o
        }
        _ => JsObject::new(),
    }
}

pub fn truthy(v: &JsValue) -> bool {
    match v {
        JsValue::Undefined | JsValue::Null => false,
        JsValue::Bool(b) => *b,
        JsValue::Num(n) => *n != 0.0 && !n.is_nan(),
        JsValue::Str(x) => !x.is_empty(),
        _ => true,
    }
}

pub fn iso_now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}
