//! Port of the pure part of ui/server/services/pilotdeckConfig.js (defaults, normalization/migration,
//! credential sanitizing, model resolution, validation, secret masking, runtime env, memory options,
//! configRevision, expandTilde, syncAgentModelWithRouter).
//!
//! Configs are the JSON view of the YAML object. `process.env`, `os.homedir()` and `PILOT_HOME` are passed
//! explicitly through [`PilotDeckEnv`]. JS exceptions become [`JsError`] with V8's message text.
use crate::config_parsers::{
    lookup_catalog_provider, parse_gateway_config, parse_thinking_settings, parse_tools_config,
    resolve_catalog_provider_api_key_env_var, resolve_catalog_provider_default_url,
};
use crate::js_number::{js_number_to_string, js_trim};
use crate::js_object::{create_property_error, field, is_record, js_order, truthy};
use crate::js_object_string;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

pub const MASK: &str = "********";
const CATALOG_PROVIDER_DEFAULT_URLS: [&str; 12] = [
    "anthropic", "openai", "openai-responses", "dashscope", "deepseek", "google", "moonshot", "minimax", "volc_ark",
    "zhipu", "openrouter", "ollama",
];

/// The process state the JS module reads: `process.env`, `os.homedir()` and `PILOT_HOME_DIR`.
#[derive(Clone, Debug, Default)]
pub struct PilotDeckEnv {
    pub home: String,
    pub env: HashMap<String, String>,
}

impl PilotDeckEnv {
    /// `process.env.PILOT_HOME || path.join(os.homedir(), '.pilotdeck')` (captured at module load in JS).
    pub fn pilot_home_dir(&self) -> String {
        match self.env.get("PILOT_HOME") {
            Some(v) if !v.is_empty() => v.clone(),
            _ => path_join(&[&self.home, ".pilotdeck"]),
        }
    }
    fn get(&self, k: &str) -> Option<&str> {
        self.env.get(k).map(String::as_str)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JsError {
    pub name: &'static str,
    pub message: String,
}

fn type_error(message: String) -> JsError {
    JsError { name: "TypeError", message }
}
fn error(message: String) -> JsError {
    JsError { name: "Error", message }
}

/// POSIX `path.join` (join non-empty segments with '/', then `path.normalize`).
pub fn path_join(parts: &[&str]) -> String {
    let joined = parts.iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join("/");
    if joined.is_empty() {
        return ".".into();
    }
    let abs = joined.starts_with('/');
    let trailing = joined.ends_with('/');
    let mut out: Vec<&str> = vec![];
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|l| *l != "..") {
                    out.pop();
                } else if !abs {
                    out.push("..");
                }
            }
            s => out.push(s),
        }
    }
    let mut s = out.join("/");
    if s.is_empty() && !abs {
        s = ".".into();
    }
    if !s.is_empty() && trailing {
        s.push('/');
    }
    if abs { format!("/{s}") } else { s }
}

fn normalize_string(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => js_trim(s).to_string(),
        _ => String::new(),
    }
}

fn deep_merge(base: &Value, over: &Value) -> Value {
    let Value::Object(b) = base else { return over.clone() };
    let mut out = b.clone();
    if let Value::Object(o) = over {
        for (k, v) in o {
            let merged = match (v, out.get(k)) {
                (Value::Object(_), Some(cur @ Value::Object(_))) => deep_merge(cur, v),
                _ => v.clone(),
            };
            out.insert(k.clone(), merged);
        }
    }
    Value::Object(out)
}

pub fn build_default_pilotdeck_config(ctx: &PilotDeckEnv) -> Value {
    json!({
        "schemaVersion": 1,
        "agent": { "model": "", "params": {}, "subagents": { "default": "inherit", "params": {} } },
        "model": { "providers": {} },
        "memory": {
            "enabled": false, "reasoningMode": "answer_first", "autoIndexIntervalMinutes": 30,
            "autoDreamIntervalMinutes": 60, "captureStrategy": "last_turn", "includeAssistant": true,
            "maxMessageChars": 6000, "heartbeatBatchSize": 30,
        },
        "router": { "enabled": false },
        "tools": { "webSearch": { "enabled": false } },
        "alwaysOn": { "projects": {} },
        "adapters": { "feishu": { "enabled": false }, "weixin": { "enabled": false }, "wecom": { "enabled": false } },
        "webui": {
            "runtime": {
                "host": "0.0.0.0", "serverPort": 3001, "vitePort": 5173, "apiTimeoutMs": 120000,
                "databasePath": path_join(&[&ctx.pilot_home_dir(), "auth.db"]), "workspacesRoot": ctx.home,
            },
            "officePreview": { "service": "builtin", "binaryPath": "" },
        },
        "telemetry": { "enabled": false },
    })
}

