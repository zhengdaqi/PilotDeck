//! Port of `ui/src/utils/workspaceFileMention.ts`.
use crate::js::{is_js_ws, trim, utf16_len, utf16_to_byte};
use serde_json::Value;

pub const ADD_WORKSPACE_FILE_MENTION_EVENT: &str = "pilotdeck:add-workspace-file-mention";

pub fn is_workspace_file_mention_request(v: &Value) -> bool {
    let Some(o) = v.as_object() else { return false };
    let ok = |k: &str| o.get(k).and_then(Value::as_str).is_some_and(|s| !trim(s).is_empty());
    ok("projectName") && ok("relativePath")
}

fn normalize_path_syntax(v: &str) -> String {
    let n = v.replace('\\', "/");
    let b = n.as_bytes();
    if b.len() >= 4 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' && b[3] == b'/' {
        n[1..].to_string()
    } else {
        n
    }
}

fn drive_prefix(v: &str) -> bool {
    let b = v.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/'
}
fn is_windows_path(v: &str) -> bool {
    drive_prefix(v) || v.starts_with("//")
}
fn is_absolute(v: &str) -> bool {
    v.starts_with('/') || is_windows_path(v)
}
fn trim_trailing_slashes(v: &str) -> String {
    if v == "/" || (v.len() == 3 && drive_prefix(v)) {
        return v.to_string();
    }
    v.trim_end_matches('/').to_string()
}
fn normalize_relative(v: &str) -> Option<String> {
    let segs: Vec<&str> = v.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
    if segs.is_empty() || segs.contains(&"..") {
        return None;
    }
    Some(segs.join("/"))
}

/// `(?:^|\s)<path>(?=$|\s)`.
pub fn has_workspace_file_mention(input: &str, relative_path: &str) -> bool {
    if input.is_empty() || relative_path.is_empty() {
        return false;
    }
    let mut start = 0;
    while let Some(i) = input[start..].find(relative_path) {
        let at = start + i;
        let before_ok = at == 0 || input[..at].chars().next_back().is_some_and(is_js_ws);
        let end = at + relative_path.len();
        let after_ok = end == input.len() || input[end..].chars().next().is_some_and(is_js_ws);
        if before_ok && after_ok {
            return true;
        }
        start = at + input[at..].chars().next().map(char::len_utf8).unwrap_or(1);
    }
    false
}

pub fn get_workspace_relative_path(file_path: &str, workspace_root: &str) -> Option<String> {
    let f = trim_trailing_slashes(&normalize_path_syntax(file_path));
    let r = trim_trailing_slashes(&normalize_path_syntax(workspace_root));
    if f.is_empty() || r.is_empty() {
        return None;
    }
    if !is_absolute(&f) {
        return normalize_relative(&f);
    }
    let ci = is_windows_path(&r);
    let (cf, cr) = if ci { (f.to_lowercase(), r.to_lowercase()) } else { (f.clone(), r.clone()) };
    let prefix = if cr.ends_with('/') { cr } else { format!("{cr}/") };
    if !cf.starts_with(&prefix) {
        return None;
    }
    // TS slices the *original* path by the comparable prefix's UTF-16 length.
    let n = utf16_len(&prefix);
    let fu: Vec<u16> = f.encode_utf16().collect();
    let rel = String::from_utf16_lossy(&fu[n.min(fu.len())..]);
    normalize_relative(&rel)
}

pub fn canonicalize_workspace_file_path(file_path: &str, workspace_root: &str) -> String {
    let f = trim_trailing_slashes(&normalize_path_syntax(trim(file_path)));
    if f.is_empty() {
        return String::new();
    }
    if !workspace_root.is_empty() {
        if let Some(r) = get_workspace_relative_path(&f, workspace_root).filter(|r| !r.is_empty()) {
            return r;
        }
    }
    if !is_absolute(&f) {
        return normalize_relative(&f).filter(|r| !r.is_empty()).unwrap_or(f);
    }
    f
}

pub fn get_workspace_file_identity(file_path: &str, workspace_root: &str) -> String {
    let c = canonicalize_workspace_file_path(file_path, workspace_root);
    let r = normalize_path_syntax(workspace_root);
    if is_windows_path(&r) || is_windows_path(&c) {
        c.to_lowercase()
    } else {
        c
    }
}

pub fn is_workspace_path_at_or_below(candidate: &str, parent: &str, workspace_root: &str) -> bool {
    let c = get_workspace_file_identity(candidate, workspace_root);
    let p = get_workspace_file_identity(parent, workspace_root);
    if c.is_empty() || p.is_empty() {
        return false;
    }
    c == p || c.starts_with(&format!("{p}/"))
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileMentionInsertion {
    pub input: String,
    /// UTF-16 offset.
    pub cursor_position: i64,
    pub already_present: bool,
}

pub fn insert_workspace_file_mention(input: &str, relative_path: &str, requested: i64) -> FileMentionInsertion {
    let len = utf16_len(input) as i64;
    let cursor = requested.min(len).max(0);
    if has_workspace_file_mention(input, relative_path) {
        return FileMentionInsertion { input: input.into(), cursor_position: cursor, already_present: true };
    }
    let b = utf16_to_byte(input, cursor as usize);
    let (before, after) = input.split_at(b);
    let lead = if !before.is_empty() && !before.chars().next_back().is_some_and(is_js_ws) { " " } else { "" };
    let trail = if !after.is_empty() && after.chars().next().is_some_and(is_js_ws) { "" } else { " " };
    let ins = format!("{lead}{relative_path}{trail}");
    FileMentionInsertion {
        input: format!("{before}{ins}{after}"),
        cursor_position: (utf16_len(before) + utf16_len(&ins)) as i64,
        already_present: false,
    }
}
