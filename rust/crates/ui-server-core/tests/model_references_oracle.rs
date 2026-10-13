//! Differential test against ui/server/services/modelReferences.js (rust/parity/ui_server/gen_model_references.mjs).
use pilotdeck_ui_server_core::model_references::*;
use serde_json::{json, Value};

fn pairs(v: &Value) -> Vec<(String, Value)> {
    v.as_array().unwrap().iter().map(|p| (p[0].as_str().unwrap().to_string(), p[1].clone())).collect()
}

#[test]
fn model_references_match_js() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/model_references.json")).unwrap();
    let mut bad = vec![];
    for (i, c) in cases.iter().enumerate() {
        let s = |v: &Value| v.as_str().unwrap_or("").to_string();
        let err = |e: String| json!({"__error": e});
        let got = match c["fn"].as_str().unwrap() {
            "find" => find_model_references(&c["config"], &s(&c["target"]["providerId"]), &s(&c["target"]["modelId"])),
            "plan" => plan_model_removal(&c["config"], &s(&c["target"]["providerId"]), &s(&c["target"]["modelId"]), &s(&c["opts"]["replacement"]))
                .unwrap_or_else(err),
            "rewrite" => rewrite_model_references(&c["config"], &Renames { providers: pairs(&c["providerRenames"]), models: pairs(&c["modelRenames"]) })
                .unwrap_or_else(err),
            "configured" => configured_model_ids(&c["config"]),
            "actions" => json!(REFERENCE_ACTIONS),
            f => panic!("{f}"),
        };
        // Byte-exact: key order matters (JSON.stringify output).
        if serde_json::to_string(&got).unwrap() != serde_json::to_string(&c["out"]).unwrap() {
            bad.push(format!("#{i} {} target={} opts={}\n  want {}\n  got  {}", c["fn"], c["target"], c["opts"], c["out"], got));
        }
    }
    eprintln!("ui-server modelReferences oracle: {} cases, {} mismatches", cases.len(), bad.len());
    for b in bad.iter().take(4) {
        eprintln!("{}", &b[..b.len().min(1500)]);
    }
    assert!(bad.is_empty());
}
