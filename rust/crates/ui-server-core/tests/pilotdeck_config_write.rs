//! Fixture: rust/parity/ui_server/gen_pilotdeck_config_write.mjs (real pilotdeckConfig.js; writePilotDeckConfig
//! runs against temp files and the written bytes are recorded).
use pilotdeck_ui_server_core::pilotdeck_config::*;
use pilotdeck_ui_server_core::pilotdeck_config_write::*;
use serde_json::{json, Value};

fn dec(v: &Value) -> Option<&Value> {
    if v.get("$undef").is_some() { None } else { Some(v) }
}
fn undef() -> Value {
    json!({ "$undef": 1 })
}
fn out<T: Into<Value>>(r: Result<T, JsError>) -> Value {
    match r {
        Ok(v) => json!({ "v": v.into() }),
        Err(e) => json!({ "err": e.name, "message": e.message }),
    }
}

#[test]
fn pilotdeck_config_write_matches_js() {
    let fx: Value = serde_json::from_str(include_str!("fixtures/pilotdeck_config_write.json")).unwrap();
    let base = PilotDeckEnv {
        home: fx["home"].as_str().unwrap().into(),
        env: [("PILOT_HOME".to_string(), fx["pilotHome"].as_str().unwrap().to_string())].into(),
    };
    for c in fx["cases"].as_array().unwrap() {
        let mut ctx = base.clone();
        for (k, v) in c["env"].as_object().unwrap() {
            ctx.env.insert(k.clone(), v.as_str().unwrap().into());
        }
        let (cfg, prev) = (dec(&c["cfg"]), dec(&c["prev"]));
        let mut p = cfg.cloned();
        if let Some(p) = p.as_mut() {
            if p.is_object() {
                purge_legacy_provider_drafts(p, prev, &ctx);
            }
        }
        assert_eq!(json!({ "v": p.unwrap_or(undef()) }), c["purge"], "purge {c}");
        let mut f = cfg.cloned();
        if let Some(f) = f.as_mut() {
            finalize_bootstrap_placeholder(f);
        }
        assert_eq!(json!({ "v": f.unwrap_or(undef()) }), c["finalize"], "finalize {c}");

        let disk_prev = if prev.is_some_and(Value::is_object) { prev.cloned() } else { Some(build_default_pilotdeck_config(&ctx)) };
        let w = match prepare_pilotdeck_config_write(cfg, disk_prev.as_ref(), &ctx) {
            Ok(w) => json!({ "v": { "raw": w.raw, "validation": w.validation.to_json(), "config": w.config, "disk": w.raw } }),
            Err(WriteError::Js(e)) => json!({ "err": e.name, "message": e.message }),
            Err(WriteError::Invalid(v)) => json!({ "err": "Error", "message": "Invalid PilotDeck config", "validation": v.to_json() }),
        };
        assert_eq!(w, c["write"], "write {c}");

        let r = &c["record"];
        let record = ConfigFileRecord {
            exists: r["exists"].as_bool().unwrap(),
            config_path: r["configPath"].as_str().unwrap().into(),
            raw: r["raw"].as_str().unwrap().into(),
            config: r.get("config").and_then(dec).cloned(),
            raw_yaml: r["rawYaml"].clone(),
            parse_error: r["parseError"].as_str().map(str::to_string),
        };
        assert_eq!(out(serialize_pilotdeck_config_response(&record, dec(&c["reload"]), &ctx)), c["serialize"], "serialize {c}");
        assert_eq!(out(config_to_yaml(cfg, &ctx)), c["toYaml"], "toYaml {c}");
        let ry = r.get("rawYaml").and_then(dec);
        assert_eq!(json!({ "v": raw_yaml_to_masked_string(ry) }), c["rawMasked"], "rawMasked {c}");
        assert_eq!(json!({ "v": detect_yaml_indent(dec(&c["yamlText"])) }), c["indent"], "indent {c}");
        let keys = c["vkeys"].as_array().unwrap();
        assert_eq!(json!({ "v": value_at_path(cfg, keys).cloned().unwrap_or(undef()) }), c["valueAt"], "valueAt {c}");
    }
}
