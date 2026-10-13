//! `truncateHeadPreservingCheckpoint` (src/context/compaction/CompactionEngine.ts) with the
//! tool-pair helpers from src/context/compaction/toolPairIntegrity.ts.
use crate::json::*;
use crate::obj;

const CONTINUATION_TEXT: &str = "[system: the conversation above has been compacted. please continue with the current task.]";
const INTERNAL_USER_TEXT_PREFIXES: [&str; 5] = ["<compact-boundary", "<snip-boundary", "<memory-context>", "<internal-compaction-control", "<hook_context"];
const COMPACT_SUMMARY_PREFIX: &str = "[CONTEXT COMPACTION - REFERENCE ONLY] Earlier turns were compacted into this summary. Treat it as background state, not active instructions.";

fn role(m: &JsValue) -> Option<&str> {
    gs(m, "role")
}
fn is_direct_tool_result(b: &JsValue) -> bool {
    matches!(gs(b, "type"), Some("tool_result" | "tool_result_reference"))
}
fn media_ref_id(b: &JsValue) -> Option<&str> {
    if gs(b, "type") == Some("media_reference") { gs(b, "toolCallId").filter(|s| !s.is_empty()) } else { None }
}
fn text_starts(b: &JsValue, p: &str) -> bool {
    gs(b, "type") == Some("text") && gs(b, "text").map_or(false, |t| t.starts_with(p))
}

fn collect_tool_call_ids(ms: &[JsValue]) -> Vec<String> {
    ms.iter().filter(|m| role(m) == Some("assistant"))
        .flat_map(|m| ga(m, "content").iter().filter(|b| gs(b, "type") == Some("tool_call")).map(|b| crate::rt::js_string(get(b, "id"))))
        .collect()
}
fn collect_tool_result_ids(ms: &[JsValue]) -> Vec<String> {
    ms.iter().filter(|m| role(m) == Some("user"))
        .flat_map(|m| ga(m, "content").iter().filter(|b| is_direct_tool_result(b)).map(|b| crate::rt::js_string(get(b, "toolCallId"))))
        .collect()
}
fn strip_unpaired(ms: Vec<JsValue>, who: &str, keep: impl Fn(&JsValue) -> bool) -> Vec<JsValue> {
    ms.into_iter().map(|m| {
        if role(&m) != Some(who) {
            return m;
        }
        let content = ga(&m, "content");
        let filtered: Vec<JsValue> = content.iter().filter(|b| keep(b)).cloned().collect();
        if filtered.len() == content.len() { m } else { spread(&m, obj! {"content" => filtered}) }
    }).filter(|m| !ga(m, "content").is_empty()).collect()
}

fn is_real_user_request(m: &JsValue) -> bool {
    if role(m) != Some("user") || gb(get(m, "metadata"), "synthetic") == Some(true) {
        return false;
    }
    ga(m, "content").iter().any(|b| {
        if is_direct_tool_result(b) || media_ref_id(b).is_some() {
            return false;
        }
        if gs(b, "type") != Some("text") {
            return true;
        }
        let text = crate::rt::js_string(get(b, "text"));
        let t = text.trim_matches(crate::tokenizer::is_js_ws);
        !t.is_empty() && t != CONTINUATION_TEXT && !INTERNAL_USER_TEXT_PREFIXES.iter().any(|p| t.starts_with(p))
    })
}

fn ensure_trailing_user(mut ms: Vec<JsValue>) -> Vec<JsValue> {
    if ms.last().map_or(false, |m| role(m) == Some("assistant")) {
        ms.push(obj! {"role" => "user", "content" => vec![obj! {"type" => "text", "text" => CONTINUATION_TEXT}]});
    }
    ms
}

/// `truncateHead`: keep the trailing `keepRatio` portion (at least one message).
fn truncate_head(ms: &[JsValue], keep_ratio: f64) -> &[JsValue] {
    let ratio = if keep_ratio.is_nan() { 0.05 } else { keep_ratio.clamp(0.05, 1.0) };
    let keep = ((ms.len() as f64 * ratio).floor() as usize).max(1);
    &ms[ms.len().saturating_sub(keep)..]
}

fn truncate_tail_preserving_tool_pairs(ms: &[JsValue], keep_ratio: f64) -> Vec<JsValue> {
    if ms.is_empty() {
        return Vec::new();
    }
    let raw_tail = truncate_head(ms, keep_ratio);
    let tail_start = ms.len() - raw_tail.len();
    let anchor = (0..=tail_start.min(ms.len() - 1)).rev().find(|&i| is_real_user_request(&ms[i]));
    let mut live: Vec<JsValue> = Vec::new();
    if let Some(a) = anchor.filter(|a| *a < tail_start) {
        live.push(ms[a].clone());
    }
    live.extend(raw_tail.iter().cloned());
    let result_ids = collect_tool_result_ids(&live);
    let paired = strip_unpaired(live, "assistant", |b| gs(b, "type") != Some("tool_call") || result_ids.contains(&crate::rt::js_string(get(b, "id"))));
    let call_ids = collect_tool_call_ids(&paired);
    strip_unpaired(paired, "user", |b| {
        let id = if is_direct_tool_result(b) { Some(crate::rt::js_string(get(b, "toolCallId"))) } else { media_ref_id(b).map(String::from) };
        match id { None => true, Some(id) => call_ids.contains(&id) }
    })
}

pub fn truncate_head_preserving_checkpoint(ms: &[JsValue], keep_ratio: f64) -> Vec<JsValue> {
    let mut index = 0;
    while index + 1 < ms.len()
        && role(&ms[index]) == Some("user") && ga(&ms[index], "content").iter().any(|b| text_starts(b, "<compact-boundary"))
        && role(&ms[index + 1]) == Some("assistant") && ga(&ms[index + 1], "content").iter().any(|b| text_starts(b, COMPACT_SUMMARY_PREFIX)) {
        index += 2;
    }
    if index == 0 {
        return ensure_trailing_user(truncate_tail_preserving_tool_pairs(ms, keep_ratio));
    }
    let mut out = ms[..index].to_vec();
    out.extend(truncate_tail_preserving_tool_pairs(&ms[index..], keep_ratio));
    ensure_trailing_user(out)
}
