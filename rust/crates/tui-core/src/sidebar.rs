//! `app/sidebar-helpers.ts`. Sessions are `GatewaySessionInfo` JSON objects.
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionGroup {
    #[serde(rename = "groupKey")]
    pub group_key: String,
    pub label: String,
    pub sessions: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SidebarRow {
    Header {
        #[serde(rename = "groupKey")]
        group_key: String,
        label: String,
        count: usize,
        collapsed: bool,
    },
    Session {
        session: Value,
        #[serde(rename = "groupKey")]
        group_key: String,
    },
}

fn opt_str<'a>(s: &'a Value, k: &str) -> Option<&'a str> {
    s.get(k).and_then(Value::as_str)
}

/// Insertion-ordered grouping (JS Map semantics).
fn group_by(sessions: &[Value], key: impl Fn(&Value) -> String) -> Vec<(String, Vec<Value>)> {
    let mut out: Vec<(String, Vec<Value>)> = Vec::new();
    for s in sessions {
        let k = key(s);
        match out.iter_mut().find(|(g, _)| *g == k) {
            Some((_, l)) => l.push(s.clone()),
            None => out.push((k, vec![s.clone()])),
        }
    }
    out
}

pub fn group_by_project(sessions: &[Value]) -> Vec<SessionGroup> {
    let mut groups: Vec<SessionGroup> = group_by(sessions, |s| opt_str(s, "cwd").unwrap_or("(unknown)").to_string())
        .into_iter()
        .map(|(k, l)| SessionGroup { label: k.rsplit('/').next().unwrap_or(&k).to_string(), group_key: k, sessions: l })
        .collect();
    let t = |g: &SessionGroup| g.sessions.iter().map(|s| s.get("lastModified").and_then(Value::as_f64).unwrap_or(0.0)).fold(f64::NEG_INFINITY, f64::max);
    groups.sort_by(|a, b| t(b).partial_cmp(&t(a)).unwrap_or(std::cmp::Ordering::Equal));
    groups
}

const ORDER: [&str; 6] = ["running", "waiting", "background", "idle", "done", "error"];

fn status_label(k: &str) -> String {
    match k {
        "running" => "Running",
        "waiting" => "Waiting",
        "idle" => "Idle",
        "done" => "Done",
        "error" => "Error",
        "background" => "Background",
        _ => k,
    }
    .to_string()
}

pub fn group_by_status(sessions: &[Value]) -> Vec<SessionGroup> {
    let mut groups: Vec<SessionGroup> = group_by(sessions, |s| opt_str(s, "tag").unwrap_or("idle").to_string())
        .into_iter()
        .map(|(k, l)| SessionGroup { group_key: format!("status:{k}"), label: status_label(&k), sessions: l })
        .collect();
    let rank = |g: &SessionGroup| {
        let k = g.group_key.replacen("status:", "", 1);
        ORDER.iter().position(|o| *o == k).unwrap_or(99)
    };
    groups.sort_by_key(rank);
    groups
}

pub fn group_sessions(sessions: &[Value], by: &str) -> Vec<SessionGroup> {
    if by == "project" { group_by_project(sessions) } else { group_by_status(sessions) }
}

fn session_key(s: &Value) -> Option<&str> {
    match s.get("sessionKey") {
        Some(Value::Null) | None => opt_str(s, "sessionId"),
        Some(v) => v.as_str(),
    }
}

pub fn compute_smart_collapse(groups: &[SessionGroup], active: &str) -> BTreeSet<String> {
    let mut c = BTreeSet::new();
    if groups.len() <= 1 {
        return c;
    }
    for g in groups {
        if !g.sessions.iter().any(|s| session_key(s) == Some(active)) {
            c.insert(g.group_key.clone());
        }
    }
    c
}

pub fn flatten_sidebar_rows(groups: &[SessionGroup], collapsed: &BTreeSet<String>) -> Vec<SidebarRow> {
    if groups.len() == 1 {
        return groups[0].sessions.iter().map(|s| SidebarRow::Session { session: s.clone(), group_key: groups[0].group_key.clone() }).collect();
    }
    let mut rows = Vec::new();
    for g in groups {
        let is_c = collapsed.contains(&g.group_key);
        rows.push(SidebarRow::Header { group_key: g.group_key.clone(), label: g.label.clone(), count: g.sessions.len(), collapsed: is_c });
        if !is_c {
            rows.extend(g.sessions.iter().map(|s| SidebarRow::Session { session: s.clone(), group_key: g.group_key.clone() }));
        }
    }
    rows
}

pub fn session_status_icon(s: &Value) -> &'static str {
    match opt_str(s, "tag").unwrap_or("idle") {
        "running" => "✻",
        "waiting" => "✽",
        "done" => "✓",
        "error" => "✗",
        "background" => "■",
        _ => "∙",
    }
}

pub fn session_display_title(s: &Value, max_len: usize) -> String {
    let raw = ["customTitle", "aiTitle", "firstPrompt", "summary"]
        .iter()
        .filter_map(|k| opt_str(s, k))
        .find(|v| !v.is_empty())
        .or_else(|| opt_str(s, "sessionId"))
        .unwrap_or("");
    let units: Vec<u16> = raw.encode_utf16().collect();
    if units.len() <= max_len {
        return raw.to_string();
    }
    format!("{}…", String::from_utf16_lossy(&units[..max_len.saturating_sub(1)]))
}

pub fn relative_time(now_ms: f64, ts: f64) -> String {
    let diff = now_ms - ts;
    if diff < 0.0 {
        return "now".into();
    }
    let seconds = (diff / 1000.0).floor();
    if seconds < 60.0 {
        return format!("{seconds}s");
    }
    let minutes = (seconds / 60.0).floor();
    if minutes < 60.0 {
        return format!("{minutes}m");
    }
    let hours = (minutes / 60.0).floor();
    if hours < 24.0 {
        return format!("{hours}h");
    }
    format!("{}d", (hours / 24.0).floor())
}
