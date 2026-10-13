//! Port of src/model/providerEndpoint.ts, normalizeNetworkError (src/network/fetch.ts) and
//! ui/server/services/modelConnectionProbe.js. The probe is sans-IO: the host supplies an async `fetch`.
//! JS details kept: `x || []` + `.some`/`for…of` TypeErrors, `String(v)` coercions, ASCII-only `/i`
//! folding (no `u` flag), `.` excluding line terminators and `.{0,60}` counting UTF-16 code units.
use crate::js_number::{is_js_ws, js_trim};
use crate::js_object::{field, truthy};
use crate::js_object_string;
use serde_json::{json, Map, Value};
use std::future::Future;

// ---------------------------------------------------------------- providerEndpoint

fn trim_slashes(v: &str) -> &str {
    v.trim_matches('/')
}

fn join_url(base: &str, path: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), path.trim_start_matches('/'))
}

fn path_segments(base: &str) -> Vec<String> {
    match url::Url::parse(base) {
        Ok(u) => u.path().split('/').filter(|s| !s.is_empty()).map(str::to_string).collect(),
        Err(_) => vec![],
    }
}

/// `/^v\d+(?:beta\d*)?$/i`
fn is_version_segment(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || !b[0].eq_ignore_ascii_case(&b'v') {
        return false;
    }
    let mut i = 1;
    let d = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == d {
        return false;
    }
    if i == b.len() {
        return true;
    }
    if b.len() < i + 4 || !b[i..i + 4].eq_ignore_ascii_case(b"beta") {
        return false;
    }
    b[i + 4..].iter().all(u8::is_ascii_digit)
}

fn path_ends_with(base: &str, suffix: &[String]) -> bool {
    let segs: Vec<String> = path_segments(base).iter().map(|s| s.to_lowercase()).collect();
    if segs.len() < suffix.len() {
        return false;
    }
    let off = segs.len() - suffix.len();
    suffix.iter().enumerate().all(|(i, s)| segs[off + i] == s.to_lowercase())
}

fn base_url_ends_with_endpoint(base: &str, endpoint: &str) -> bool {
    let segs: Vec<String> = trim_slashes(endpoint).split('/').filter(|s| !s.is_empty()).map(str::to_string).collect();
    if path_ends_with(base, &segs) {
        return true;
    }
    if segs.len() == 2 && segs[0].to_lowercase() == "models" {
        let bsegs: Vec<String> = path_segments(base).iter().map(|s| s.to_lowercase()).collect();
        let last = bsegs.last().cloned().unwrap_or_default();
        let suffix = segs[1].find(':').map(|i| segs[1][i..].to_lowercase()).unwrap_or_default();
        return !suffix.is_empty() && bsegs.len() >= 2 && bsegs[bsegs.len() - 2] == "models" && last.ends_with(&suffix);
    }
    false
}

fn build_endpoint_candidates(base: &str, default_version: &str, endpoint: &str) -> Vec<String> {
    let base = js_trim(base).trim_end_matches('/');
    let endpoint = trim_slashes(endpoint);
    if base_url_ends_with_endpoint(base, endpoint) {
        return vec![base.to_string()];
    }
    let unversioned = join_url(base, endpoint);
    if path_segments(base).iter().any(|s| is_version_segment(s)) {
        return vec![unversioned];
    }
    let joined: Vec<&str> = [default_version, endpoint].into_iter().filter(|s| !s.is_empty()).collect();
    let versioned = join_url(base, &joined.join("/"));
    let mut out: Vec<String> = Vec::new();
    for u in [versioned, unversioned] {
        if !u.is_empty() && !out.contains(&u) {
            out.push(u);
        }
    }
    out
}

pub fn normalize_google_probe_model(model: Option<&str>) -> String {
    let text = js_trim(model.unwrap_or(""));
    let m = text.strip_prefix("google/").unwrap_or(text);
    match m {
        "gemini-3-pro" => "gemini-3-pro-preview",
        "gemini-3.1-pro" => "gemini-3.1-pro-preview",
        "gemini-3-flash" | "gemini-3.1-flash" | "gemini-3.1-flash-preview" => "gemini-3-flash-preview",
        "gemini-3.1-flash-lite" => "gemini-3.1-flash-lite-preview",
        other => other,
    }
    .to_string()
}

