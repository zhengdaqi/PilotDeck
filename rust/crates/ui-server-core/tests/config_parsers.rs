//! Fixture: rust/parity/ui_server/gen_config_parsers.mjs (real TS modules under node).
use pilotdeck_ui_server_core::config_parsers::*;
use serde_json::{json, Value};

fn dec(v: &Value) -> Option<&Value> {
    if v.get("$undef").is_some() { None } else { Some(v) }
}
fn opt(v: Option<impl Into<Value>>) -> Value {
    v.map(Into::into).unwrap_or(json!({ "$undef": 1 }))
}

#[test]
fn config_parsers_match_ts() {
    let fx: Value = serde_json::from_str(include_str!("fixtures/config_parsers.json")).unwrap();
    for c in fx["scope"].as_array().unwrap() {
        let id = c["id"].as_str().unwrap();
        let (p, u) = (dec(&c["protocol"]), dec(&c["url"]));
        assert_eq!(opt(resolve_catalog_provider_default_url(id, p)), c["defaultUrl"], "{c}");
        assert_eq!(opt(resolve_catalog_provider_api_key_env_var(id, p, u)), c["envVar"], "{c}");
        let l = lookup_catalog_provider(id);
        assert_eq!(json!(l.map(|_| id)), c["lookup"], "{c}");
        for (k, f) in [("protocol", "lookupProtocol"), ("defaultUrl", "lookupDefaultUrl"), ("apiKeyEnvVar", "lookupEnv")] {
            assert_eq!(opt(l.and_then(|x| x.get(k)).cloned()), c[f], "{c}");
        }
    }
    for c in fx["thinking"].as_array().unwrap() {
        let got = match parse_thinking_settings(dec(&c["raw"]), c["protocol"].as_str().unwrap()) {
            Ok(v) => json!({ "v": opt(v) }),
            Err(m) => json!({ "err": "Error", "message": m }),
        };
        assert_eq!(got, c["out"], "{c}");
    }
    type P = fn(Option<&Value>, &mut Vec<Value>) -> Option<Value>;
    for (key, f) in [("gateway", parse_gateway_config as P), ("adapters", parse_adapters_config as P), ("tools", parse_tools_config as P)] {
        for c in fx[key].as_array().unwrap() {
            let mut d = vec![];
            let out = f(dec(&c["raw"]), &mut d);
            assert_eq!(opt(out), c["out"], "{key} {c}");
            assert_eq!(json!(d), c["d"], "{key} {c}");
        }
    }
}