/// `normalizePilotDeckConfig`. Errors reproduce the TypeErrors thrown when `webui` / `webui.officePreview`
/// are overridden by non-objects.
pub fn normalize_pilotdeck_config(input: Option<&Value>, ctx: &PilotDeckEnv) -> Result<Value, JsError> {
    let empty = json!({});
    let source = if is_record(input) { input.unwrap() } else { &empty };
    let mut n = deep_merge(&build_default_pilotdeck_config(ctx), source);
    for key in ["memory", "router"] {
        if let Some(Value::Object(s)) = source.get(key) {
            if !s.contains_key("enabled") {
                n[key]["enabled"] = json!(true);
            }
        }
    }
    if let Some(Value::Object(ws)) = field(source.get("tools"), "webSearch") {
        if !ws.contains_key("enabled") {
            n["tools"]["webSearch"]["enabled"] = json!(true);
        }
    }
    let src_op = field(source.get("webui"), "officePreview").filter(|v| v.is_object());
    let legacy = normalize_string(field(src_op, "spreadsheetMode")).to_lowercase();
    let configured = normalize_string(field(src_op, "service")).to_lowercase();
    let service = if !legacy.is_empty() {
        if legacy == "print" && configured == "libreoffice" { "libreoffice".to_string() } else { "builtin".to_string() }
    } else if configured.is_empty() || configured == "none" {
        "builtin".into()
    } else {
        configured
    };
    let webui = n.get_mut("webui").expect("default webui");
    let op = match webui {
        Value::Null => return Err(type_error("Cannot read properties of null (reading 'officePreview')".into())),
        Value::Object(m) => m.get_mut("officePreview"),
        _ => None,
    };
    match op {
        Some(Value::Object(m)) => {
            m.insert("service".into(), Value::String(service));
            m.shift_remove("spreadsheetMode");
        }
        None => return Err(type_error("Cannot set properties of undefined (setting 'service')".into())),
        Some(Value::Null) => return Err(type_error("Cannot set properties of null (setting 'service')".into())),
        Some(Value::Array(_)) => unreachable!("array-valued officePreview is outside the ported domain"),
        Some(other) => {
            let msg = create_property_error("service", other);
            return Err(type_error(msg.trim_start_matches("TypeError: ").to_string()));
        }
    }
    Ok(js_order(n))
}

fn allows_missing_api_key(id: &str) -> bool {
    id == "ollama"
}

/// `sanitizeProviderCredentials` (in place).
pub fn sanitize_provider_credentials(config: &mut Value) {
    let Some(Value::Object(providers)) = config.get_mut("model").and_then(|m| m.get_mut("providers")) else { return };
    for (id, p) in providers.iter_mut() {
        let Value::Object(p) = p else { continue };
        if let Some(Value::String(k)) = p.get("apiKey") {
            let t = js_trim(k).to_string();
            if allows_missing_api_key(id) && t.is_empty() {
                p.shift_remove("apiKey");
            } else {
                p.insert("apiKey".into(), Value::String(t));
            }
        }
        if let Some(Value::String(u)) = p.get("url") {
            let t = js_trim(u).to_string();
            p.insert("url".into(), Value::String(t));
        }
    }
}

fn split_model_ref(text: &str) -> Option<(&str, &str)> {
    let text = js_trim(text);
    if text.is_empty() {
        return None;
    }
    let slash = text.find('/')?;
    if slash == 0 || slash == text.len() - 1 {
        return None;
    }
    Some((&text[..slash], &text[slash + 1..]))
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedModel<'a> {
    pub id: String,
    pub provider_id: String,
    pub provider: &'a Value,
    pub model: String,
    pub def: Value,
}

impl ResolvedModel<'_> {
    pub fn to_json(&self) -> Value {
        json!({ "id": self.id, "providerId": self.provider_id, "provider": self.provider, "model": self.model, "def": self.def })
    }
}

