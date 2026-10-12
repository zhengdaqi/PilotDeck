//! Port of the title resolution used by search (`readSessionInfo` in
//! `src/session/storage/SessionList.ts` + `SessionLiteReader.ts`). Only the
//! fields that influence the search-visible title are reproduced.

use regex::Regex;
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::sync::OnceLock;

pub const SESSION_LITE_READ_BYTES: u64 = 65_536;
const SCAN_CHUNK: usize = 64 * 1024;
const MAX_METADATA_LINE_BYTES: usize = 128 * 1024;

/// A JS exception escaping `readSessionInfo` (rejects the whole search).
#[derive(Debug, Clone)]
pub struct JsThrow(pub String);

#[derive(Default, Debug, Clone)]
struct Metadata {
    is_snapshot: bool,
    title: Option<String>,
    ai_title: Option<String>,
    first_prompt: Option<String>,
    last_prompt: Option<String>,
}

impl Metadata {
    fn merge(mut self, other: Metadata) -> Metadata {
        if other.is_snapshot {
            self.is_snapshot = true;
        }
        macro_rules! take {
            ($f:ident) => {
                if other.$f.is_some() {
                    self.$f = other.$f;
                }
            };
        }
        take!(title);
        take!(ai_title);
        take!(first_prompt);
        take!(last_prompt);
        self
    }
}

/// Title fields of `SessionInfo` that search uses.
#[derive(Debug, Clone)]
pub struct TitleInfo {
    pub custom_title: Option<String>,
    pub ai_title: Option<String>,
    pub summary: String,
}

impl TitleInfo {
    /// `customTitle ?? aiTitle ?? summary ?? sessionId`
    pub fn display_title(&self) -> String {
        self.custom_title
            .clone()
            .or_else(|| self.ai_title.clone())
            .unwrap_or_else(|| self.summary.clone())
    }
}

struct Lite {
    size: u64,
    head: String,
    tail: String,
}

fn read_lite(path: &str) -> Option<Lite> {
    let meta = std::fs::metadata(path).ok()?;
    let size = meta.len();
    if size == 0 {
        return None;
    }
    let mut file = File::open(path).ok()?;
    let head_len = size.min(SESSION_LITE_READ_BYTES) as usize;
    let mut head_buf = vec![0u8; head_len];
    let n = read_fully(&mut file, &mut head_buf).ok()?;
    let head = String::from_utf8_lossy(&head_buf[..n]).into_owned();
    let tail_offset = size.saturating_sub(SESSION_LITE_READ_BYTES);
    let tail = if tail_offset > 0 {
        let mut buf = vec![0u8; (size - tail_offset).min(SESSION_LITE_READ_BYTES) as usize];
        file.seek(SeekFrom::Start(tail_offset)).ok()?;
        let n = read_fully(&mut file, &mut buf).ok()?;
        String::from_utf8_lossy(&buf[..n]).into_owned()
    } else {
        head.clone()
    };
    Some(Lite { size, head, tail })
}

fn read_fully(file: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        let n = file.read(&mut buf[total..])?;
        if n == 0 {
            break;
        }
        total += n;
    }
    Ok(total)
}

/// JS `split(/\r?\n/)`
fn split_lines(s: &str) -> impl Iterator<Item = &str> {
    s.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l))
}

fn field_regex(field: &str) -> Regex {
    Regex::new(&format!(r#""{}"\s*:\s*"((?:\\.|[^"])*)""#, regex::escape(field))).unwrap()
}

fn unescape_json_string(value: &str) -> Result<String, JsThrow> {
    serde_json::from_str::<String>(&format!("\"{value}\""))
        .map_err(|e| JsThrow(format!("{e}")))
}

fn last_metadata_string_field(source: &str, field: &str) -> Result<Option<String>, JsThrow> {
    static CACHE: OnceLock<Vec<(String, Regex)>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        ["title", "aiTitle"].iter().map(|f| (f.to_string(), field_regex(f))).collect()
    });
    let re = &cache.iter().find(|(f, _)| f == field).expect("known field").1;
    let mut value = None;
    for line in split_lines(source) {
        if !line.contains("\"session_metadata\"") {
            continue;
        }
        if let Some(c) = re.captures(line) {
            let m = c.get(1).map(|m| m.as_str()).unwrap_or("");
            if !m.is_empty() {
                value = Some(unescape_json_string(m)?);
            }
        }
    }
    Ok(value)
}

