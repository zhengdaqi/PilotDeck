//! Shared, framework-independent view model: reuses the migrated `tui-core` sidebar grouping
//! (the same code the TUI uses) to prove first-party logic can be shared with a Wasm UI.
use pilotdeck_tui_core::sidebar::group_sessions;
use serde_json::{json, Value};

pub fn sample_sessions() -> Vec<Value> {
    vec![
        json!({"sessionId": "s1", "summary": "alpha main", "cwd": "/p/alpha", "lastModified": 300, "tag": "running"}),
        json!({"sessionId": "s2", "summary": "alpha side", "cwd": "/p/alpha", "lastModified": 200, "tag": "idle"}),
        json!({"sessionId": "s3", "summary": "beta work", "cwd": "/p/beta", "lastModified": 250, "tag": "done"}),
    ]
}

/// (label, count) rows for the given grouping.
pub fn rows(group_by: &str) -> Vec<(String, usize)> {
    group_sessions(&sample_sessions(), group_by).into_iter().map(|g| (g.label, g.sessions.len())).collect()
}

pub fn toggle(group_by: &str) -> &'static str {
    if group_by == "project" { "status" } else { "project" }
}
