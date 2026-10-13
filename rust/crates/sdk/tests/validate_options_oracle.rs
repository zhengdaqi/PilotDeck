//! Oracle test against `rust/parity/sdk/gen_validate_options.mjs` (the real `validateOptions` from client.ts).
use pilotdeck_sdk::options::{validate_options, Js};
use serde_json::Value;

#[test]
fn validate_options_matches_ts() {
    let fx: Value = serde_json::from_str(include_str!("fixtures/validate_options.json")).unwrap();
    let cases = fx["cases"].as_array().unwrap();
    assert!(cases.len() > 9000, "{}", cases.len());
    let mut bad = vec![];
    for c in cases {
        let got = match validate_options(&Js::from_json(&c["options"])) {
            Ok(()) => "ok".to_string(),
            Err(e) => e.to_string(),
        };
        if got != c["result"].as_str().unwrap() {
            bad.push(format!("{}\n  ts:   {}\n  rust: {got}", c["options"], c["result"].as_str().unwrap()));
        }
    }
    assert!(bad.is_empty(), "{} of {} differ:\n{}", bad.len(), cases.len(), bad.iter().take(8).cloned().collect::<Vec<_>>().join("\n"));
}
