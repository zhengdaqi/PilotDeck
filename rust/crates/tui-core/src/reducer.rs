//! `app/types.ts` `applyGatewayEventToTuiState`.
use crate::truncate::js_trim_end;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum TuiMessage {
    System { text: String },
    User { text: String },
    Assistant {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        thinking: Option<String>,
    },
    Tool {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        ok: Option<bool>,
        #[serde(rename = "toolCallId", skip_serializing_if = "Option::is_none", default)]
        tool_call_id: Option<String>,
        #[serde(rename = "toolName", skip_serializing_if = "Option::is_none", default)]
        tool_name: Option<String>,
        #[serde(rename = "lineCount", skip_serializing_if = "Option::is_none", default)]
        line_count: Option<Value>,
        #[serde(rename = "resultPath", skip_serializing_if = "Option::is_none", default)]
        result_path: Option<String>,
        #[serde(rename = "fullText", skip_serializing_if = "Option::is_none", default)]
        full_text: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        expanded: Option<bool>,
    },
    Error { text: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityItem {
    pub id: String,
    pub text: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingPermission {
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "toolName")]
    pub tool_name: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReducerState {
    pub messages: Vec<TuiMessage>,
    pub activity: Vec<ActivityItem>,
    pub mode: String,
    #[serde(rename = "isRunning")]
    pub is_running: bool,
    #[serde(rename = "pendingPermissions")]
    pub pending_permissions: Vec<PendingPermission>,
}

impl Default for ReducerState {
    fn default() -> Self {
        Self { messages: vec![], activity: vec![], mode: "default".into(), is_running: false, pending_permissions: vec![] }
    }
}

fn s<'a>(e: &'a Value, k: &str) -> Option<&'a str> {
    e.get(k).and_then(Value::as_str)
}
fn st(e: &Value, k: &str) -> String {
    s(e, k).unwrap_or("").to_string()
}

/// JS `String.prototype.trim`.
fn js_trim(x: &str) -> &str {
    js_trim_end(x).trim_start_matches(crate::truncate::is_js_ws)
}

fn push_activity(list: &[ActivityItem], item: ActivityItem) -> Vec<ActivityItem> {
    let mut v = vec![item];
    v.extend(list.iter().cloned());
    v.truncate(8);
    v
}

pub fn apply(state: &ReducerState, e: &Value) -> ReducerState {
    let mut n = state.clone();
    match s(e, "type").unwrap_or("") {
        "turn_started" => {
            n.is_running = true;
            n.activity.clear();
            n.pending_permissions.clear();
        }
        "assistant_text_delta" => {
            let text = st(e, "text");
            if !text.is_empty() {
                match n.messages.last_mut() {
                    Some(TuiMessage::Assistant { text: t, .. }) => t.push_str(&text),
                    _ => n.messages.push(TuiMessage::Assistant { text, thinking: None }),
                }
            }
        }
        "assistant_thinking_delta" => {
            let text = st(e, "text");
            if !text.is_empty() {
                match n.messages.last_mut() {
                    Some(TuiMessage::Assistant { thinking, .. }) => thinking.get_or_insert_with(String::new).push_str(&text),
                    _ => n.messages.push(TuiMessage::Assistant { text: String::new(), thinking: Some(text.clone()) }),
                }
            }
            let think_len = match n.messages.last() {
                Some(TuiMessage::Assistant { thinking, .. }) => thinking.as_ref().map_or(0, |t| t.encode_utf16().count()),
                _ => 0,
            };
            n.activity = push_activity(&state.activity, ActivityItem { id: format!("thinking-{think_len}"), text, status: "info".into() });
        }
        "tool_call_started" => {
            n.activity = push_activity(&state.activity, ActivityItem { id: st(e, "toolCallId"), text: st(e, "name"), status: "running".into() });
        }
        "tool_call_finished" => {
            let ok = e.get("ok").and_then(Value::as_bool);
            let preview = js_trim(s(e, "resultPreview").unwrap_or("")).to_string();
            let text = if !preview.is_empty() { preview } else if ok == Some(true) { "ok".into() } else { "error".into() };
            let id = st(e, "toolCallId");
            n.messages.push(TuiMessage::Tool {
                text,
                ok,
                tool_call_id: s(e, "toolCallId").map(str::to_string),
                tool_name: s(e, "toolName").map(str::to_string),
                line_count: e.get("resultLineCount").filter(|v| !v.is_null()).cloned(),
                result_path: s(e, "resultPath").map(str::to_string),
                full_text: None,
                expanded: None,
            });
            n.activity.retain(|a| a.id != id);
        }
        "permission_request" => {
            let rid = st(e, "requestId");
            let tool = st(e, "toolName");
            n.pending_permissions.push(PendingPermission { request_id: rid.clone(), tool_name: tool.clone(), payload: e.get("payload").cloned().unwrap_or(Value::Null) });
            n.activity = push_activity(&state.activity, ActivityItem { id: rid, text: format!("permission: {tool}"), status: "info".into() });
        }
        "structured_output" => {
            let payload = e.get("payload").cloned().unwrap_or(Value::Null);
            n.messages.push(TuiMessage::System { text: serde_json::to_string_pretty(&payload).unwrap_or_default() });
        }
        "plan_mode_changed" => n.mode = st(e, "mode"),
        "turn_completed" => {
            n.is_running = false;
            n.activity.clear();
            n.pending_permissions.clear();
        }
        "error" => {
            n.is_running = false;
            n.activity.clear();
            n.pending_permissions.clear();
            n.messages.push(TuiMessage::Error { text: st(e, "message") });
        }
        "tool_result_detail_available" => {
            let id = s(e, "toolCallId");
            for m in &mut n.messages {
                if let TuiMessage::Tool { tool_call_id, result_path, full_text, .. } = m {
                    if tool_call_id.as_deref() == id {
                        if let Some(p) = s(e, "resultPath") {
                            *result_path = Some(p.into());
                        }
                        if let Some(f) = s(e, "fullText") {
                            *full_text = Some(f.into());
                        }
                    }
                }
            }
        }
        _ => {}
    }
    n
}