/// `resolveModel(config, ref, { allowMissing })`: `Ok(None)` only when `allow_missing`.
pub fn resolve_model<'a>(config: Option<&'a Value>, r: Option<&Value>, allow_missing: bool) -> Result<Option<ResolvedModel<'a>>, JsError> {
    let inherit = normalize_string(field(field(config, "agent"), "model"));
    let ref_text = normalize_string(r);
    let effective = if ref_text.is_empty() || ref_text == "inherit" { inherit } else { ref_text };
    let fail = |m: String| if allow_missing { Ok(None) } else { Err(error(m)) };
    let Some((pid, mid)) = split_model_ref(&effective) else {
        let shown = match r {
            None | Some(Value::Null) => String::new(),
            Some(v) => js_object_string(v),
        };
        return fail(format!("Invalid model reference: {shown}"));
    };
    let provider = field(field(field(config, "model"), "providers"), pid);
    let Some(provider @ Value::Object(pm)) = provider else {
        return fail(format!("Provider not found for model \"{effective}\": {pid}"));
    };
    let empty = Map::new();
    let models = match pm.get("models") {
        Some(Value::Object(m)) => m,
        _ => &empty,
    };
    let Some(raw) = models.get(mid) else {
        return fail(format!("Model not found for provider \"{pid}\": {mid}"));
    };
    if !raw.is_null() && !raw.is_object() {
        return fail(format!("Model definition for provider \"{pid}\" must be an object: {mid}"));
    }
    Ok(Some(ResolvedModel {
        id: effective.clone(),
        provider_id: pid.into(),
        provider,
        model: mid.into(),
        def: if raw.is_object() { raw.clone() } else { json!({}) },
    }))
}

fn resolves(config: &Value, r: &str) -> bool {
    matches!(resolve_model(Some(config), Some(&Value::String(r.into())), true), Ok(Some(_)))
}

pub fn resolve_configured_provider_url(id: &str, provider: Option<&Value>) -> String {
    let configured = normalize_string(field(provider, "url"));
    if !configured.is_empty() {
        return configured;
    }
    let protocol = provider_protocol(id, provider);
    resolve_catalog_provider_default_url(id, protocol.as_ref()).unwrap_or_default()
}

fn provider_protocol(id: &str, provider: Option<&Value>) -> Option<Value> {
    let p = normalize_string(field(provider, "protocol"));
    if !p.is_empty() {
        return Some(Value::String(p));
    }
    lookup_catalog_provider(id).and_then(|c| c.get("protocol")).cloned()
}

