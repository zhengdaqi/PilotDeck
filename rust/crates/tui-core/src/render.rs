//! `tui-render.ts` (non-interactive render state).
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct TuiRenderState {
    pub transcript: Vec<String>,
    #[serde(rename = "activeTools")]
    pub active_tools: BTreeMap<String, String>,
    pub errors: Vec<String>,
}

pub fn apply_tui_event(state: &mut TuiRenderState, e: &Value) {
    let s = |k: &str| e.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    match e.get("type").and_then(Value::as_str).unwrap_or("") {
        "assistant_text_delta" => state.transcript.push(s("text")),
        "assistant_thinking_delta" => state.transcript.push(format!("[thinking] {}", s("text"))),
        "tool_call_started" => {
            state.active_tools.insert(s("toolCallId"), s("name"));
        }
        "tool_call_finished" => {
            state.active_tools.remove(&s("toolCallId"));
            let ok = e.get("ok").and_then(Value::as_bool) == Some(true);
            state.transcript.push(format!("[tool:{}] {}", s("toolCallId"), if ok { "ok" } else { "error" }));
        }
        "error" => state.errors.push(s("message")),
        _ => {}
    }
}