/// `encodeURIComponent`.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub struct EndpointInput<'a> {
    pub protocol: &'a str,
    pub base_url: &'a str,
    pub model: Option<&'a str>,
    pub google_method: Option<&'a str>,
}

pub fn build_provider_chat_endpoint_candidates(i: &EndpointInput) -> Vec<String> {
    match i.protocol {
        "anthropic" => build_endpoint_candidates(i.base_url, "v1", "messages"),
        "openai-responses" => build_endpoint_candidates(i.base_url, "v1", "responses"),
        "google" => {
            let method = i.google_method.filter(|m| !m.is_empty()).unwrap_or("generateContent");
            let model = encode_uri_component(&normalize_google_probe_model(i.model));
            let trimmed = js_trim(i.base_url).trim_end_matches('/');
            let base = if trimmed.is_empty() { "https://generativelanguage.googleapis.com" } else { trimmed };
            build_endpoint_candidates(base, "v1beta", &format!("models/{model}:{method}"))
        }
        _ => build_endpoint_candidates(i.base_url, "v1", "chat/completions"),
    }
}

pub fn build_provider_chat_endpoint(i: &EndpointInput) -> String {
    build_provider_chat_endpoint_candidates(i).into_iter().next().unwrap_or_default()
}

pub fn build_provider_models_endpoint_candidates(protocol: &str, base_url: &str) -> Vec<String> {
    if protocol == "google" {
        build_endpoint_candidates(base_url, "v1beta", "models")
    } else {
        build_endpoint_candidates(base_url, "v1", "models")
    }
}

pub fn build_provider_models_endpoint(protocol: &str, base_url: &str) -> String {
    build_provider_models_endpoint_candidates(protocol, base_url).into_iter().next().unwrap_or_default()
}

fn is_array(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Array(_)))
}

/// `!body || typeof body !== "object"` admits objects and arrays (arrays carry none of the probed keys).
fn object_like(body: Option<&Value>) -> bool {
    matches!(body, Some(Value::Object(_) | Value::Array(_)))
}

pub fn is_expected_provider_response_shape(protocol: &str, body: Option<&Value>) -> bool {
    if !object_like(body) {
        return false;
    }
    let f = |k| field(body, k);
    match protocol {
        "anthropic" => is_array(f("content")) || f("type") == Some(&json!("message")),
        "google" => is_array(f("candidates")),
        "openai-responses" => f("object") == Some(&json!("response")) || is_array(f("output")) || matches!(f("output_text"), Some(Value::String(_))),
        _ => is_array(f("choices")),
    }
}

pub fn is_expected_provider_models_response_shape(protocol: &str, body: Option<&Value>) -> bool {
    if !object_like(body) {
        return false;
    }
    if protocol == "google" {
        return is_array(field(body, "models"));
    }
    is_array(field(body, "data")) || is_array(field(body, "models"))
}

// ---------------------------------------------------------------- network errors