fn env_reference(s: &str) -> Option<&str> {
    let name = s.strip_prefix("${")?.strip_suffix('}')?;
    let mut chars = name.chars();
    let first = chars.next()?;
    ((first.is_ascii_alphabetic() || first == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')).then_some(name)
}

pub fn resolve_configured_provider_api_key(id: &str, provider: Option<&Value>, ctx: &PilotDeckEnv) -> String {
    let env_value = |name: &str| ctx.get(name).map(|v| js_trim(v).to_string()).unwrap_or_default();
    let configured = normalize_string(field(provider, "apiKey"));
    if !configured.is_empty() {
        return match env_reference(&configured) {
            Some(name) => env_value(name),
            None => configured,
        };
    }
    let protocol = provider_protocol(id, provider);
    let url = Value::String(resolve_configured_provider_url(id, provider));
    match resolve_catalog_provider_api_key_env_var(id, protocol.as_ref(), Some(&url)) {
        Some(name) => env_value(&name),
        None => String::new(),
    }
}

fn valid_http_url(s: &str) -> bool {
    matches!(url::Url::parse(s), Ok(u) if u.scheme() == "http" || u.scheme() == "https")
}

fn validate_provider(id: &str, provider: &Value, errors: &mut Vec<String>, ctx: &PilotDeckEnv) {
    let Value::Object(p) = provider else {
        errors.push(format!("model.providers.{id} must be an object"));
        return;
    };
    let protocol = normalize_string(p.get("protocol")).to_lowercase();
    if protocol.is_empty() {
        errors.push(format!("model.providers.{id}.protocol is required"));
    } else if !["openai", "openai-responses", "anthropic", "google"].contains(&protocol.as_str()) {
        errors.push(format!("model.providers.{id}.protocol must be \"openai\", \"openai-responses\", \"anthropic\", or \"google\""));
    }
    if normalize_string(p.get("url")).is_empty() && !CATALOG_PROVIDER_DEFAULT_URLS.contains(&id) {
        errors.push(format!("model.providers.{id}.url is required"));
    }
    let url = resolve_configured_provider_url(id, Some(provider));
    if !url.is_empty() && !valid_http_url(&url) {
        errors.push(format!("model.providers.{id}.url must be a valid HTTP or HTTPS URL"));
    }
    if !allows_missing_api_key(id) && resolve_configured_provider_api_key(id, Some(provider), ctx).is_empty() {
        errors.push(format!("model.providers.{id}.apiKey is required"));
    }
    match p.get("models") {
        Some(Value::Object(models)) => {
            for (mid, m) in models {
                if js_trim(mid).is_empty() {
                    errors.push(format!("model.providers.{id}.models contains an empty model id"));
                }
                if !m.is_null() && !m.is_object() {
                    errors.push(format!("model.providers.{id}.models.{mid} must be an object"));
                }
                if m.is_object() {
                    if let Err(e) = parse_thinking_settings(m.get("thinking"), &protocol) {
                        errors.push(format!("model.providers.{id}.models.{mid}: {e}"));
                    }
                }
            }
        }
        _ => errors.push(format!("model.providers.{id}.models must be an object")),
    }
}

fn validate_model_ref(config: &Value, r: Option<&Value>, label: &str, errors: &mut Vec<String>) {
    let m = normalize_string(r);
    if m.is_empty() {
        return;
    }
    if !resolves(config, &m) {
        errors.push(format!("{label}=\"{m}\" doesn't resolve to a configured provider/model"));
    }
}

fn validate_required_model_ref(config: &Value, r: Option<&Value>, label: &str, errors: &mut Vec<String>) {
    let m = normalize_string(r);
    if !matches!(r, Some(Value::String(_))) || m.is_empty() || split_model_ref(&m).is_none() {
        errors.push(format!("{label} must use provider/model format"));
        return;
    }
    validate_model_ref(config, r, label, errors);
}

fn validate_baseline_model_ref(config: &Value, r: Option<&Value>, errors: &mut Vec<String>) {
    let label = "router.stats.baselineModel";
    match r {
        None => {}
        Some(v @ Value::String(s)) => {
            if split_model_ref(s).is_none() {
                errors.push(format!("{label} must use provider/model format"));
                return;
            }
            validate_model_ref(config, Some(v), label, errors);
        }
        Some(Value::Object(o)) => {
            let (p, m) = (normalize_string(o.get("provider")), normalize_string(o.get("model")));
            if p.is_empty() || m.is_empty() {
                errors.push(format!("{label} must contain provider and model"));
                return;
            }
            validate_model_ref(config, Some(&Value::String(format!("{p}/{m}"))), label, errors);
        }
        Some(_) => errors.push(format!("{label} must be an object with provider and model")),
    }
}

fn validate_router_pricing(config: &Value, stats: Option<&Value>, errors: &mut Vec<String>) {
    let Some(stats) = stats else { return };
    let Value::Object(stats) = stats else {
        errors.push("router.stats must be an object".into());
        return;
    };
    if stats.get("enabled").is_some_and(|e| !e.is_boolean()) {
        errors.push("router.stats.enabled must be a boolean".into());
    }
    let Some(mp) = stats.get("modelPricing") else { return };
    let Value::Object(mp) = mp else {
        errors.push("router.stats.modelPricing must be an object keyed by provider/model".into());
        return;
    };
    for (key, pricing) in mp {
        let label = format!("router.stats.modelPricing.{key}");
        if split_model_ref(key).is_none() || !resolves(config, key) {
            errors.push(format!("{label} must reference a configured provider/model"));
        }
        let Value::Object(pr) = pricing else {
            errors.push(format!("{label} must be an object"));
            continue;
        };
        for f in ["input", "output", "cacheRead"] {
            if let Some(v) = pr.get(f) {
                if !v.as_f64().is_some_and(|x| x.is_finite() && x >= 0.0) || !v.is_number() {
                    errors.push(format!("{label}.{f} must be a finite non-negative number"));
                }
            }
        }
        if pr.get("unit").is_some_and(|u| !matches!(u.as_str(), Some("$/百万 Token" | "¥/百万 Token"))) {
            errors.push(format!("{label}.unit must be one of $/百万 Token or ¥/百万 Token"));
        }
    }
}

fn validate_router_model_refs(config: &Value, errors: &mut Vec<String>) {
    let Some(Value::Object(router)) = config.get("router") else { return };
    if router.get("enabled") == Some(&Value::Bool(false)) {
        return;
    }
    validate_baseline_model_ref(config, field(router.get("stats"), "baselineModel"), errors);
    if router.get("enabled").is_some_and(|e| !e.is_boolean()) {
        errors.push("router.enabled must be a boolean".into());
    }
    if let Some(Value::Object(sc)) = router.get("scenarios") {
        for (k, r) in sc {
            validate_required_model_ref(config, Some(r), &format!("router.scenarios.{k}"), errors);
        }
    }
    if let Some(Value::Object(fb)) = router.get("fallback") {
        for (k, refs) in fb {
            let Value::Array(refs) = refs else { continue };
            for (i, r) in refs.iter().enumerate() {
                validate_model_ref(config, Some(r), &format!("router.fallback.{k}[{i}]"), errors);
            }
        }
    }
    validate_router_pricing(config, router.get("stats"), errors);
    let Some(Value::Object(ts)) = router.get("tokenSaver") else { return };
    if ts.get("enabled").is_some_and(|e| !e.is_boolean()) {
        errors.push("router.tokenSaver.enabled must be a boolean".into());
    }
    if ts.get("enabled") == Some(&Value::Bool(false)) {
        return;
    }
    validate_required_model_ref(config, ts.get("judge"), "router.tokenSaver.judge", errors);
    if ts.get("defaultTier").is_some_and(|t| !t.is_string()) {
        errors.push("router.tokenSaver.defaultTier must be a string".into());
    }
    if ts.get("tiers").is_some_and(|t| !t.is_object()) {
        errors.push("router.tokenSaver.tiers must be a non-empty object".into());
    }
    if let Some(Value::Object(tiers)) = ts.get("tiers") {
        if tiers.is_empty() {
            errors.push("router.tokenSaver.tiers must be a non-empty object".into());
        }
        for (k, tier) in tiers {
            let Value::Object(t) = tier else {
                errors.push(format!("router.tokenSaver.tiers.{k} must be an object with model"));
                continue;
            };
            validate_required_model_ref(config, t.get("model"), &format!("router.tokenSaver.tiers.{k}.model"), errors);
            for f in ["label", "description"] {
                if t.get(f).is_some_and(|v| !v.is_string()) {
                    errors.push(format!("router.tokenSaver.tiers.{k}.{f} must be a string"));
                }
            }
        }
        if let Some(Value::String(dt)) = ts.get("defaultTier") {
            if !tiers.contains_key(dt) {
                errors.push(format!("router.tokenSaver.defaultTier=\"{dt}\" must exist in router.tokenSaver.tiers"));
            }
        }
    }
    if let Some(sub) = ts.get("subagent") {
        match sub {
            Value::Object(s) if matches!(s.get("policy").and_then(Value::as_str), Some("skip" | "judge")) => {}
            Value::Object(_) => errors.push("router.tokenSaver.subagent.policy must be one of skip / judge".into()),
            _ => errors.push("router.tokenSaver.subagent must be an object".into()),
        }
    }
}

fn push_diagnostics(d: Vec<Value>, errors: &mut Vec<String>, warnings: &mut Vec<String>) {
    for x in d {
        let msg = x.get("message").and_then(Value::as_str).unwrap_or_default();
        let message = match x.get("path").and_then(Value::as_str) {
            Some(p) if !p.is_empty() => format!("{p}: {msg}"),
            _ => msg.to_string(),
        };
        if x.get("severity").and_then(Value::as_str) == Some("warning") {
            warnings.push(message);
        } else {
            errors.push(message);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Validation {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub config: Value,
}

impl Validation {
    pub fn to_json(&self) -> Value {
        json!({ "valid": self.valid, "errors": self.errors, "warnings": self.warnings, "config": self.config })
    }
}

fn agent_model(n: &Value) -> Result<Option<&Value>, JsError> {
    match n.get("agent") {
        Some(Value::Null) => Err(type_error("Cannot read properties of null (reading 'model')".into())),
        a => Ok(field(a, "model")),
    }
}

pub fn validate_pilotdeck_config(config: Option<&Value>, ctx: &PilotDeckEnv) -> Result<Validation, JsError> {
    let n = normalize_pilotdeck_config(config, ctx)?;
    let (mut errors, mut warnings) = (vec![], vec![]);
    if let Some(Value::Object(providers)) = field(n.get("model"), "providers") {
        for (id, p) in providers {
            validate_provider(id, p, &mut errors, ctx);
        }
    }
    let main = normalize_string(agent_model(&n)?);
    if main.is_empty() {
        warnings.push("agent.model is empty; pick a model from model.providers.".to_string());
    } else if !resolves(&n, &main) {
        errors.push(format!("agent.model=\"{main}\" doesn't resolve to a configured provider/model"));
    }
    let memory = n.get("memory");
    let mem_ref = normalize_string(field(memory, "model"));
    if truthy(field(memory, "enabled")) && !mem_ref.is_empty() && mem_ref != "inherit" && !resolves(&n, &mem_ref) {
        errors.push(format!("memory.model=\"{mem_ref}\" doesn't resolve to a configured provider/model"));
    }
    let sub = normalize_string(field(field(n.get("agent"), "subagents"), "default"));
    if !sub.is_empty() && sub != "inherit" && !resolves(&n, &sub) {
        warnings.push(format!("agent.subagents.default=\"{sub}\" doesn't resolve to a configured provider/model; subagents will inherit agent.model"));
    }
    validate_router_model_refs(&n, &mut errors);
    let mut d = vec![];
    parse_gateway_config(n.get("gateway"), &mut d);
    push_diagnostics(d, &mut errors, &mut warnings);
    let mut d = vec![];
    parse_tools_config(n.get("tools"), &mut d);
    push_diagnostics(d, &mut errors, &mut warnings);
    let webui = n.get("webui");
    if field(field(webui, "runtime"), "contextWindow").is_some() {
        warnings.push("webui.runtime.contextWindow is deprecated and ignored. Use agent.maxContextTokens to override the model's context window for auto-compaction.".into());
    }
    let op = field(webui, "officePreview");
    if let Some(s) = field(op, "service") {
        if !["builtin", "libreoffice"].contains(&normalize_string(Some(s)).to_lowercase().as_str()) {
            errors.push("webui.officePreview.service must be \"builtin\" or \"libreoffice\"".into());
        }
    }
    if field(op, "binaryPath").is_some_and(|b| !b.is_string()) {
        errors.push("webui.officePreview.binaryPath must be a string".into());
    }
    Ok(Validation { valid: errors.is_empty(), errors, warnings, config: n })
}

fn is_secret_key(key: &str) -> bool {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    // JS `/…$/i` without the u flag never folds non-ASCII onto ASCII, so ASCII lowercasing is exact.
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"(api[_-]?key|token|secret|password|auth[_-]?token|access[_-]?token|bot[_-]?token|app[_-]?token|encoding[_-]?aes[_-]?key)$").unwrap()
    });
    ["key", "apiKey", "api_key", "authToken", "accessToken"].contains(&key) || re.is_match(&key.to_ascii_lowercase())
}

pub fn mask_secrets(v: &Value) -> Value {
    match v {
        Value::Array(a) => Value::Array(a.iter().map(mask_secrets).collect()),
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, c)| {
                    let masked = matches!(c, Value::String(s) if !js_trim(s).is_empty()) && is_secret_key(k);
                    (k.clone(), if masked { Value::String(MASK.into()) } else { mask_secrets(c) })
                })
                .collect(),
        ),
        _ => v.clone(),
    }
}

