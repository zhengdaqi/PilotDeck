//! `formatToolSummary.ts` and `groupConsecutiveTools.ts`.
use crate::reducer::TuiMessage;
use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

fn re(cell: &'static OnceLock<Regex>, p: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(p).unwrap())
}

fn js_slice_units(s: &str, n: usize) -> String {
    let u: Vec<u16> = s.encode_utf16().take(n).collect();
    String::from_utf16_lossy(&u)
}

pub fn format_tool_summary(tool_name: &str, args_hint: Option<&str>, line_count: i64, _bytes: i64, ok: bool, preview: &str) -> String {
    static PASS: OnceLock<Regex> = OnceLock::new();
    static FAIL: OnceLock<Regex> = OnceLock::new();
    static ERRS: OnceLock<Regex> = OnceLock::new();
    static ERR: OnceLock<Regex> = OnceLock::new();
    if !ok {
        let first = js_slice_units(preview.split('\n').next().unwrap_or(""), 80);
        return format!("{tool_name}: \u{2717} {first}");
    }
    let label = match args_hint {
        Some(h) if !h.is_empty() => format!("{tool_name}: {h}"),
        _ => tool_name.to_string(),
    };
    if tool_name == "bash" {
        if let Some(p) = re(&PASS, r"([0-9]+)\s*(?i-u:passing|passed|ok)").captures(preview) {
            return match re(&FAIL, r"([0-9]+)\s*(?i-u:failing|failed)").captures(preview) {
                Some(f) => format!("{label} \u{2192} \u{2717} {} passed, {} failed", &p[1], &f[1]),
                None => format!("{label} \u{2192} \u{2713} {} passed", &p[1]),
            };
        }
        if re(&ERRS, r"(?i-u:error\s*TS[0-9]+|SyntaxError|TypeError|ReferenceError)").is_match(preview) {
            let n = re(&ERR, r"(?i-u:error)").find_iter(preview).count();
            return format!("{label} \u{2192} \u{2717} {n} error{}", if n > 1 { "s" } else { "" });
        }
    }
    format!("{label} \u{2192} {line_count} lines")
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum DisplayItem {
    #[serde(rename = "single")]
    Single { message: TuiMessage, index: usize },
    #[serde(rename = "tool_group")]
    ToolGroup {
        #[serde(rename = "toolName")]
        tool_name: String,
        messages: Vec<TuiMessage>,
        #[serde(rename = "startIndex")]
        start_index: usize,
        expanded: bool,
    },
}

fn tool_name(m: &TuiMessage) -> Option<&str> {
    match m {
        TuiMessage::Tool { tool_name, .. } => tool_name.as_deref(),
        _ => None,
    }
}

pub fn group_consecutive_tools(messages: &[TuiMessage]) -> Vec<DisplayItem> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        // JS truthiness: empty toolName does not start a group.
        if let Some(name) = tool_name(&messages[i]).filter(|n| !n.is_empty()) {
            let mut j = i + 1;
            while j < messages.len() && tool_name(&messages[j]) == Some(name) {
                j += 1;
            }
            if j - i >= 3 {
                let slice = messages[i..j].to_vec();
                let expanded = slice.iter().any(|m| matches!(m, TuiMessage::Tool { expanded: Some(true), .. }));
                items.push(DisplayItem::ToolGroup { tool_name: name.to_string(), messages: slice, start_index: i, expanded });
                i = j;
                continue;
            }
        }
        items.push(DisplayItem::Single { message: messages[i].clone(), index: i });
        i += 1;
    }
    items
}
