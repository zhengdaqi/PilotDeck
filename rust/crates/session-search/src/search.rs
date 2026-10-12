//! Port of `src/session/search/NodeSessionSearchPort.ts`.

use crate::js;
use crate::paths;
use crate::session_info::read_session_title_info;
use regex::{Regex, RegexBuilder};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

const DEFAULT_LIMIT: f64 = 20.0;
const MAX_LIMIT: f64 = 100.0;
const SNIPPET_RADIUS: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleFilter {
    All,
    Only(Role),
}

#[derive(Debug, Clone, Default)]
pub struct SearchInput {
    pub pilot_home: String,
    pub project_root: Option<String>,
    pub query: String,
    pub limit: Option<f64>,
    pub case_sensitive: bool,
    pub regex: bool,
    pub role: Option<RoleFilter>,
    pub session_id: Option<String>,
    pub include_internal: bool,
}

/// Field order mirrors `{ ...match, sessionTitle, projectKey }` in the TS source.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    pub session_id: String,
    pub role: Role,
    pub text: String,
    pub snippet: String,
    pub created_at: String,
    pub line_number: u64,
    pub session_title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub truncated: bool,
    pub sessions_scanned: usize,
}

/// An error that rejects the TS promise; `message` mirrors `error.message`.
#[derive(Debug, Clone)]
pub struct SearchError {
    pub message: String,
}

enum Matcher {
    Regex(Regex),
    Plain { needle: String, case_sensitive: bool },
}

impl Matcher {
    fn test(&self, text: &str) -> bool {
        match self {
            Matcher::Regex(re) => re.is_match(text),
            Matcher::Plain { needle, case_sensitive: true } => text.contains(needle.as_str()),
            Matcher::Plain { needle, .. } => text.to_lowercase().contains(needle.as_str()),
        }
    }

    /// UTF-16 index, or -1 when not found.
    fn find_index(&self, text: &str) -> i64 {
        match self {
            Matcher::Regex(re) => re.find(text).map(|m| js::byte_to_utf16(text, m.start()) as i64).unwrap_or(-1),
            Matcher::Plain { needle, case_sensitive: true } => {
                js::utf16_index_of(text, needle).map(|i| i as i64).unwrap_or(-1)
            }
            Matcher::Plain { needle, .. } => {
                js::utf16_index_of(&text.to_lowercase(), needle).map(|i| i as i64).unwrap_or(-1)
            }
        }
    }
}

fn build_matcher(query: &str, case_sensitive: bool, regex: bool) -> Result<Matcher, SearchError> {
    if regex {
        let re = RegexBuilder::new(query)
            .case_insensitive(!case_sensitive)
            .build()
            .map_err(|e| SearchError { message: format!("Invalid regular expression: /{query}/: {e}") })?;
        return Ok(Matcher::Regex(re));
    }
    let needle = if case_sensitive { query.to_string() } else { query.to_lowercase() };
    Ok(Matcher::Plain { needle, case_sensitive })
}

struct FileTarget {
    path: String,
    project_key: Option<String>,
}

fn is_internal_session(session_id: &str) -> bool {
    for kind in ["discovery", "workspace", "report"] {
        let prefix = format!("always-on-{kind}");
        if let Some(rest) = session_id.strip_prefix(&prefix) {
            if rest.starts_with(':') || rest.starts_with('-') {
                return true;
            }
        }
    }
    false
}

/// Sorted directory listing (Node/libuv `scandir` returns names sorted).
fn read_dir_names(dir: &str) -> Option<Vec<String>> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut names: Vec<String> =
        entries.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    Some(names)
}

fn list_jsonl_files(chat_dir: &str, project_key: &str, include_internal: bool) -> Vec<FileTarget> {
    let Some(names) = read_dir_names(chat_dir) else { return Vec::new() };
    names
        .into_iter()
        .filter_map(|name| {
            let session_id = name.strip_suffix(".jsonl")?;
            if !include_internal && is_internal_session(session_id) {
                return None;
            }
            Some(FileTarget { path: paths::join(&[chat_dir, &name]), project_key: Some(project_key.to_string()) })
        })
        .collect()
}

