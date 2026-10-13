//! Fixture: rust/parity/ui_server/gen_pilotdeck_config.mjs (real pilotdeckConfig.js under node, controlled env).
use pilotdeck_ui_server_core::pilotdeck_config::*;
use serde_json::{json, Value};

fn dec(v: &Value) -> Option<&Value> {
    if v.get("$undef").is_some() { None } else { Some(v) }
}
fn undef() -> Value {
    json!({ "$undef": 1 })
}
fn out(r: Result<Value, JsError>) -> Value {
    match r {
        Ok(v) => json!({ "v": v }),
        Err(e) => json!({ "err": e.name, "message": e.message }),
    }
}

#[test]
fn pilotdeck_config_matches_js() {
    let fx: Value = serde_json::from_str(include_str!("fixtures/pilotdeck_config.json")).unwrap();
    let home = fx["home"].as_str().unwrap().to_string();
    let base = PilotDeckEnv { home: home.clone(), env: [("PILOT_HOME".to_string(), fx["pilotHome"].as_str().unwrap().to_string())].into() };
    assert_eq!(build_default_pilotdeck_config(&base), fx["defaults"]);
    for c in fx["cases"].as_array().unwrap() {
        let mut ctx = base.clone();
        for (k, v) in c["env"].as_object().unwrap() {
            ctx.env.insert(k.clone(), v.as_str().unwrap().into());
        }
        let cfg = dec(&c["cfg"]);
        assert_eq!(out(normalize_pilotdeck_config(cfg, &ctx)), c["normalize"], "normalize {c}");
        assert_eq!(out(validate_pilotdeck_config(cfg, &ctx).map(|v| v.to_json())), c["validate"], "validate {c}");
        let mut s = cfg.cloned();
        if let Some(s) = s.as_mut() {
            sanitize_provider_credentials(s);
        }
        assert_eq!(json!({ "v": s.unwrap_or(undef()) }), c["sanitize"], "sanitize {c}");
        let am = c["allowMissing"].as_bool().unwrap();
        let r = resolve_model(cfg, dec(&c["ref"]), am).map(|m| m.map(|m| m.to_json()).unwrap_or(Value::Null));
        assert_eq!(out(r), c["resolve"], "resolve {c}");
        assert_eq!(json!({ "v": cfg.map(mask_secrets).unwrap_or(undef()) }), c["mask"], "mask {c}");
        let edited = dec(&c["edited"]);
        assert_eq!(json!({ "v": preserve_masked_secrets(edited, cfg).unwrap_or(undef()) }), c["preserve"], "preserve {c}");
        assert_eq!(json!({ "v": has_unresolved_masked_secrets(edited) }), c["unresolved"], "unresolved {c}");
        let pid = c["pid"].as_str().unwrap();
        let prov = cfg.and_then(|c| c.get("model")).and_then(|m| m.get("providers")).and_then(|p| p.as_object()).and_then(|p| p.get(pid));
        assert_eq!(json!({ "v": resolve_configured_provider_url(pid, prov) }), c["url"], "url {c}");
        assert_eq!(json!({ "v": resolve_configured_provider_api_key(pid, prov, &ctx) }), c["apiKey"], "apiKey {c}");
        assert_eq!(out(build_runtime_env(cfg, &ctx)), c["runtimeEnv"], "runtimeEnv {c}");
        assert_eq!(out(build_memory_llm_options(cfg, &ctx).map(|v| v.unwrap_or(undef()))), c["memoryLlm"], "memoryLlm {c}");
        assert_eq!(out(build_memory_defaults(cfg, &ctx)), c["memoryDefaults"], "memoryDefaults {c}");
        let mut s = cfg.cloned();
        if let Some(s) = s.as_mut() {
            sync_agent_model_with_router(s);
        }
        assert_eq!(json!({ "v": s.unwrap_or(undef()) }), c["sync"], "sync {c}");
        assert_eq!(json!({ "v": config_revision(dec(&c["raw"])) }), c["revision"], "revision {c}");
        assert_eq!(json!({ "v": expand_tilde(dec(&c["tilde"]), &ctx) }), c["expand"], "expand {c}");
    }
}

#[test]
fn path_join_matches_node_posix() {
    for (parts, want) in [
        (&["/h", "a/../../b"][..], "/b"), (&["/h", ""], "/h"), (&["/h", "x/./y/"], "/h/x/y/"), (&["/h", "/a"], "/h/a"),
        (&["/", ".."], "/"), (&["a", "../.."], ".."), (&["", ""], "."), (&["a", ".."], "."), (&["a/", "."], "a"), (&["..", ".."], "../.."),
    ] {
        assert_eq!(path_join(parts), want, "{parts:?}");
    }
}
