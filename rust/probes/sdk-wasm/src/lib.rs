//! Probe for the npm wasm wrapper of the SDK (ADR-0002 D3).
//! - `HOOK_EVENTS` / `filterEscalatingDefaultMode`: pure exports compared against the TS SDK.
//! - `gatewayHello(url, token)`: the Gateway WS handshake through the *host's* WebSocket
//!   (web-sys), proving the transport can live in the wrapper without TCP access.
use js_sys::{Function, Promise, JSON};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{MessageEvent, WebSocket};

const HOOK_EVENTS: [&str; 33] = [
    "PreToolUse", "PostToolUse", "PostToolUseFailure", "PostToolBatch", "Notification",
    "UserPromptSubmit", "UserPromptExpansion", "SessionStart", "SessionEnd", "Stop", "StopFailure",
    "SubagentStart", "SubagentStop", "PreCompact", "PostCompact", "PreModelSwitch", "PostModelSwitch",
    "PermissionRequest", "PermissionDenied", "Setup", "TeammateIdle", "TaskCreated", "TaskCompleted",
    "Elicitation", "ElicitationResult", "ConfigChange", "WorktreeCreate", "WorktreeRemove",
    "InstructionsLoaded", "CwdChanged", "FileChanged", "DirectoryAdded", "MessageDisplay",
];

#[wasm_bindgen(js_name = hookEvents)]
pub fn hook_events() -> Vec<JsValue> {
    HOOK_EVENTS.iter().map(|s| JsValue::from_str(s)).collect()
}

/// `filterEscalatingDefaultMode(resolved)`: deep clone of `resolved.config`.
#[wasm_bindgen(js_name = filterEscalatingDefaultMode)]
pub fn filter_escalating_default_mode(resolved: JsValue) -> Result<JsValue, JsValue> {
    let cfg = js_sys::Reflect::get(&resolved, &"config".into())?;
    js_sys::global().dyn_into::<js_sys::Object>()?;
    let sc: Function = js_sys::Reflect::get(&js_sys::global(), &"structuredClone".into())?.dyn_into()?;
    sc.call1(&JsValue::NULL, &cfg)
}

/// Opens the Gateway WS, sends `hello`, resolves with the `hello_ok` frame (or rejects).
#[wasm_bindgen(js_name = gatewayHello)]
pub fn gateway_hello(url: String, token: String) -> Promise {
    Promise::new(&mut |resolve: Function, reject: Function| {
        let ws = match WebSocket::new(&url) {
            Ok(ws) => ws,
            Err(e) => {
                let _ = reject.call1(&JsValue::NULL, &e);
                return;
            }
        };
        let hello = serde_json::json!({
            "type": "hello", "protocolVersion": "1.2", "clientName": "sdk-wasm-probe", "clientVersion": "0.0.0", "token": token,
        })
        .to_string();
        let ws2 = ws.clone();
        let onopen = Closure::<dyn FnMut()>::new(move || {
            let _ = ws2.send_with_str(&hello);
        });
        let ws3 = ws.clone();
        let (res, rej) = (resolve.clone(), reject.clone());
        let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            let Some(text) = e.data().as_string() else { return };
            let Ok(v) = JSON::parse(&text) else { return };
            let ty = js_sys::Reflect::get(&v, &"type".into()).ok().and_then(|t| t.as_string()).unwrap_or_default();
            if ty == "hello_ok" {
                let _ = res.call1(&JsValue::NULL, &v);
                let _ = ws3.close();
            } else if ty == "error" || ty == "hello_error" {
                let _ = rej.call1(&JsValue::NULL, &v);
                let _ = ws3.close();
            }
        });
        let rej2 = reject.clone();
        let onclose = Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
            let _ = rej2.call1(&JsValue::NULL, &e);
        });
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onopen.forget();
        onmessage.forget();
        onclose.forget();
    })
}
