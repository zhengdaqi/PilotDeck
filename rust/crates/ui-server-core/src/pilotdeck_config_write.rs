//! Write-side pure pipeline of ui/server/services/pilotdeckConfig.js: purgeLegacyProviderDrafts,
//! finalizeBootstrapPlaceholder, the in-memory part of writePilotDeckConfig (sanitize → purge → router sync →
//! bootstrap finalize → validation → YAML text), serializePilotDeckConfigResponse, configToYaml,
//! rawYamlToMaskedString, detectYamlIndent and valueAtPath.
//!
//! YAML text uses `pilotdeck_setup::yamlfmt::stringify`, the fuzz-verified `yaml@2` `stringify(v, { lineWidth: 0 })`
//! emulation. File I/O (reading, atomic replace, revision check) is the host's job.
use crate::js_number::{js_number_to_string, js_trim};
use crate::js_object::{array_index, field, js_order, truthy};
use crate::js_object_string;
use crate::model_references::find_model_references;
use crate::pilotdeck_config::*;
use pilotdeck_skills::jsjson::{stringify_compact, JsObject, JsValue};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub const BOOTSTRAP_PLACEHOLDER_KEY: &str = "PLACEHOLDER_RUN_ONBOARDING_TO_REPLACE";

/// JSON value → the JS object model used by the YAML/JSON writers.
pub fn to_js_value(v: &Value) -> JsValue {
    match v {
        Value::Null => JsValue::Null,
        Value::Bool(b) => JsValue::Bool(*b),
        Value::Number(n) => JsValue::Num(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => JsValue::Str(s.clone()),
        Value::Array(a) => JsValue::Arr(a.iter().map(to_js_value).collect()),
        Value::Object(m) => {
            let mut o = JsObject::new();
            for (k, x) in m {
                o.set(k.clone(), to_js_value(x));
            }
            JsValue::Obj(o)
        }
    }
}

/// `stringify(value, { lineWidth: 0 })` from the `yaml` package.
pub fn yaml_stringify(v: &Value) -> String {
    pilotdeck_setup::yamlfmt::stringify(&to_js_value(&js_order(v.clone())))
}

fn json_stringify(v: &Value) -> String {
    stringify_compact(&to_js_value(v))
}

fn is_bootstrap_placeholder_provider(id: &str, provider: &Value) -> bool {
    id == "_placeholder" || normalize_string(field(Some(provider), "apiKey")) == BOOTSTRAP_PLACEHOLDER_KEY
}

/// `purgeLegacyProviderDrafts(config, previousConfig)` (in place).
pub fn purge_legacy_provider_drafts(config: &mut Value, previous: Option<&Value>, ctx: &PilotDeckEnv) {
    let Some(Value::Object(prev)) = field(field(previous, "model"), "providers") else { return };
    let Some(Value::Object(providers)) = field(Some(config), "model").and_then(|m| m.get("providers")) else { return };
    let ids: Vec<String> = providers.keys().cloned().collect();
    for id in ids {
        let providers = &config["model"]["providers"];
        let Some(p @ Value::Object(pm)) = providers.get(&id) else { continue };
        let models_empty = match pm.get("models") {
            None => true,
            Some(Value::Object(m)) => m.is_empty(),
            _ => false,
        };
        let drop = models_empty
            && !allows_missing_api_key(&id)
            && resolve_configured_provider_api_key(&id, Some(p), ctx).is_empty()
            && matches!(prev.get(&id), Some(pp @ Value::Object(_)) if json_stringify(p) == json_stringify(pp))
            && find_model_references(config, &id, "").as_array().is_some_and(Vec::is_empty);
        if drop {
            config["model"]["providers"].as_object_mut().unwrap().shift_remove(&id);
        }
    }
}

/// `finalizeBootstrapPlaceholder(config)` (in place).
pub fn finalize_bootstrap_placeholder(config: &mut Value) {
    if !config.is_object() {
        return;
    }
    let agent_model = normalize_string(field(field(Some(config), "agent"), "model"));
    if agent_model.is_empty() {
        return;
    }
    match resolve_model(Some(config), Some(&Value::String(agent_model.clone())), true) {
        Ok(Some(sel)) if !is_bootstrap_placeholder_provider(&sel.provider_id, sel.provider) => {}
        _ => return,
    }
    let mut removed: HashSet<String> = HashSet::new();
    if let Some(Value::Object(providers)) = config.get_mut("model").and_then(|m| m.get_mut("providers")) {
        providers.retain(|id, p| {
            let drop = is_bootstrap_placeholder_provider(id, p);
            if drop {
                removed.insert(id.clone());
            }
            !drop
        });
    }
    if removed.is_empty() {
        return;
    }
    let refs_removed = |v: Option<&Value>| {
        let s = normalize_string(v);
        matches!(s.find('/'), Some(i) if i > 0 && removed.contains(&s[..i]))
    };
    let am = Value::String(agent_model.clone());
    let sub = normalize_string(field(field(field(Some(config), "agent"), "subagents"), "default"));
    if !sub.is_empty() && sub != "inherit" && refs_removed(Some(&Value::String(sub))) {
        config["agent"]["subagents"]["default"] = json!("inherit");
    }
    if refs_removed(field(field(Some(config), "memory"), "model")) {
        config["memory"]["model"] = json!("inherit");
    }
    let Some(Value::Object(router)) = config.get_mut("router") else { return };
    if let Some(Value::Object(sc)) = router.get_mut("scenarios") {
        for (_, v) in sc.iter_mut() {
            if refs_removed(Some(v)) {
                *v = am.clone();
            }
        }
    }
    if let Some(Value::Object(fb)) = router.get_mut("fallback") {
        for (_, refs) in fb.iter_mut() {
            if let Value::Array(a) = refs {
                for v in a.iter_mut() {
                    if refs_removed(Some(v)) {
                        *v = am.clone();
                    }
                }
            }
        }
    }
    if let Some(Value::Object(stats)) = router.get_mut("stats") {
        if let Some(Value::Object(mp)) = stats.get_mut("modelPricing") {
            mp.retain(|k, _| !refs_removed(Some(&Value::String(k.clone()))));
        }
        match stats.get_mut("baselineModel") {
            Some(b @ Value::String(_)) if refs_removed(Some(b)) => *b = am.clone(),
            Some(Value::Object(b)) if removed.contains(&normalize_string(b.get("provider"))) => {
                let (p, m) = split_model_ref(&agent_model).expect("selected agent model is a provider/model ref");
                let (p, m) = (p.to_string(), m.to_string());
                b.insert("provider".into(), json!(p));
                b.insert("model".into(), json!(m));
            }
            _ => {}
        }
    }
    if let Some(Value::Object(ts)) = router.get_mut("tokenSaver") {
        if refs_removed(ts.get("judge")) {
            ts.insert("judge".into(), am.clone());
        }
        if let Some(Value::Object(tiers)) = ts.get_mut("tiers") {
            for (_, t) in tiers.iter_mut() {
                if let Value::Object(t) = t {
                    if refs_removed(t.get("model")) {
                        t.insert("model".into(), am.clone());
                    }
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum WriteError {
    /// A JS TypeError from the pipeline (V8 message text).
    Js(JsError),
    /// `Error('Invalid PilotDeck config')` with `error.validation`.
    Invalid(Validation),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedWrite {
    /// YAML text to write atomically.
    pub raw: String,
    pub validation: Validation,
    /// `validation.config` after the blank `memory.model` cleanup (the object written).
    pub config: Value,
}

fn drop_blank_memory_model(v: &mut Value) {
    if let Some(Value::Object(m)) = v.get_mut("memory") {
        if matches!(m.get("model"), Some(Value::String(s)) if js_trim(s).is_empty()) {
            m.shift_remove("model");
        }
    }
}

/// In-memory part of `writePilotDeckConfig(config, { previousConfig })`. `previous` is `previousConfig` when it is a
/// record, otherwise the host passes the disk config (`readPilotDeckConfigFile().config` when it parsed; the
/// defaults when the file is missing).
pub fn prepare_pilotdeck_config_write(config: Option<&Value>, previous: Option<&Value>, ctx: &PilotDeckEnv) -> Result<PreparedWrite, WriteError> {
    let mut s = match config {
        None => return Err(WriteError::Js(JsError { name: "TypeError", message: "Cannot read properties of undefined (reading 'memory')".into() })),
        Some(Value::Null) => return Err(WriteError::Js(JsError { name: "TypeError", message: "Cannot read properties of null (reading 'memory')".into() })),
        Some(v) => v.clone(),
    };
    if s.is_object() {
        sanitize_provider_credentials(&mut s);
        purge_legacy_provider_drafts(&mut s, previous, ctx);
        sync_agent_model_with_router(&mut s);
        finalize_bootstrap_placeholder(&mut s);
        drop_blank_memory_model(&mut s);
    }
    let validation = validate_pilotdeck_config(Some(&s), ctx).map_err(WriteError::Js)?;
    if !validation.valid {
        return Err(WriteError::Invalid(validation));
    }
    let mut obj = validation.config.clone();
    drop_blank_memory_model(&mut obj);
    let mut validation = validation;
    validation.config = obj.clone();
    Ok(PreparedWrite { raw: yaml_stringify(&obj), validation, config: obj })
}

/// `configToYaml(config)`.
pub fn config_to_yaml(config: Option<&Value>, ctx: &PilotDeckEnv) -> Result<String, JsError> {
    Ok(yaml_stringify(&normalize_pilotdeck_config(config, ctx)?))
}

/// `rawYamlToMaskedString(rawYaml)`.
pub fn raw_yaml_to_masked_string(raw: Option<&Value>) -> String {
    match raw {
        Some(v @ Value::Object(_)) => yaml_stringify(&mask_secrets(v)),
        _ => yaml_stringify(&json!({})),
    }
}

/// The record `readPilotDeckConfigFile()` returns.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigFileRecord {
    pub exists: bool,
    pub config_path: String,
    pub raw: String,
    /// Always present for records read from disk; `None` mirrors an `undefined` field.
    pub config: Option<Value>,
    /// `null` (parse error) or the parsed document.
    pub raw_yaml: Value,
    pub parse_error: Option<String>,
}

/// `serializePilotDeckConfigResponse(record, reloadResult)`; `reload` is included when truthy.
pub fn serialize_pilotdeck_config_response(record: &ConfigFileRecord, reload: Option<&Value>, ctx: &PilotDeckEnv) -> Result<Value, JsError> {
    let mut o = Map::new();
    o.insert("exists".into(), json!(record.exists));
    o.insert("path".into(), json!(record.config_path));
    if let Some(pe) = record.parse_error.as_deref().filter(|p| !p.is_empty()) {
        o.insert("raw".into(), json!(record.raw));
        o.insert("revision".into(), json!(config_revision(Some(&json!(record.raw)))));
        if let Some(c) = &record.config {
            o.insert("config".into(), mask_secrets(c));
        }
        o.insert("configDisabled".into(), json!(true));
        o.insert("parseError".into(), json!(pe));
        o.insert("validation".into(), json!({ "valid": false, "errors": [format!("Invalid YAML: {pe}")], "warnings": [] }));
    } else {
        let v = validate_pilotdeck_config(record.config.as_ref(), ctx)?;
        let masked = record.config.as_ref().map(mask_secrets);
        let has_disk = match &record.raw_yaml {
            Value::Object(m) => !m.is_empty(),
            Value::Array(a) => !a.is_empty(),
            _ => false,
        };
        let raw = if has_disk { raw_yaml_to_masked_string(Some(&record.raw_yaml)) } else { config_to_yaml(masked.as_ref(), ctx)? };
        o.insert("raw".into(), json!(raw));
        o.insert("revision".into(), json!(config_revision(Some(&json!(record.raw)))));
        if let Some(m) = masked {
            o.insert("config".into(), m);
        }
        o.insert("validation".into(), json!({ "valid": v.valid, "errors": v.errors, "warnings": v.warnings }));
    }
    if truthy(reload) {
        o.insert("reload".into(), reload.unwrap().clone());
    }
    Ok(Value::Object(o))
}

/// `detectYamlIndent(raw)`.
pub fn detect_yaml_indent(raw: Option<&Value>) -> usize {
    let text = match raw {
        None | Some(Value::Null) => String::new(),
        Some(v) => js_object_string(v),
    };
    let min = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .filter(|l| !js_trim(l).is_empty() && !js_trim(l).starts_with('#'))
        .map(|l| l.bytes().take_while(|b| *b == b' ').count())
        .filter(|n| *n > 0)
        .min();
    min.map_or(2, |m| m.clamp(2, 8))
}

/// `valueAtPath(value, keys)` for string / number keys (`undefined` → `None`).
pub fn value_at_path<'a>(value: Option<&'a Value>, keys: &[Value]) -> Option<&'a Value> {
    let mut cur = value;
    for k in keys {
        let key = match k {
            Value::Number(n) => js_number_to_string(n.as_f64().unwrap_or(f64::NAN)),
            other => js_object_string(other),
        };
        cur = match cur {
            Some(Value::Object(m)) => m.get(&key),
            Some(Value::Array(a)) => array_index(&key).and_then(|i| a.get(i as usize)),
            _ => return None,
        };
    }
    cur
}