pub fn preserve_masked_secrets(next: Option<&Value>, prev: Option<&Value>) -> Option<Value> {
    match next? {
        Value::String(s) if s == MASK && matches!(prev, Some(Value::String(_))) => prev.cloned(),
        Value::Array(a) => Some(Value::Array(
            a.iter()
                .enumerate()
                .map(|(i, x)| {
                    let p = match prev {
                        Some(Value::Array(pa)) => pa.get(i),
                        _ => None,
                    };
                    preserve_masked_secrets(Some(x), p).unwrap_or(Value::Null)
                })
                .collect(),
        )),
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, c) in m {
                if let Some(v) = preserve_masked_secrets(Some(c), field(prev, k)) {
                    out.insert(k.clone(), v);
                }
            }
            Some(Value::Object(out))
        }
        v => Some(v.clone()),
    }
}

pub fn has_unresolved_masked_secrets(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Array(a)) => a.iter().any(|x| has_unresolved_masked_secrets(Some(x))),
        Some(Value::Object(m)) => m
            .iter()
            .any(|(k, c)| (is_secret_key(k) && c.as_str() == Some(MASK)) || has_unresolved_masked_secrets(Some(c))),
        _ => false,
    }
}

fn protocol_to_memory_api(p: Option<&Value>) -> &'static str {
    match p.and_then(Value::as_str) {
        Some("anthropic") => "anthropic",
        Some("google") => "google",
        Some("openai-responses") => "openai-responses",
        _ => "openai-completions",
    }
}

