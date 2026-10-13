//! Ports of `finalizePreparedModelRequest` helpers: `buildCachePlan` / `stableSerialize`
//! (src/context/cache/CachePlan.ts) and `materializeMediaReferences`
//! (src/model/request/materializeMediaReferences.ts).
use crate::json::*;
use crate::agent::nullish;
use crate::obj;
use std::cmp::Ordering;

pub const RECENT_MESSAGE_BREAKPOINT_COUNT: usize = 3;

/// `stableSerialize`: JSON with sorted keys (UTF-16 order); `undefined` prints as `undefined`.
pub fn stable_serialize(v: &JsValue) -> String {
    match v {
        JsValue::Undefined => "undefined".into(),
        JsValue::Arr(items) => format!("[{}]", items.iter().map(stable_serialize).collect::<Vec<_>>().join(",")),
        JsValue::Obj(o) => {
            let mut keys: Vec<&String> = o.iter().map(|(k, _)| k).collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            let parts: Vec<String> = keys.iter().map(|k| format!("{}:{}", to_json(&JsValue::Str((*k).clone())),
                stable_serialize(o.get(k.as_str()).unwrap_or(&UNDEF)))).collect();
            format!("{{{}}}", parts.join(","))
        }
        other => to_json(other),
    }
}

/// Primary weight of an ASCII char under the CLDR root collation (ICU, `localeCompare` with the
/// default locale): whitespace/punctuation/symbols < digits < letters (case-insensitive).
fn primary(c: char) -> (u32, u32) {
    const PUNCT: &str = "\t\n\u{b}\u{c}\r _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";
    if let Some(i) = PUNCT.find(c) {
        return (0, i as u32);
    }
    if c.is_ascii_digit() {
        return (1, c as u32);
    }
    if c.is_ascii_alphabetic() {
        return (2, c.to_ascii_lowercase() as u32);
    }
    (3, c as u32)
}

/// Approximation of `String.prototype.localeCompare` (ICU root collation) for ASCII-dominated
/// identifiers such as tool names: primary comparison, then lowercase-before-uppercase.
/// Verified against node by `tests/locale_compare.rs` on random ASCII strings.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    let pa: Vec<(u32, u32)> = a.chars().map(primary).collect();
    let pb: Vec<(u32, u32)> = b.chars().map(primary).collect();
    match pa.cmp(&pb) {
        Ordering::Equal => {}
        o => return o,
    }
    for (x, y) in a.chars().zip(b.chars()) {
        if x != y {
            // tertiary: lowercase sorts before uppercase
            return if x.is_ascii_lowercase() { Ordering::Less } else { Ordering::Greater };
        }
    }
    Ordering::Equal
}

pub fn select_recent_message_breakpoints(messages: &[JsValue]) -> Vec<usize> {
    let idx: Vec<usize> = messages.iter().enumerate().filter(|(_, m)| gs(m, "role") != Some("system")).map(|(i, _)| i).collect();
    let start = idx.len().saturating_sub(RECENT_MESSAGE_BREAKPOINT_COUNT);
    idx[start..].to_vec()
}

pub fn build_cache_plan(provider: &JsValue, model: &JsValue, system_prompt: &JsValue, tools: &[JsValue],
                        messages: &[JsValue], generation: &JsValue) -> JsValue {
    let picked = select_recent_message_breakpoints(messages);
    let mut stable_tools = tools.to_vec();
    stable_tools.sort_by(|l, r| {
        let (ln, rn) = (crate::rt::js_string(get(l, "name")), crate::rt::js_string(get(r, "name")));
        match locale_compare(&ln, &rn) {
            Ordering::Equal => locale_compare(&stable_serialize(l), &stable_serialize(r)),
            o => o,
        }
    });
    let or_empty = |v: &JsValue| if nullish(v) { JsValue::Str(String::new()) } else { v.clone() };
    let fp_input = obj! {"provider" => or_empty(provider), "model" => or_empty(model), "system" => or_empty(system_prompt),
        "tools" => JsValue::Arr(stable_tools), "messages" => picked.iter().map(|i| messages[*i].clone()).collect::<Vec<_>>()};
    let fingerprint = crate::util::sha256_hex(stable_serialize(&fp_input).as_bytes());
    obj! {"provider" => provider, "model" => model, "system" => truthy(system_prompt), "tools" => false,
        "messages" => picked.iter().map(|i| JsValue::Num(*i as f64)).collect::<Vec<_>>(),
        "fingerprint" => fingerprint, "generation" => generation}
}

/// `materializeMediaReferences`: returns the messages and `console.warn` diagnostic lines.
pub fn materialize_media_references(messages: &[JsValue]) -> (Vec<JsValue>, Vec<String>) {
    let mut diags = Vec::new();
    let out = messages.iter().map(|m| {
        let mut m = m.clone();
        let content: Vec<JsValue> = match get(&m, "content") {
            JsValue::Arr(blocks) => blocks.clone(),
            _ => Vec::new(),
        };
        let content: Vec<JsValue> = content.into_iter().map(|b| materialize_block(b, &mut diags)).collect();
        set(&mut m, "content", content);
        m
    }).collect();
    (out, diags)
}

fn materialize_block(block: JsValue, diags: &mut Vec<String>) -> JsValue {
    if gs(&block, "type") != Some("media_reference") {
        return block;
    }
    let path = crate::rt::js_string(get(&block, "path"));
    let media = crate::rt::js_string(get(&block, "mediaType"));
    let fallback = obj! {"type" => "text", "text" => get(&block, "preview")};
    match std::fs::read(&path) {
        Err(e) => {
            diags.push(format!("[pilotdeck] media_reference_materialization_failed: {} ({media}, {path})", node_fs_error(&e, &path)));
            fallback
        }
        Ok(bytes) => {
            let data = String::from_utf8_lossy(&bytes).into_owned();
            let mb = match media.as_str() {
                "image" => {
                    let mut b = obj! {"type" => "image", "source" => "base64", "data" => data, "mimeType" => get(&block, "mimeType"),
                        "bytes" => get(&block, "originalBytes")};
                    if truthy(get(&block, "detail")) {
                        set(&mut b, "detail", get(&block, "detail").clone());
                    }
                    Some(b)
                }
                "pdf" if gs(&block, "mimeType") == Some("application/pdf") => {
                    let mut b = obj! {"type" => "pdf", "source" => "base64", "data" => data, "mimeType" => "application/pdf",
                        "bytes" => get(&block, "originalBytes")};
                    if !is_undef(get(&block, "pages")) {
                        set(&mut b, "pages", get(&block, "pages").clone());
                    }
                    Some(b)
                }
                "audio" => Some(obj! {"type" => "audio", "source" => "base64", "data" => data, "mimeType" => get(&block, "mimeType"),
                    "bytes" => get(&block, "originalBytes")}),
                _ => None,
            };
            mb.unwrap_or_else(|| {
                diags.push(format!("[pilotdeck] media_reference_unsupported_type: Unsupported media reference type: {media} ({media}, {path})"));
                fallback
            })
        }
    }
}

fn node_fs_error(e: &std::io::Error, path: &str) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => format!("ENOENT: no such file or directory, open '{path}'"),
        std::io::ErrorKind::PermissionDenied => format!("EACCES: permission denied, open '{path}'"),
        _ if e.raw_os_error() == Some(21) => "EISDIR: illegal operation on a directory, read".to_string(),
        _ => e.to_string(),
    }
}
