//! Shared-vector runner: used by `cargo test` and by the browser self-test build (same code).
use crate::workspace_file_mention as w;
use serde_json::{json, Value};

/// Returns (checked, failures).
pub fn run_workspace_file_mention(fixture: &str) -> (usize, Vec<String>) {
    let v: Value = serde_json::from_str(fixture).expect("fixture json");
    let s = |x: &Value| x.as_str().unwrap_or("").to_string();
    let mut n = 0;
    let mut bad = vec![];
    let mut check = |kind: &str, case: &Value, got: Value| {
        n += 1;
        if case["out"] != got {
            bad.push(format!("{kind} {case} got {got}"));
        }
    };
    for c in v["mentionRequest"].as_array().unwrap() {
        check("mentionRequest", c, json!(w::is_workspace_file_mention_request(&c["v"])));
    }
    for c in v["has"].as_array().unwrap() {
        check("has", c, json!(w::has_workspace_file_mention(&s(&c["input"]), &s(&c["rel"]))));
    }
    for c in v["relative"].as_array().unwrap() {
        check("relative", c, json!(w::get_workspace_relative_path(&s(&c["f"]), &s(&c["r"]))));
    }
    for c in v["canonical"].as_array().unwrap() {
        check("canonical", c, json!(w::canonicalize_workspace_file_path(&s(&c["f"]), &s(&c["r"]))));
    }
    for c in v["identity"].as_array().unwrap() {
        check("identity", c, json!(w::get_workspace_file_identity(&s(&c["f"]), &s(&c["r"]))));
    }
    for c in v["atOrBelow"].as_array().unwrap() {
        check("atOrBelow", c, json!(w::is_workspace_path_at_or_below(&s(&c["c"]), &s(&c["p"]), &s(&c["r"]))));
    }
    for c in v["insert"].as_array().unwrap() {
        let r = w::insert_workspace_file_mention(&s(&c["input"]), &s(&c["rel"]), c["cur"].as_i64().unwrap());
        check("insert", c, json!({"input": r.input, "cursorPosition": r.cursor_position, "alreadyPresent": r.already_present}));
    }
    (n, bad)
}