/// `Number.parseInt(s, 10)` → `String(n)` when finite and positive.
fn parse_positive_int(s: &str) -> Option<String> {
    let s = js_trim(s);
    let (neg, rest) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || neg {
        return None;
    }
    let n: f64 = digits.parse().ok()?;
    (n.is_finite() && n > 0.0).then(|| js_number_to_string(n))
}

pub fn expand_tilde(v: Option<&Value>, ctx: &PilotDeckEnv) -> String {
    let t = normalize_string(v);
    if t == "~" {
        return ctx.home.clone();
    }
    match t.strip_prefix("~/") {
        Some(rest) => path_join(&[&ctx.home, rest]),
        None => t,
    }
}

/// `buildRuntimeEnv`: an ordered string map (JS integer-key ordering applied).
pub fn build_runtime_env(config: Option<&Value>, ctx: &PilotDeckEnv) -> Result<Value, JsError> {
    let n = normalize_pilotdeck_config(config, ctx)?;
    let main = resolve_model(Some(&n), agent_model(&n)?, true)?;
    let empty = json!({});
    let runtime = match field(Some(&n), "webui").and_then(|w| w.get("runtime")) {
        None | Some(Value::Null) => &empty,
        Some(r) => r,
    };
    let rt = |k: &str, d: Value| match field(Some(runtime), k) {
        None | Some(Value::Null) => js_object_string(&d),
        Some(v) => js_object_string(v),
    };
    let env_or = |k: &str, d: String| match ctx.get(k) {
        Some(v) if !v.is_empty() => v.to_string(),
        _ => d,
    };
    let mut env = Map::new();
    let set = |env: &mut Map<String, Value>, k: &str, v: String| {
        env.insert(k.into(), Value::String(v));
    };
    set(&mut env, "SERVER_PORT", env_or("SERVER_PORT", rt("serverPort", json!(3001))));
    set(&mut env, "VITE_PORT", env_or("VITE_PORT", rt("vitePort", json!(5173))));
    set(&mut env, "HOST", env_or("HOST", rt("host", json!("0.0.0.0"))));
    set(&mut env, "API_TIMEOUT_MS", rt("apiTimeoutMs", json!(120000)));
    let memory = n.get("memory");
    set(&mut env, "PILOTDECK_MEMORY_ENABLED", if truthy(field(memory, "enabled")) { "1" } else { "0" }.into());
    for (k, name) in [("databasePath", "DATABASE_PATH"), ("workspacesRoot", "WORKSPACES_ROOT")] {
        let v = field(Some(runtime), k);
        if truthy(v) {
            set(&mut env, name, expand_tilde(v, ctx));
        }
    }
    let proxy = n.get("proxy");
    let proxy_url = [field(proxy, "url"), proxy.filter(|p| p.is_string()), field(Some(runtime), "httpsProxy")]
        .into_iter()
        .find(|v| truthy(*v))
        .flatten();
    if let Some(p) = proxy_url {
        set(&mut env, "HTTPS_PROXY", js_object_string(p));
        set(&mut env, "https_proxy", js_object_string(p));
    }
    if let Some(m) = &main {
        let url = resolve_configured_provider_url(&m.provider_id, Some(m.provider));
        set(&mut env, "PILOTDECK_API_BASE_URL", url.clone());
        set(&mut env, "PILOTDECK_API_KEY", resolve_configured_provider_api_key(&m.provider_id, Some(m.provider), ctx));
        set(&mut env, "PILOTDECK_MODEL", m.model.clone());
        set(&mut env, "OPENAI_BASE_URL", url);
        for k in ["OPENAI_MODEL", "ANTHROPIC_MODEL", "GEMINI_MODEL"] {
            set(&mut env, k, m.model.clone());
        }
    }
    let params = match field(n.get("agent"), "params") {
        None | Some(Value::Null) => &empty,
        Some(p) => p,
    };
    let nn = |k: &str| field(Some(params), k).filter(|v| !v.is_null());
    let requested = nn("maxOutputTokens").or_else(|| nn("max_output_tokens")).or_else(|| nn("max_tokens"));
    match requested.map(js_object_string).and_then(|s| parse_positive_int(&s)) {
        Some(v) => set(&mut env, "PILOTDECK_MAX_OUTPUT_TOKENS", v),
        None => {
            if let Some(v) = ctx.get("PILOTDECK_MAX_OUTPUT_TOKENS").filter(|v| !v.is_empty()) {
                set(&mut env, "PILOTDECK_MAX_OUTPUT_TOKENS", v.to_string());
            }
        }
    }
    let tavily = nn("tavilyApiKey").or_else(|| nn("tavily_api_key")).cloned().or_else(|| ctx.get("TAVILY_API_KEY").map(|s| json!(s)));
    if truthy(tavily.as_ref()) {
        set(&mut env, "TAVILY_API_KEY", js_object_string(tavily.as_ref().unwrap()));
    }
    let mem_ref = normalize_string(field(memory, "model"));
    let mem_ref_v = if mem_ref.is_empty() { agent_model(&n)?.cloned() } else { Some(Value::String(mem_ref)) };
    if let Some(m) = resolve_model(Some(&n), mem_ref_v.as_ref(), true)? {
        set(&mut env, "PILOTDECK_MEMORY_MODEL", m.model.clone());
        set(&mut env, "PILOTDECK_MEMORY_PROVIDER", m.provider_id.clone());
        set(&mut env, "PILOTDECK_MEMORY_BASE_URL", resolve_configured_provider_url(&m.provider_id, Some(m.provider)));
        set(&mut env, "PILOTDECK_MEMORY_API_KEY", resolve_configured_provider_api_key(&m.provider_id, Some(m.provider), ctx));
        let api = normalize_string(field(memory, "apiType"));
        let api = if api.is_empty() { protocol_to_memory_api(m.provider.get("protocol")).to_string() } else { api };
        set(&mut env, "PILOTDECK_MEMORY_API_TYPE", api);
    }
    if let Some(Value::Object(ce)) = n.get("customEnv") {
        for (k, v) in ce {
            if let Value::String(s) = v {
                if !js_trim(s).is_empty() {
                    set(&mut env, k, s.clone());
                }
            }
        }
    }
    Ok(js_order(Value::Object(env)))
}