/// What a `fetch` rejection carried, within the JSON domain.
#[derive(Clone, Debug)]
pub enum FetchFailure {
    /// An `Error` with `message` and optional own `code` / `cause`.
    Error { message: String, code: Option<Value>, cause: Option<Value> },
    /// A non-Error rejection value (`None` = undefined).
    Other(Option<Value>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct NetworkFetchError {
    pub code: &'static str,
    pub message: String,
}

fn read_error_code(e: &FetchFailure) -> Option<String> {
    let FetchFailure::Error { code, cause, .. } = e else { return None };
    if let Some(Value::String(c)) = code {
        return Some(c.clone());
    }
    if let Some(Value::String(c)) = field(cause.as_ref(), "code") {
        return Some(c.clone());
    }
    None
}

/// `normalizeNetworkError(error)` without abort signals (the host maps aborts/timeouts itself).
pub fn normalize_network_error(e: &FetchFailure) -> NetworkFetchError {
    let message = match e {
        FetchFailure::Error { message, .. } => message.clone(),
        FetchFailure::Other(None | Some(Value::Null)) => "Network request failed.".into(),
        FetchFailure::Other(Some(v)) => js_object_string(v),
    };
    let combined = format!("{} {}", read_error_code(e).unwrap_or_default(), message).to_lowercase();
    let has = |n: &str| combined.contains(n);
    let code = if has("enotfound") || has("eai_again") || has("dns") {
        "network_dns_error"
    } else if has("econnreset") || has("socket hang up") || has("terminated") {
        "network_connection_reset"
    } else if has("econnrefused") {
        "network_connection_refused"
    } else if has("etimedout") || has("timeout") {
        "network_timeout"
    } else if has("certificate") || has("tls") || has("ssl") {
        "network_tls_error"
    } else if has("proxy") {
        "network_proxy_error"
    } else if has("abort") {
        "network_abort"
    } else {
        "network_fetch_failed"
    };
    NetworkFetchError { code, message }
}

// ---------------------------------------------------------------- probe helpers

const SOME_ERR: &str = "((intermediate value) || []).some is not a function";

/// `(v || []).some(pred)` — non-array truthy values throw V8's TypeError.
fn js_some(v: Option<&Value>, pred: impl Fn(Option<&Value>) -> bool) -> Result<bool, String> {
    if !truthy(v) {
        return Ok(false);
    }
    match v {
        Some(Value::Array(a)) => Ok(a.iter().any(|x| pred(Some(x)))),
        _ => Err(SOME_ERR.into()),
    }
}

/// `for (const x of v || [])` — strings iterate characters (whose probed properties are all undefined,
/// so they are yielded as nothing); numbers, booleans and plain objects are not iterable.
fn js_iter(v: Option<&Value>) -> Result<&[Value], String> {
    if !truthy(v) {
        return Ok(&[]);
    }
    match v {
        Some(Value::Array(a)) => Ok(a),
        Some(Value::String(_)) => Ok(&[]),
        Some(Value::Number(_) | Value::Bool(_)) => {
            Err(format!("{} {} is not iterable (cannot read property Symbol(Symbol.iterator))", if matches!(v, Some(Value::Bool(_))) { "boolean" } else { "number" }, js_object_string(v.unwrap())))
        }
        _ => Err("object is not iterable (cannot read property Symbol(Symbol.iterator))".into()),
    }
}

fn lower_str(v: Option<&Value>) -> String {
    if truthy(v) { js_object_string(v.unwrap()).to_lowercase() } else { String::new() }
}

pub fn has_error_finish(body: Option<&Value>, protocol: &str) -> Result<bool, String> {
    if truthy(field(body, "error")) || field(body, "status") == Some(&json!("failed")) {
        return Ok(true);
    }
    match protocol {
        "google" => js_some(field(body, "candidates"), |c| lower_str(field(c, "finishReason")) == "error"),
        "openai" => js_some(field(body, "choices"), |c| lower_str(field(c, "finish_reason")) == "error"),
        _ => Ok(lower_str(field(body, "stop_reason")) == "error"),
    }
}

pub fn extract_probe_text(body: Option<&Value>, protocol: &str) -> Result<String, String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut push = |v: Option<&Value>| {
        if let Some(Value::String(s)) = v {
            if !js_trim(s).is_empty() {
                chunks.push(s.clone());
            }
        }
    };
    match protocol {
        "anthropic" => {
            for part in js_iter(field(body, "content"))? {
                if field(Some(part), "type") == Some(&json!("text")) {
                    push(field(Some(part), "text"));
                }
            }
        }
        "google" => {
            for c in js_iter(field(body, "candidates"))? {
                for part in js_iter(field(field(Some(c), "content"), "parts"))? {
                    if !truthy(field(Some(part), "thought")) {
                        push(field(Some(part), "text"));
                    }
                }
            }
        }
        "openai-responses" => {
            let ot = field(body, "output_text");
            if truthy(ot) {
                return Ok(strip_thinking(&js_object_string(ot.unwrap())));
            }
            for item in js_iter(field(body, "output"))? {
                if field(Some(item), "type") == Some(&json!("reasoning")) {
                    continue;
                }
                for part in js_iter(field(Some(item), "content"))? {
                    push(field(Some(part), "text"));
                    push(field(Some(part), "output_text"));
                }
            }
        }
        _ => {
            for choice in js_iter(field(body, "choices"))? {
                match field(field(Some(choice), "message"), "content") {
                    Some(s @ Value::String(_)) => push(Some(s)),
                    Some(Value::Array(parts)) => {
                        for p in parts {
                            push(field(Some(p), "text"));
                        }
                    }
                    _ => {}
                }
                push(field(Some(choice), "text"));
            }
        }
    }
    Ok(strip_thinking(&chunks.join(" ")))
}

fn match_ci_at(s: &str, i: usize, lit: &str) -> bool {
    s.len() >= i + lit.len() && s.as_bytes()[i..i + lit.len()].eq_ignore_ascii_case(lit.as_bytes())
}

/// The opening tag `<think>` / `<thinking>` at byte `i`: returns (tag name length, tag end).
fn open_tag_at(s: &str, i: usize) -> Option<(usize, usize)> {
    for name in ["think", "thinking"] {
        let lit = format!("<{name}>");
        if match_ci_at(s, i, &lit) {
            return Some((name.len(), i + lit.len()));
        }
    }
    None
}

/// `String(text || '').replace(/<(think|thinking)>[\s\S]*?<\/\1>/gi, '').replace(/<(think|thinking)>[\s\S]*$/gi, '').trim()`
/// (the backreference compares ASCII-case-insensitively, as JS `/i` without `u`).
pub fn strip_thinking(text: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    let mut copied = 0;
    while i < text.len() {
        if let Some((n, body)) = open_tag_at(text, i) {
            let name = &text[i + 1..i + 1 + n];
            let close = format!("</{name}>");
            if let Some(end) = (body..=text.len().saturating_sub(close.len())).find(|&j| text.is_char_boundary(j) && match_ci_at(text, j, &close)) {
                out.push_str(&text[copied..i]);
                i = end + close.len();
                copied = i;
                continue;
            }
        }
        i += text[i..].chars().next().map_or(1, char::len_utf8);
    }
    out.push_str(&text[copied..]);
    let cut = (0..out.len()).find(|&j| out.is_char_boundary(j) && open_tag_at(&out, j).is_some()).unwrap_or(out.len());
    js_trim(&out[..cut]).to_string()
}

/// `stripThinking(v)` for any input value (`String(text || '')`).
pub fn strip_thinking_value(v: Option<&Value>) -> String {
    strip_thinking(&if truthy(v) { js_object_string(v.unwrap()) } else { String::new() })
}

pub fn is_incomplete(body: Option<&Value>, protocol: &str) -> Result<bool, String> {
    if field(body, "status") == Some(&json!("incomplete")) || truthy(field(body, "incomplete_details")) {
        return Ok(true);
    }
    match protocol {
        "google" => js_some(field(body, "candidates"), |c| field(c, "finishReason") == Some(&json!("MAX_TOKENS"))),
        "anthropic" => {
            let r = field(body, "stop_reason");
            Ok(r == Some(&json!("max_tokens")) || r == Some(&json!("pause_turn")))
        }
        _ => js_some(field(body, "choices"), |c| field(c, "finish_reason") == Some(&json!("length"))),
    }
}

pub fn is_valid_image_color_answer(answer: Option<&Value>, expected: Option<&Value>) -> bool {
    let color = lower_str(expected);
    !color.is_empty() && lower_str(answer).contains(&color)
}

/// `responseDetail(text, response)`: may yield any JSON value picked from the body.
pub fn response_detail(text: &str, status: u16, status_text: &str) -> Value {
    let fallback = || format!("{status} {status_text}");
    match serde_json::from_str::<Value>(text) {
        Ok(body) => {
            let b = Some(&body);
            let err = field(b, "error");
            for v in [field(err, "message"), field(err, "type"), field(b, "message")] {
                if truthy(v) {
                    return v.unwrap().clone();
                }
            }
            Value::String(fallback())
        }
        Err(_) => Value::String(if text.is_empty() { fallback() } else { text.to_string() }),
    }
}

/// JS-regex view of a string: ASCII-lowercased (`/i` without `u`) code units, astral characters
/// standing for two non-matching units.
fn regex_view(s: &str) -> Vec<char> {
    let mut out = Vec::new();
    for c in s.chars() {
        if c.len_utf16() == 2 {
            out.push('\u{E000}');
            out.push('\u{E000}');
        } else {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn lit_at(v: &[char], i: usize, lit: &str) -> Option<usize> {
    let mut j = i;
    for c in lit.chars() {
        if v.get(j) != Some(&c) {
            return None;
        }
        j += 1;
    }
    Some(j)
}

/// Positions after `(?:alts)` at i.
fn alts_at(v: &[char], i: usize, alts: &[&str]) -> Vec<usize> {
    alts.iter().filter_map(|a| lit_at(v, i, a)).collect()
}

/// Does `(?:a).{0,max}(?:b)` match starting at i?
fn gap_then(v: &[char], starts: Vec<usize>, max: usize, tail: &dyn Fn(&[char], usize) -> bool) -> bool {
    starts.into_iter().any(|s| {
        let mut k = s;
        loop {
            if tail(v, k) {
                return true;
            }
            if k - s >= max || k >= v.len() || is_line_terminator(v[k]) {
                return false;
            }
            k += 1;
        }
    })
}

/// `allowed values?\s*:\s*\[\s*['"]text['"]\s*\]` at i.
fn allowed_text_at(v: &[char], i: usize) -> bool {
    let ws = |k: &mut usize| {
        while *k < v.len() && is_js_ws(v[*k]) {
            *k += 1;
        }
    };
    let Some(mut k) = lit_at(v, i, "allowed value") else { return false };
    if v.get(k) == Some(&'s') {
        // `s?` is greedy but may backtrack; the next token (`\s*:`) can never start with 's'.
        k += 1;
    }
    ws(&mut k);
    if v.get(k) != Some(&':') { return false; }
    k += 1;
    ws(&mut k);
    if v.get(k) != Some(&'[') { return false; }
    k += 1;
    ws(&mut k);
    if !matches!(v.get(k), Some('\'' | '"')) { return false; }
    let Some(mut k) = lit_at(v, k + 1, "text") else { return false };
    if !matches!(v.get(k), Some('\'' | '"')) { return false; }
    k += 1;
    ws(&mut k);
    v.get(k) == Some(&']')
}

pub fn looks_like_image_unsupported(detail: &str) -> bool {
    let v = regex_view(detail);
    let neg = ["not supported", "unsupported", "not enabled", "not available"];
    let modal = ["image", "vision", "multimodal"];
    (0..=v.len()).any(|i| {
        gap_then(&v, alts_at(&v, i, &modal), 60, &|v, k| !alts_at(v, k, &neg).is_empty())
            || gap_then(&v, alts_at(&v, i, &["not supported", "unsupported", "does not support"]), 60, &|v, k| !alts_at(v, k, &modal).is_empty())
            || gap_then(&v, alts_at(&v, i, &["content.type", "content type"]), 60, &|v, k| allowed_text_at(v, k))
    })
}

/// `classifyProbeError(detail, status)` with `/api.?key|authentication|unauthori[sz]ed/i` and
/// `/model.+not found|unknown model|does not exist/i`.
pub fn classify_probe_error(detail: &str, status: Option<u16>) -> &'static str {
    let v = regex_view(detail);
    let any = |f: &dyn Fn(usize) -> bool| (0..=v.len()).any(f);
    let api_key = any(&|i| {
        lit_at(&v, i, "api").is_some_and(|k| lit_at(&v, k, "key").is_some() || (k < v.len() && !is_line_terminator(v[k]) && lit_at(&v, k + 1, "key").is_some()))
            || lit_at(&v, i, "authentication").is_some()
            || lit_at(&v, i, "unauthorised").is_some()
            || lit_at(&v, i, "unauthorized").is_some()
    });
    if matches!(status, Some(401 | 403)) || api_key {
        return "INVALID_API_KEY";
    }
    let model = any(&|i| {
        lit_at(&v, i, "model").is_some_and(|s| {
            // `.+` then "not found": at least one non-terminator before it.
            let mut k = s;
            while k < v.len() && !is_line_terminator(v[k]) {
                k += 1;
                if lit_at(&v, k, "not found").is_some() {
                    return true;
                }
            }
            false
        }) || lit_at(&v, i, "unknown model").is_some()
            || lit_at(&v, i, "does not exist").is_some()
    });
    if status == Some(404) || model {
        return "MODEL_NOT_FOUND";
    }
    "ENDPOINT_UNREACHABLE"
}

// ---------------------------------------------------------------- image probe

pub const IMAGE_PROMPT: &str = "What color is the shape in this image? Reply with one English color word.";
pub const IMAGE_COLORS: [(&str, [u8; 3]); 4] = [("red", [220, 48, 48]), ("blue", [40, 96, 220]), ("green", [36, 168, 72]), ("yellow", [232, 196, 40])];

#[derive(Clone, Debug)]
pub struct ImageProbe {
    pub color: String,
    pub prompt: String,
    /// Base64 PNG. The IDAT deflate stream differs from Node's zlib; the decoded pixels are identical.
    pub data: String,
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
        }
    }
    crc ^ 0xffff_ffff
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut td = kind.to_vec();
    td.extend_from_slice(data);
    out.extend_from_slice(&td);
    out.extend_from_slice(&crc32(&td).to_be_bytes());
}

pub fn solid_png_rows([r, g, b]: [u8; 3], size: u32) -> Vec<u8> {
    let mut rows = Vec::new();
    for _ in 0..size {
        rows.push(0);
        for _ in 0..size {
            rows.extend_from_slice(&[r, g, b]);
        }
    }
    rows
}

pub fn encode_solid_png(rgb: [u8; 3], size: u32) -> Vec<u8> {
    let mut out = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&size.to_be_bytes());
    ihdr[4..8].copy_from_slice(&size.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = 2;
    png_chunk(&mut out, b"IHDR", &ihdr);
    png_chunk(&mut out, b"IDAT", &miniz_oxide::deflate::compress_to_vec_zlib(&solid_png_rows(rgb, size), 6));
    png_chunk(&mut out, b"IEND", &[]);
    out
}

/// `createImageCapabilityProbe()`; `color_index` is the host's `randomInt(4)` (or 0 = red under VITEST).
pub fn create_image_capability_probe(color_index: usize) -> ImageProbe {
    use base64::Engine;
    let (color, rgb) = IMAGE_COLORS[color_index % IMAGE_COLORS.len()];
    ImageProbe { color: color.into(), prompt: IMAGE_PROMPT.into(), data: base64::engine::general_purpose::STANDARD.encode(encode_solid_png(rgb, 32)) }
}

pub struct ProbeRequest {
    pub headers: Map<String, Value>,
    pub body: Value,
}

/// `requestFor(...)`; `None` arguments are `undefined`. Errors are V8 TypeError messages.
pub fn request_for(protocol: &str, api_key: Option<&str>, model: Option<&str>, image: bool, max_tokens: Option<&Value>, probe: Option<&ImageProbe>) -> Result<ProbeRequest, String> {
    let text = if image {
        probe.ok_or("Cannot read properties of null (reading 'prompt')")?.prompt.clone()
    } else {
        "Reply exactly: 1".to_string()
    };
    let data = probe.map(|p| p.data.clone());
    let mut headers = Map::new();
    let mut body = Map::new();
    let put = |m: &mut Map<String, Value>, k: &str, v: Option<Value>| {
        if let Some(v) = v {
            m.insert(k.into(), v);
        }
    };
    let bearer = |h: &mut Map<String, Value>| {
        if let Some(k) = api_key.filter(|k| !k.is_empty()) {
            h.insert("Authorization".into(), json!(format!("Bearer {k}")));
        }
        h.insert("content-type".into(), json!("application/json"));
    };
    let model_v = model.map(|m| json!(m));
    let img_url = || format!("data:image/png;base64,{}", data.clone().unwrap_or_else(|| "undefined".into()));
    match protocol {
        "google" => {
            put(&mut headers, "x-goog-api-key", api_key.map(|k| json!(k)));
            headers.insert("content-type".into(), json!("application/json"));
            let parts = if image { json!([{ "text": text }, { "inlineData": { "mimeType": "image/png", "data": data } }]) } else { json!([{ "text": text }]) };
            body.insert("contents".into(), json!([{ "role": "user", "parts": parts }]));
            let mut gc = Map::new();
            put(&mut gc, "maxOutputTokens", max_tokens.cloned());
            body.insert("generationConfig".into(), Value::Object(gc));
        }
        "anthropic" => {
            put(&mut headers, "x-api-key", api_key.map(|k| json!(k)));
            headers.insert("anthropic-version".into(), json!("2023-06-01"));
            headers.insert("content-type".into(), json!("application/json"));
            put(&mut body, "model", model_v);
            put(&mut body, "max_tokens", max_tokens.cloned());
            let content = if image { json!([{ "type": "text", "text": text }, { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": data } }]) } else { json!(text) };
            body.insert("messages".into(), json!([{ "role": "user", "content": content }]));
        }
        "openai-responses" => {
            bearer(&mut headers);
            put(&mut body, "model", model_v);
            put(&mut body, "max_output_tokens", max_tokens.cloned());
            body.insert("store".into(), json!(false));
            let input = if image { json!([{ "role": "user", "content": [{ "type": "input_text", "text": text }, { "type": "input_image", "image_url": img_url() }] }]) } else { json!(text) };
            body.insert("input".into(), input);
        }
        _ => {
            bearer(&mut headers);
            put(&mut body, "model", model_v);
            put(&mut body, "max_tokens", max_tokens.cloned());
            let content = if image { json!([{ "type": "text", "text": text }, { "type": "image_url", "image_url": { "url": img_url() } }]) } else { json!(text) };
            body.insert("messages".into(), json!([{ "role": "user", "content": content }]));
        }
    }
    Ok(ProbeRequest { headers, body: Value::Object(body) })
}

// ---------------------------------------------------------------- probeModelConnection

pub struct ProbeInput<'a> {
    pub protocol: &'a str,
    pub base_url: &'a str,
    pub endpoint_url: Option<&'a str>,
    pub api_key: Option<&'a str>,
    pub model: Option<&'a str>,
    pub image: bool,
    pub max_tokens: Option<Value>,
}

pub struct FetchCall {
    pub url: String,
    pub headers: Map<String, Value>,
    pub body: String,
}

pub struct FetchResponse {
    pub status: u16,
    pub status_text: String,
    pub text: String,
}

/// Probe outcome, serialized exactly as the JS object.
pub fn probe_failure(image_unsupported: bool, code: &str, error: Value) -> Value {
    json!({ "ok": false, "imageUnsupported": image_unsupported, "code": code, "error": error })
}

/// `probeModelConnection` for one attempt per URL (the retry/backoff loop, the 60 s timeout and the caller's
/// abort signal belong to the host's `fetch`; a host timeout must surface as a `network_timeout` failure).
pub async fn probe_model_connection<F, Fut>(input: &ProbeInput<'_>, image_probe: Option<ImageProbe>, mut fetch: F) -> Value
where
    F: FnMut(FetchCall) -> Fut,
    Fut: Future<Output = Result<FetchResponse, FetchFailure>>,
{
    let image_probe = if input.image { image_probe } else { None };
    let protocol = input.protocol;
    let urls: Vec<String> = match input.endpoint_url.filter(|u| !u.is_empty()) {
        Some(u) => vec![u.to_string()],
        None => build_provider_chat_endpoint_candidates(&EndpointInput { protocol, base_url: input.base_url, model: input.model, google_method: None }),
    };
    let max_tokens = input.max_tokens.clone().unwrap_or(json!(4096));
    let request = match request_for(protocol, Some(input.api_key.unwrap_or("")), input.model, input.image, Some(&max_tokens), image_probe.as_ref()) {
        Ok(r) => r,
        Err(e) => return probe_failure(false, "ENDPOINT_UNREACHABLE", json!(e)),
    };
    let body_text = request.body.to_string();
    let mut last: Option<Value> = None;
    let outcome: Result<Option<Value>, String> = async {
        for url in &urls {
            let response = match fetch(FetchCall { url: url.clone(), headers: request.headers.clone(), body: body_text.clone() }).await {
                Ok(r) => r,
                Err(e) => {
                    let n = normalize_network_error(&e);
                    let msg = if n.code == "network_timeout" {
                        "Connection timed out after 60s.".to_string()
                    } else if n.message.is_empty() {
                        "NetworkFetchError".to_string()
                    } else {
                        n.message
                    };
                    return Ok(Some(probe_failure(false, "ENDPOINT_UNREACHABLE", json!(msg))));
                }
            };
            if (200..300).contains(&response.status) {
                let body = serde_json::from_str::<Value>(&response.text).unwrap_or(Value::Null);
                let b = Some(&body);
                let shape = is_expected_provider_response_shape(protocol, b);
                if shape && is_incomplete(b, protocol)? {
                    return Ok(Some(probe_failure(false, if input.image { "IMAGE_CAPABILITY_UNKNOWN" } else { "PROBE_INCOMPLETE" }, json!("The test response was truncated before completion."))));
                }
                if shape && !has_error_finish(b, protocol)? && !js_trim(&extract_probe_text(b, protocol)?).is_empty() {
                    if let Some(p) = &image_probe {
                        let answer = extract_probe_text(b, protocol)?;
                        if !is_valid_image_color_answer(Some(&json!(answer)), Some(&json!(p.color))) {
                            return Ok(Some(probe_failure(false, "IMAGE_CAPABILITY_UNKNOWN", json!("The model replied without describing the test image."))));
                        }
                    }
                    return Ok(Some(json!({ "ok": true, "endpointUrl": url })));
                }
                last = Some(json!(if shape {
                    if has_error_finish(b, protocol)? {
                        "Endpoint returned an error finish status."
                    } else {
                        "Endpoint returned a valid completion response, but the model did not produce any chat text."
                    }
                } else {
                    "The endpoint returned an invalid completion response."
                }));
                continue;
            }
            let detail = response_detail(&response.text, response.status, &response.status_text);
            let ds = js_object_string(&detail);
            if input.image && looks_like_image_unsupported(&ds) {
                return Ok(Some(probe_failure(true, "IMAGE_TEST_FAILED", detail)));
            }
            if urls.len() > 1 && matches!(response.status, 400 | 404 | 405) {
                last = Some(detail);
                continue;
            }
            return Ok(Some(probe_failure(false, classify_probe_error(&ds, Some(response.status)), detail)));
        }
        Ok(None)
    }
    .await;
    match outcome {
        Ok(Some(v)) => v,
        Err(type_error) => probe_failure(false, "ENDPOINT_UNREACHABLE", json!(type_error)),
        Ok(None) => {
            let detail = last.filter(|d| truthy(Some(d))).unwrap_or(json!("Connection failed."));
            let ds = js_object_string(&detail);
            probe_failure(input.image && looks_like_image_unsupported(&ds), classify_probe_error(&ds, None), detail)
        }
    }
}