fn collect_session_files(input: &SearchInput) -> Vec<FileTarget> {
    if let Some(session_id) = &input.session_id {
        let project_root = input.project_root.clone().unwrap_or_else(paths::cwd);
        if !input.include_internal && is_internal_session(session_id) {
            return Vec::new();
        }
        let dir = paths::project_chat_dir(&project_root, &input.pilot_home);
        let name = format!("{}.jsonl", paths::sanitize_session_id_for_path(session_id));
        return vec![FileTarget { path: paths::join(&[&dir, &name]), project_key: Some(project_root) }];
    }
    if let Some(project_root) = &input.project_root {
        let dir = paths::project_chat_dir(project_root, &input.pilot_home);
        return list_jsonl_files(&dir, project_root, input.include_internal);
    }
    let Some(project_ids) = read_dir_names(&paths::resolve(&[&input.pilot_home, "projects"])) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for id in project_ids {
        let dir = paths::join(&[&input.pilot_home, "projects", &id, "chats"]);
        files.extend(list_jsonl_files(&dir, &id, input.include_internal));
    }
    files
}

fn is_record(v: &Value) -> bool {
    v.is_object()
}

fn extract_canonical_text(messages: &[&Value]) -> Option<String> {
    let mut parts = Vec::new();
    for message in messages {
        let Some(content) = message.as_object().and_then(|m| m.get("content")).and_then(|c| c.as_array()) else {
            continue;
        };
        for block in content {
            let Some(b) = block.as_object() else { continue };
            if b.get("type").and_then(|t| t.as_str()) != Some("text") {
                continue;
            }
            if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                let trimmed = js::js_trim(t);
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                }
            }
        }
    }
    if parts.is_empty() { None } else { Some(parts.join("\n")) }
}

fn extract_searchable(entry: &serde_json::Map<String, Value>) -> Option<(Role, String)> {
    let ty = entry.get("type").and_then(|t| t.as_str());
    if ty == Some("accepted_input") {
        if let Some(Value::Array(messages)) = entry.get("messages") {
            let refs: Vec<&Value> = messages.iter().collect();
            return extract_canonical_text(&refs).map(|t| (Role::User, t));
        }
    }
    if matches!(ty, Some("assistant_message") | Some("durable_message")) {
        if let Some(m) = entry.get("message").filter(|m| is_record(m)) {
            return extract_canonical_text(&[m]).map(|t| (Role::Assistant, t));
        }
    }
    None
}

fn build_snippet(text: &str, matcher: &Matcher) -> String {
    let normalized = js::collapse_whitespace(text);
    if normalized.is_empty() {
        return String::new();
    }
    let len = js::utf16_len(&normalized);
    let center = matcher.find_index(&normalized).max(0) as usize;
    let start = center.saturating_sub(SNIPPET_RADIUS);
    let end = len.min(center + SNIPPET_RADIUS);
    format!(
        "{}{}{}",
        if start > 0 { "..." } else { "" },
        js::utf16_slice(&normalized, start, end),
        if end < len { "..." } else { "" }
    )
}

/// JS `readline` line splitting (`\r\n`, `\n` or lone `\r`).
fn readline_lines(content: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let bytes = content.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                lines.push(&content[start..i]);
                i += 1;
                start = i;
            }
            b'\r' => {
                lines.push(&content[start..i]);
                i += if bytes.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < bytes.len() {
        lines.push(&content[start..]);
    }
    lines
}

struct RawMatch {
    session_id: String,
    role: Role,
    text: String,
    snippet: String,
    created_at: String,
    line_number: u64,
}

fn node_fs_error(err: &std::io::Error, path: &str) -> SearchError {
    let message = match err.raw_os_error() {
        Some(2) => format!("ENOENT: no such file or directory, open '{path}'"),
        Some(21) => "EISDIR: illegal operation on a directory, read".to_string(),
        Some(13) => format!("EACCES: permission denied, open '{path}'"),
        _ => format!("{err}, open '{path}'"),
    };
    SearchError { message }
}