fn memory_llm_from(n: &Value, ctx: &PilotDeckEnv) -> Result<Option<Value>, JsError> {
    let memory = n.get("memory");
    let r = normalize_string(field(memory, "model"));
    let rv = if r.is_empty() { agent_model(n)?.cloned() } else { Some(Value::String(r)) };
    let Some(m) = resolve_model(Some(n), rv.as_ref(), true)? else { return Ok(None) };
    let api = normalize_string(field(memory, "apiType"));
    let api = if api.is_empty() { protocol_to_memory_api(m.provider.get("protocol")).to_string() } else { api };
    let headers = m.provider.get("headers").filter(|h| h.is_object()).cloned().unwrap_or(json!({}));
    Ok(Some(json!({
        "provider": m.provider_id,
        "model": m.model,
        "apiType": api,
        "baseUrl": resolve_configured_provider_url(&m.provider_id, Some(m.provider)),
        "apiKey": resolve_configured_provider_api_key(&m.provider_id, Some(m.provider), ctx),
        "headers": headers,
    })))
}

pub fn build_memory_llm_options(config: Option<&Value>, ctx: &PilotDeckEnv) -> Result<Option<Value>, JsError> {
    memory_llm_from(&normalize_pilotdeck_config(config, ctx)?, ctx)
}