/// Evaluate `entry.messages?.flatMap(m => m.content ?? []).find(b => b.type === "text")?.text?.trim()`
/// with JS throw semantics (Err = TypeError).
fn accepted_input_text(entry: &Value) -> Result<Option<String>, ()> {
    let messages = match entry {
        Value::Object(map) => map.get("messages"),
        _ => None,
    };
    let messages = match messages {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Array(items)) => items,
        Some(_) => return Err(()),
    };
    let mut blocks: Vec<&Value> = Vec::new();
    for message in messages {
        let content = match message {
            Value::Null => return Err(()),
            Value::Object(map) => map.get("content"),
            _ => None,
        };
        match content {
            None | Some(Value::Null) => {}
            Some(Value::Array(items)) => blocks.extend(items.iter()),
            Some(other) => blocks.push(other),
        }
    }
    for block in blocks {
        match block {
            Value::Null => return Err(()),
            Value::Object(map) if map.get("type") == Some(&Value::String("text".into())) => {
                return match map.get("text") {
                    None | Some(Value::Null) => Ok(None),
                    Some(Value::String(s)) => Ok(Some(crate::js::js_trim(s).to_string())),
                    Some(_) => Err(()),
                };
            }
            _ => {}
        }
    }
    Ok(None)
}

fn first_accepted_input_text(head: &str) -> Option<String> {
    for line in split_lines(head) {
        if !line.contains("\"type\":\"accepted_input\"") {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else { return None };
        match accepted_input_text(&entry) {
            Err(()) => return None,
            Ok(Some(t)) if !t.is_empty() => return Some(t),
            Ok(_) => {}
        }
    }
    None
}

fn last_accepted_input_text(tail: &str) -> Option<String> {
    let mut last = None;
    for line in split_lines(tail) {
        if !line.contains("\"type\":\"accepted_input\"") {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else { continue };
        if let Ok(Some(t)) = accepted_input_text(&entry) {
            if !t.is_empty() {
                last = Some(t);
            }
        }
    }
    last
}

fn parse_session_metadata_line(line: &str) -> Option<Metadata> {
    if !line.contains("\"type\":\"session_metadata\"") {
        return None;
    }
    let entry: Value = serde_json::from_str(line).ok()?;
    let obj = entry.as_object()?;
    if obj.get("type")?.as_str()? != "session_metadata" {
        return None;
    }
    let md = obj.get("metadata")?.as_object()?;
    let s = |k: &str| md.get(k).and_then(|v| v.as_str()).map(str::to_string);
    Some(Metadata {
        is_snapshot: md.get("isSnapshot") == Some(&Value::Bool(true)),
        title: s("title"),
        ai_title: s("aiTitle"),
        first_prompt: s("firstPrompt"),
        last_prompt: s("lastPrompt"),
    })
}

fn info_from_lite(lite: &Lite) -> Result<Option<TitleInfo>, JsThrow> {
    let source = format!("{}\n{}", lite.head, lite.tail);
    let custom_title = last_metadata_string_field(&source, "title")?;
    let ai_title = last_metadata_string_field(&source, "aiTitle")?;
    let first_prompt = first_accepted_input_text(&lite.head);
    let last_prompt = last_accepted_input_text(&lite.tail).or(first_prompt);
    let summary = custom_title.clone().or_else(|| ai_title.clone()).or(last_prompt);
    Ok(summary.filter(|s| !s.is_empty()).map(|summary| TitleInfo { custom_title, ai_title, summary }))
}

fn info_from_metadata(md: &Metadata) -> Option<TitleInfo> {
    let summary = md
        .title
        .clone()
        .or_else(|| md.ai_title.clone())
        .or_else(|| md.last_prompt.clone())
        .or_else(|| md.first_prompt.clone())?;
    if crate::js::js_trim(&summary).is_empty() {
        return None;
    }
    Some(TitleInfo { custom_title: md.title.clone(), ai_title: md.ai_title.clone(), summary })
}

fn latest_tail_snapshot(lite: &Lite) -> Option<Metadata> {
    if lite.size <= SESSION_LITE_READ_BYTES {
        return None;
    }
    let mut latest: Option<Metadata> = None;
    for line in split_lines(&lite.tail).skip(1) {
        if !line.contains("\"type\":\"session_metadata\"") {
            continue;
        }
        latest = Some(parse_session_metadata_line(line)?);
    }
    let md = latest?;
    let nonblank = |v: &Option<String>| v.as_deref().is_some_and(|s| !crate::js::js_trim(s).is_empty());
    if md.is_snapshot
        && (nonblank(&md.title) || nonblank(&md.ai_title) || nonblank(&md.last_prompt) || nonblank(&md.first_prompt))
    {
        Some(md)
    } else {
        None
    }
}

/// Reproduces `readLastSessionMetadata`, including its 64 KiB chunking and
/// 128 KiB per-line cap for non-metadata records.
fn read_last_session_metadata(path: &str) -> Option<Metadata> {
    const NEEDLE: &[u8] = b"\"type\":\"session_metadata\"";
    let bytes = std::fs::read(path).ok()?;
    let contains = |hay: &[u8]| hay.windows(NEEDLE.len()).any(|w| w == NEEDLE);
    let mut last: Option<Metadata> = None;
    let finish = |line: &[u8], too_large: bool, last: &mut Option<Metadata>| {
        if too_large {
            return;
        }
        let text = String::from_utf8_lossy(line);
        let text = text.strip_suffix('\r').unwrap_or(&text);
        if let Some(md) = parse_session_metadata_line(text) {
            *last = Some(last.take().unwrap_or_default().merge(md));
        }
    };
    let mut line_start = 0usize;
    let mut is_md = false;
    let mut too_large = false;
    let mut i = 0usize;
    let len = bytes.len();
    // Walk segment boundaries: newlines and 64 KiB chunk ends.
    while i < len {
        let chunk_end = ((i / SCAN_CHUNK) + 1) * SCAN_CHUNK;
        let limit = chunk_end.min(len);
        let nl = bytes[i..limit].iter().position(|&b| b == b'\n').map(|p| i + p);
        let seg_end = nl.unwrap_or(limit);
        if seg_end > i && !too_large {
            if !is_md {
                is_md = contains(&bytes[line_start..seg_end]);
            }
            if !is_md && seg_end - line_start > MAX_METADATA_LINE_BYTES {
                too_large = true;
            }
        }
        match nl {
            Some(n) => {
                finish(&bytes[line_start..n], too_large, &mut last);
                line_start = n + 1;
                is_md = false;
                too_large = false;
                i = n + 1;
            }
            None => i = limit,
        }
    }
    if line_start < len {
        finish(&bytes[line_start..len], too_large, &mut last);
    }
    last
}

/// `readSessionInfo(path, sessionId)` restricted to title fields.
pub fn read_session_title_info(path: &str) -> Result<Option<TitleInfo>, JsThrow> {
    let Some(lite) = read_lite(path) else { return Ok(None) };
    let fast = info_from_lite(&lite)?;
    if fast.is_some() && lite.size <= SESSION_LITE_READ_BYTES {
        return Ok(fast);
    }
    if let Some(snapshot) = latest_tail_snapshot(&lite) {
        if let Some(info) = info_from_metadata(&snapshot) {
            return Ok(Some(info));
        }
    }
    let meta = read_last_session_metadata(path).and_then(|m| info_from_metadata(&m));
    Ok(meta.or(fast))
}
