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

/// UI batch 2 (officePreviewStatus, ime, externalFileDrop, dateUtils). `date_parse` is the host's `Date.parse`:
/// in the browser it is checked against the TS-recorded value; natively the recorded value is used.
pub fn run_batch2(fixture: &str, date_parse: Option<&dyn Fn(&str) -> f64>) -> (usize, Vec<String>) {
    use crate::{date_utils as d, external_file_drop as x, ime, office_preview as o};
    let v: Value = serde_json::from_str(fixture).expect("fixture json");
    let mut n = 0;
    let mut bad = vec![];
    let mut check = |kind: &str, case: &Value, want: &Value, got: Value| {
        n += 1;
        if *want != got {
            bad.push(format!("{kind} {case} got {got}"));
        }
    };
    let opt = |c: &Value, k: &str, u: &str| if c[u].as_bool() == Some(true) { None } else { Some(c[k].clone()) };
    for c in v["office"].as_array().unwrap() {
        let (s, b) = (opt(c, "s", "sUndef"), opt(c, "b", "bUndef"));
        check("office.norm", c, &c["norm"], json!(o::normalize_office_preview_service(s.as_ref())));
        check("office.sig", c, &c["sig"], json!(o::office_preview_renderer_signature(s.as_ref(), b.as_ref())));
    }
    for c in v["officeConfig"].as_array().unwrap() {
        check("officeConfig", c, &c["out"], json!(o::office_preview_renderer_signature_from_config(&c["c"])));
    }
    for c in v["ime"].as_array().unwrap() {
        check("ime.comp", c, &c["comp"], json!(ime::is_ime_composition_event(&c["e"])));
        check("ime.enter", c, &c["enter"], json!(ime::is_ime_enter_event(&c["e"])));
    }
    for c in v["drag"].as_array().unwrap() {
        let dt = opt(c, "dt", "dtUndef");
        check("drag", c, &c["out"], json!(x::is_external_file_drag(dt.as_ref())));
    }
    for c in v["dropPath"].as_array().unwrap() {
        check("dropPath", c, &c["out"], json!(x::resolve_external_file_drop_target_path(c["p"].as_str(), c["t"].as_str(), c["r"].as_str().unwrap())));
    }
    for c in v["timeAgo"].as_array().unwrap() {
        let recorded = c["parsed"].as_f64().unwrap_or(f64::NAN);
        let parsed = match date_parse {
            Some(p) => {
                let host = p(c["s"].as_str().unwrap());
                check("Date.parse", c, &json!(c["parsed"]), if host.is_nan() { Value::Null } else { json!(host as i64) });
                host
            }
            None => recorded,
        };
        let t = d::format_time_ago(parsed, c["now"].as_f64().unwrap());
        let keyed = match &t {
            d::TimeAgo::Key(k, cnt) => json!([k, cnt]),
            d::TimeAgo::LocaleDate(ms) => json!({"localeDate": *ms as i64}),
        };
        check("timeAgo", c, &c["out"], keyed.clone());
        check("timeAgo.plain", c, &c["plain"], d::english(&t).map(Value::String).unwrap_or(keyed));
    }
    (n, bad)
}