/// `buildMemoryDefaults` (JSON view: undefined fields omitted).
pub fn build_memory_defaults(config: Option<&Value>, ctx: &PilotDeckEnv) -> Result<Value, JsError> {
    let n = normalize_pilotdeck_config(config, ctx)?;
    let memory = n.get("memory").filter(|m| !m.is_null()).cloned().unwrap_or(json!({}));
    let llm = memory_llm_from(&n, ctx)?;
    let pick = |keys: &[&str]| {
        let mut o = Map::new();
        for k in keys {
            if let Some(v) = field(Some(&memory), k) {
                o.insert((*k).into(), v.clone());
            }
        }
        o
    };
    let mut out = Map::new();
    if let Some(l) = llm {
        out.insert("llm".into(), l);
    }
    out.insert("defaultIndexingSettings".into(), Value::Object(pick(&["reasoningMode", "autoIndexIntervalMinutes", "autoDreamIntervalMinutes"])));
    out.extend(pick(&["captureStrategy", "includeAssistant", "maxMessageChars", "heartbeatBatchSize"]));
    Ok(Value::Object(out))
}

/// `configRevision(raw)`: sha256 hex of `String(raw ?? '')`.
pub fn config_revision(raw: Option<&Value>) -> String {
    use sha2::{Digest, Sha256};
    let s = match raw {
        None | Some(Value::Null) => String::new(),
        Some(v) => js_object_string(v),
    };
    Sha256::digest(s.as_bytes()).iter().fold(String::with_capacity(64), |mut h, b| {
        use std::fmt::Write;
        let _ = write!(h, "{b:02x}");
        h
    })
}

/// `syncAgentModelWithRouter` (in place).
pub fn sync_agent_model_with_router(config: &mut Value) {
    let agent_ref = normalize_string(field(field(Some(config), "agent"), "model"));
    let Some(slash) = agent_ref.find('/') else { return };
    if slash == 0 || slash >= agent_ref.len() - 1 {
        return;
    }
    let (pid, mid) = (&agent_ref[..slash], &agent_ref[slash + 1..]);
    let Some(Value::Object(router)) = config.get_mut("router") else { return };
    if router.get("enabled") == Some(&Value::Bool(false)) {
        return;
    }
    let Some(Value::Object(sc)) = router.get_mut("scenarios") else { return };
    let cur = sc.get("default");
    let current_id = match cur {
        Some(Value::String(s)) => js_trim(s).to_string(),
        Some(o @ Value::Object(_)) => normalize_string(o.get("id")),
        _ => String::new(),
    };
    if current_id == agent_ref {
        return;
    }
    let next = if matches!(cur, Some(Value::String(_))) {
        json!(agent_ref)
    } else {
        json!({ "id": agent_ref, "provider": pid, "model": mid })
    };
    sc.insert("default".into(), next);
}