fn search_file(file: &FileTarget, matcher: &Matcher, role_filter: RoleFilter) -> Result<Vec<RawMatch>, SearchError> {
    let bytes = std::fs::read(&file.path).map_err(|e| node_fs_error(&e, &file.path))?;
    let content = String::from_utf8_lossy(&bytes);
    let mut out = Vec::new();
    for (idx, line) in readline_lines(&content).into_iter().enumerate() {
        let line_number = idx as u64 + 1;
        if js::js_trim(line).is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else { continue };
        let obj = match &entry {
            Value::Null => {
                return Err(SearchError { message: "Cannot read properties of null (reading 'sessionId')".into() })
            }
            Value::Object(map) => Some(map),
            _ => None,
        };
        let get_str = |k: &str| obj.and_then(|o| o.get(k)).and_then(|v| v.as_str()).map(str::to_string);
        let session_id = get_str("sessionId").unwrap_or_else(|| file.path.clone());
        let created_at = get_str("createdAt").unwrap_or_default();
        let Some(obj) = obj else { continue };
        if let Some((role, text)) = extract_searchable(obj) {
            if let RoleFilter::Only(r) = role_filter {
                if r != role {
                    continue;
                }
            }
            if !matcher.test(&text) {
                continue;
            }
            let snippet = build_snippet(&text, matcher);
            out.push(RawMatch { session_id: session_id.clone(), role, text, snippet, created_at: created_at.clone(), line_number });
        }
    }
    Ok(out)
}

/// Rust equivalent of `searchNodeSessionHistory`.
pub fn search(input: &SearchInput) -> Result<SearchResult, SearchError> {
    let query = js::js_trim(&input.query).to_string();
    if query.is_empty() {
        return Ok(SearchResult { query, matches: Vec::new(), truncated: false, sessions_scanned: 0 });
    }
    let limit = input.limit.unwrap_or(DEFAULT_LIMIT).max(1.0).min(MAX_LIMIT);
    let matcher = build_matcher(&query, input.case_sensitive, input.regex)?;
    let role_filter = input.role.unwrap_or(RoleFilter::All);
    let files = collect_session_files(input);

    let mut titles: HashMap<String, String> = HashMap::new();
    for file in &files {
        let Some(name) = file.path.rsplit(['/', '\\']).next() else { continue };
        let session_id = name.strip_suffix(".jsonl").unwrap_or(name);
        if session_id.is_empty() {
            continue;
        }
        let info = read_session_title_info(&file.path).map_err(|e| SearchError { message: e.0 })?;
        if let Some(info) = info {
            titles.insert(format!("{}:{}", file.project_key.as_deref().unwrap_or(""), session_id), info.display_title());
        }
    }

    let mut matches = Vec::new();
    let mut truncated = false;
    'files: for file in &files {
        for m in search_file(file, &matcher, role_filter)? {
            let key = format!("{}:{}", file.project_key.as_deref().unwrap_or(""), m.session_id);
            let title = titles.get(&key).cloned().unwrap_or_else(|| m.session_id.clone());
            matches.push(SearchMatch {
                session_id: m.session_id,
                role: m.role,
                text: m.text,
                snippet: m.snippet,
                created_at: m.created_at,
                line_number: m.line_number,
                session_title: title,
                project_key: file.project_key.clone(),
            });
            if matches.len() as f64 >= limit {
                truncated = true;
                break 'files;
            }
        }
    }
    Ok(SearchResult { query, matches, truncated, sessions_scanned: files.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_sessions() {
        assert!(is_internal_session("always-on-report:x"));
        assert!(is_internal_session("always-on-workspace-1"));
        assert!(!is_internal_session("always-on-other-1"));
    }

    #[test]
    fn readline_splitting() {
        assert_eq!(readline_lines("a\r\nb\rc\n"), vec!["a", "b", "c"]);
        assert_eq!(readline_lines("a\n\nb"), vec!["a", "", "b"]);
    }
}
