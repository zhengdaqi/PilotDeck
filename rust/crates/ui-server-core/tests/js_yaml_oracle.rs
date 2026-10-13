//! Oracle test for the js-yaml 3.14.2 loader port against `rust/parity/ui_server/gen_js_yaml.mjs`.
//! Values compare as serialized JSON strings (key order matters); errors compare js-yaml's reason text.
use pilotdeck_ui_server_core::js_yaml::safe_load;
use serde_json::{json, Value};

#[test]
fn js_yaml_safe_load_matches_ts() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/js_yaml.json")).unwrap();
    let (mut ok, mut err, mut bad) = (0, 0, vec![]);
    for c in &cases {
        let input = c["input"].as_str().unwrap();
        let got = match safe_load(input) {
            Ok(None) => json!({ "ok": "__undefined" }),
            Ok(Some(y)) => json!({ "ok": y.to_json() }),
            Err(e) => json!({ "err": e }),
        };
        if got.to_string() == c["out"].to_string() {
            if got.get("ok").is_some() { ok += 1 } else { err += 1 }
        } else {
            bad.push(format!("{input:?}\n ts {}\n rs {}", c["out"], got));
        }
    }
    eprintln!("ok {ok} err {err} bad {}", bad.len());
    assert!(bad.is_empty(), "{} mismatches:\n{}", bad.len(), bad.iter().take(15).cloned().collect::<Vec<_>>().join("\n"));
    assert!(ok > 1000 && err > 4000, "{ok} {err}");
}
