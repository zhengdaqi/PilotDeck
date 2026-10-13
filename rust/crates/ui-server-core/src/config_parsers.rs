//! Ports of the pilotdeckConfig dependencies:
//! - src/model/catalog: `PROVIDER_CATALOG` (snapshot `provider_catalog.json`, generated from the TS module and checked
//!   for equality by gen_config_parsers.mjs --check) and `lookupCatalogProvider`. Object.prototype keys
//!   ("constructor", "__proto__", …) resolve to values whose catalog fields are all undefined, which every caller
//!   treats like a missing provider, so they map to `None`.
//! - src/model/config/providerCredentialScope.ts
//! - src/model/thinking/settings.ts `parseThinkingSettings`
//! - src/pilot/config/parseGatewayConfig.ts (`parseGatewayConfig`, `parseAdaptersConfig`)
//! - src/pilot/config/parseToolsConfig.ts
//!
//! Parsed objects omit `undefined` fields (the JSON view of the TS result); passthrough numbers keep their JSON value.
use crate::js_number::js_trim;
use crate::js_object::field;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

pub fn provider_catalog() -> &'static Map<String, Value> {
    static C: OnceLock<Map<String, Value>> = OnceLock::new();
    C.get_or_init(|| serde_json::from_str(include_str!("provider_catalog.json")).expect("catalog snapshot"))
}

pub fn lookup_catalog_provider(id: &str) -> Option<&'static Value> {
    provider_catalog().get(id)
}

fn catalog_str(id: &str, key: &str) -> Option<&'static str> {
    lookup_catalog_provider(id).and_then(|p| p.get(key)).and_then(Value::as_str)
}

const GOOGLE_OPENAI_COMPATIBLE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/openai";

fn canonical_provider_url(v: Option<&Value>) -> String {
    let Some(Value::String(s)) = v else { return String::new() };
    let t = js_trim(s);
    if t.is_empty() {
        return String::new();
    }
    match url::Url::parse(t) {
        Ok(u) if u.scheme() == "http" || u.scheme() == "https" => u.as_str().trim_end_matches('/').to_string(),
        _ => String::new(),
    }
}

fn is_str(v: Option<&Value>, s: &str) -> bool {
    matches!(v, Some(Value::String(x)) if x == s)
}

pub fn resolve_catalog_provider_default_url(id: &str, protocol: Option<&Value>) -> Option<String> {
    if id == "google" && is_str(protocol, "openai") {
        return Some(GOOGLE_OPENAI_COMPATIBLE_URL.into());
    }
    catalog_str(id, "defaultUrl").map(str::to_string)
}

pub fn resolve_catalog_provider_api_key_env_var(id: &str, protocol: Option<&Value>, url: Option<&Value>) -> Option<String> {
    let env = catalog_str(id, "apiKeyEnvVar").filter(|e| !e.is_empty())?;
    let catalog_protocol = catalog_str(id, "protocol");
    let can_use = matches!((protocol, catalog_protocol), (Some(Value::String(p)), Some(c)) if p == c) || (id == "google" && is_str(protocol, "openai"));
    if !can_use {
        return None;
    }
    let expected = canonical_provider_url(resolve_catalog_provider_default_url(id, protocol).map(Value::String).as_ref());
    if expected.is_empty() || canonical_provider_url(url) != expected {
        return None;
    }
    Some(env.to_string())
}

// ---------------------------------------------------------------- thinking

pub const THINKING_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

pub fn thinking_formats_for_protocol(protocol: &str) -> &'static [&'static str] {
    match protocol {
        "anthropic" => &["provider", "anthropic", "server-default"],
        "google" => &["provider", "google", "server-default"],
        "openai-responses" => &["provider", "openai", "server-default"],
        _ => &["provider", "openai", "thinking-type", "qwen-cloud", "qwen-local", "openrouter", "server-default"],
    }
}

/// `parseThinkingSettings(raw, protocol)`; `Err` carries the thrown Error message.
pub fn parse_thinking_settings(raw: Option<&Value>, protocol: &str) -> Result<Option<Value>, String> {
    let Some(raw) = raw else { return Ok(None) };
    let Value::Object(o) = raw else { return Err("Model thinking must be an object.".into()) };
    let state = o.get("state").cloned().unwrap_or(json!("default"));
    let efforts = o.get("efforts").cloned().unwrap_or(json!([]));
    let format = o.get("format").cloned().unwrap_or(json!("provider"));
    if !["default", "enabled", "disabled"].iter().any(|s| state == json!(s)) {
        return Err("thinking.state must be default, enabled or disabled.".into());
    }
    let Value::Array(list) = &efforts else { return Err("thinking.efforts must contain only low, medium, high, xhigh or max.".into()) };
    if list.iter().any(|e| !THINKING_EFFORTS.iter().any(|x| *e == json!(x))) {
        return Err("thinking.efforts must contain only low, medium, high, xhigh or max.".into());
    }
    if !thinking_formats_for_protocol(protocol).iter().any(|f| format == json!(f)) {
        return Err("thinking.format must match the provider protocol.".into());
    }
    let kept: Vec<&str> = THINKING_EFFORTS.iter().copied().filter(|e| list.contains(&json!(e))).collect();
    Ok(Some(json!({ "state": state, "efforts": kept, "format": format })))
}

// ---------------------------------------------------------------- diagnostics helpers

pub fn diagnostic(code: &str, severity: &str, message: &str, path: &str, recoverable: bool) -> Value {
    json!({ "code": code, "severity": severity, "message": message, "path": path, "recoverable": recoverable })
}

fn fatal(d: &mut Vec<Value>, code: &str, message: &str, path: &str) {
    d.push(diagnostic(code, "fatal", message, path, false));
}

fn warn(d: &mut Vec<Value>, code: &str, message: &str, path: &str) {
    d.push(diagnostic(code, "warning", message, path, true));
}

fn record(v: Option<&Value>) -> Option<&Map<String, Value>> {
    match v {
        Some(Value::Object(o)) => Some(o),
        _ => None,
    }
}

fn string_field(r: &Map<String, Value>, key: &str) -> Option<Value> {
    r.get(key).filter(|v| v.is_string()).cloned()
}

fn finite_number(v: Option<&Value>) -> Option<&Value> {
    v.filter(|v| v.as_f64().is_some_and(f64::is_finite))
}

fn number_field(r: &Map<String, Value>, key: &str, fallback: i64) -> Value {
    finite_number(r.get(key)).cloned().unwrap_or(json!(fallback))
}

fn bool_field(r: &Map<String, Value>, key: &str, fallback: bool) -> Value {
    r.get(key).filter(|v| v.is_boolean()).cloned().unwrap_or(json!(fallback))
}

fn put(o: &mut Map<String, Value>, k: &str, v: Option<Value>) {
    if let Some(v) = v {
        o.insert(k.into(), v);
    }
}

// ---------------------------------------------------------------- gateway / adapters

/// `parseGatewayConfig(raw, diagnostics)`; `raw: None` = undefined.
pub fn parse_gateway_config(raw: Option<&Value>, d: &mut Vec<Value>) -> Option<Value> {
    raw?;
    let Some(r) = record(raw) else {
        fatal(d, "GATEWAY_CONFIG_INVALID", "gateway config must be an object.", "gateway");
        return None;
    };
    if string_field(r, "bindAddress").unwrap_or(json!("127.0.0.1")) != json!("127.0.0.1") {
        fatal(d, "GATEWAY_BIND_ADDRESS_UNSUPPORTED", "gateway.bindAddress must be 127.0.0.1 in the first phase.", "gateway.bindAddress");
    }
    if r.contains_key("tokenPath") {
        warn(d, "GATEWAY_TOKEN_PATH_REMOVED", "gateway.tokenPath is no longer configurable; the gateway token is stored under PilotHome.", "gateway.tokenPath");
    }
    let max_mcp = number_field(r, "maxPerSessionMcpInstances", 5);
    let mut o = Map::new();
    o.insert("port".into(), number_field(r, "port", 18789));
    o.insert("bindAddress".into(), json!("127.0.0.1"));
    o.insert("idleSessionTimeoutMinutes".into(), number_field(r, "idleSessionTimeoutMinutes", 30));
    o.insert("idleSweepIntervalSeconds".into(), number_field(r, "idleSweepIntervalSeconds", 60));
    o.insert("memoryDiagnostics".into(), bool_field(r, "memoryDiagnostics", false));
    put(&mut o, "staticAssetsPath", string_field(r, "staticAssetsPath"));
    o.insert("maxPerSessionMcpInstances".into(), if max_mcp.as_f64().unwrap() >= 1.0 { max_mcp } else { json!(1) });
    Some(Value::Object(o))
}

const PLATFORM_KEYS: [&str; 16] = ["telegram", "discord", "slack", "matrix", "mattermost", "signal", "whatsapp", "bluebubbles", "dingtalk", "wecom", "wecomCallback", "email", "sms", "homeassistant", "apiServer", "webhook"];

fn one_of(v: Option<Value>, allowed: &[&str]) -> Option<Value> {
    v.filter(|x| allowed.iter().any(|a| *x == json!(a)))
}

/// `parseAdaptersConfig(raw, diagnostics)`.
pub fn parse_adapters_config(raw: Option<&Value>, d: &mut Vec<Value>) -> Option<Value> {
    raw?;
    let Some(r) = record(raw) else {
        fatal(d, "ADAPTERS_CONFIG_INVALID", "adapters config must be an object.", "adapters");
        return None;
    };
    let mut o = Map::new();
    for k in ["cli", "tui"] {
        put(&mut o, k, record(r.get(k)).map(|x| json!({ "autoConnectServer": bool_field(x, "autoConnectServer", true) })));
    }
    put(&mut o, "feishu", record(r.get("feishu")).map(|x| {
        let mut f = Map::new();
        f.insert("enabled".into(), bool_field(x, "enabled", false));
        for k in ["appId", "appSecret", "encryptKey", "verifyToken"] {
            put(&mut f, k, string_field(x, k));
        }
        f.insert("defaultSessionLabel".into(), string_field(x, "defaultSessionLabel").unwrap_or(json!("general")));
        put(&mut f, "connectionMode", one_of(string_field(x, "connectionMode"), &["stream", "webhook"]));
        put(&mut f, "domainName", one_of(string_field(x, "domainName"), &["feishu", "lark"]));
        put(&mut f, "permissionMode", one_of(string_field(x, "permissionMode"), &["default", "bypassPermissions"]));
        Value::Object(f)
    }));
    put(&mut o, "weixin", record(r.get("weixin")).map(|x| json!({ "enabled": bool_field(x, "enabled", false) })));
    put(&mut o, "qq", record(r.get("qq")).map(|x| {
        let strings = |k: &str| match x.get(k) {
            Some(Value::Array(a)) => Some(Value::Array(a.iter().filter(|v| v.is_string()).cloned().collect())),
            _ => None,
        };
        let mut q = Map::new();
        q.insert("enabled".into(), bool_field(x, "enabled", false));
        put(&mut q, "appId", string_field(x, "appId"));
        put(&mut q, "clientSecret", string_field(x, "clientSecret"));
        put(&mut q, "allowGroups", strings("allowGroups"));
        put(&mut q, "triggerPrefixes", strings("triggerPrefixes"));
        put(&mut q, "maxMessageLength", finite_number(x.get("maxMessageLength")).cloned());
        Value::Object(q)
    }));
    for k in PLATFORM_KEYS {
        if let Some(x) = record(r.get(k)) {
            let mut p = Map::new();
            p.insert("enabled".into(), bool_field(x, "enabled", false));
            for f in ["token", "apiKey", "webhookUrl"] {
                put(&mut p, f, string_field(x, f));
            }
            put(&mut p, "extra", record(x.get("extra")).map(|e| Value::Object(e.clone())));
            o.insert(k.into(), Value::Object(p));
        }
    }
    Some(Value::Object(o))
}

// ---------------------------------------------------------------- tools

pub const WEB_SEARCH_PROVIDERS: [&str; 9] = ["glm", "tavily", "serper", "brave", "baidu", "bocha", "exa", "serpapi", "custom"];
pub const SERPAPI_ENGINES: [&str; 6] = ["google", "bing", "baidu", "duckduckgo", "yahoo", "yandex"];

fn parse_enum(raw: Option<&Value>, allowed: &[&str], path: &str, code: &str, d: &mut Vec<Value>) -> Option<Value> {
    let raw = raw?;
    if allowed.iter().any(|a| *raw == json!(a)) {
        return Some(raw.clone());
    }
    fatal(d, code, &format!("{path} must be one of: {}.", allowed.join(", ")), path);
    None
}

fn non_empty_trimmed(v: &Value) -> Option<String> {
    v.as_str().map(js_trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn parse_custom_provider(raw: Option<&Value>, d: &mut Vec<Value>) -> Option<Value> {
    raw?;
    let Some(r) = record(raw) else {
        fatal(d, "TOOLS_WEB_SEARCH_CUSTOM_PROVIDER_INVALID", "tools.webSearch.customProvider must be an object.", "tools.webSearch.customProvider");
        return None;
    };
    let mut o = Map::new();
    put(&mut o, "auth", parse_enum(r.get("auth"), &["bearer", "bodyApiKey", "queryApiKey", "none"], "tools.webSearch.customProvider.auth", "TOOLS_WEB_SEARCH_CUSTOM_AUTH_INVALID", d));
    put(&mut o, "method", parse_enum(r.get("method"), &["GET", "POST"], "tools.webSearch.customProvider.method", "TOOLS_WEB_SEARCH_CUSTOM_METHOD_INVALID", d));
    const FIELDS: [&str; 9] = ["name", "queryParam", "apiKeyParam", "resultsPath", "titleField", "urlField", "snippetField", "sourceField", "publishedAtField"];
    for f in FIELDS {
        if let Some(v) = r.get(f) {
            match non_empty_trimmed(v) {
                Some(s) => {
                    o.insert(f.into(), json!(s));
                }
                None => {
                    let path = format!("tools.webSearch.customProvider.{f}");
                    fatal(d, "TOOLS_WEB_SEARCH_CUSTOM_STRING_INVALID", &format!("{path} must be a non-empty string."), &path);
                }
            }
        }
    }
    for k in r.keys() {
        if k != "auth" && k != "method" && !FIELDS.contains(&k.as_str()) {
            let path = format!("tools.webSearch.customProvider.{k}");
            warn(d, "TOOLS_WEB_SEARCH_CUSTOM_PROVIDER_UNKNOWN_FIELD", &format!("Unknown tools.webSearch.customProvider field {k}."), &path);
        }
    }
    (!o.is_empty()).then_some(Value::Object(o))
}

fn parse_web_search(raw: Option<&Value>, d: &mut Vec<Value>) -> Option<Value> {
    raw?;
    let Some(r) = record(raw) else {
        fatal(d, "TOOLS_WEB_SEARCH_INVALID", "tools.webSearch must be an object.", "tools.webSearch");
        return None;
    };
    let mut o = Map::new();
    if let Some(e) = r.get("enabled") {
        match e {
            Value::Bool(b) => {
                o.insert("enabled".into(), json!(b));
            }
            _ => fatal(d, "TOOLS_WEB_SEARCH_ENABLED_INVALID", "tools.webSearch.enabled must be a boolean.", "tools.webSearch.enabled"),
        }
    }
    if o.get("enabled") == Some(&json!(false)) {
        return Some(json!({ "enabled": false }));
    }
    if let Some(p) = r.get("provider") {
        if WEB_SEARCH_PROVIDERS.iter().any(|x| *p == json!(x)) {
            o.insert("provider".into(), p.clone());
        } else {
            fatal(d, "TOOLS_WEB_SEARCH_PROVIDER_INVALID", "tools.webSearch.provider must be a supported search provider.", "tools.webSearch.provider");
        }
    }
    put(&mut o, "searchEngine", parse_enum(r.get("searchEngine"), &SERPAPI_ENGINES, "tools.webSearch.searchEngine", "TOOLS_WEB_SEARCH_ENGINE_INVALID", d));
    if let Some(k) = r.get("apiKey") {
        match non_empty_trimmed(k) {
            Some(s) => {
                o.insert("apiKey".into(), json!(s));
            }
            None => fatal(d, "TOOLS_WEB_SEARCH_API_KEY_INVALID", "tools.webSearch.apiKey must be a non-empty string.", "tools.webSearch.apiKey"),
        }
    }
    if let Some(e) = r.get("endpoint") {
        match non_empty_trimmed(e) {
            None => fatal(d, "TOOLS_WEB_SEARCH_ENDPOINT_INVALID", "tools.webSearch.endpoint must be a non-empty URL string.", "tools.webSearch.endpoint"),
            Some(s) => match url::Url::parse(&s) {
                Ok(u) if u.scheme() == "http" || u.scheme() == "https" => {
                    o.insert("endpoint".into(), json!(s));
                }
                _ => fatal(d, "TOOLS_WEB_SEARCH_ENDPOINT_INVALID", "tools.webSearch.endpoint must be an HTTP(S) URL.", "tools.webSearch.endpoint"),
            },
        }
    }
    put(&mut o, "customProvider", parse_custom_provider(r.get("customProvider"), d));
    if r.contains_key("region") {
        warn(d, "TOOLS_WEB_SEARCH_REGION_DEPRECATED", "tools.webSearch.region has been removed. Select tools.webSearch.provider instead.", "tools.webSearch.region");
    }
    if r.contains_key("tavilyApiKey") {
        warn(d, "TOOLS_WEB_SEARCH_TAVILY_KEY_DEPRECATED", "tools.webSearch.tavilyApiKey has been removed. Set tools.webSearch.provider: tavily and use tools.webSearch.apiKey.", "tools.webSearch.tavilyApiKey");
    }
    const KNOWN: [&str; 8] = ["enabled", "provider", "apiKey", "endpoint", "customProvider", "searchEngine", "region", "tavilyApiKey"];
    for k in r.keys() {
        if !KNOWN.contains(&k.as_str()) {
            warn(d, "TOOLS_WEB_SEARCH_UNKNOWN_FIELD", &format!("Unknown tools.webSearch field {k}."), &format!("tools.webSearch.{k}"));
        }
    }
    Some(Value::Object(o))
}

/// `parseToolsConfig(raw, diagnostics)`.
pub fn parse_tools_config(raw: Option<&Value>, d: &mut Vec<Value>) -> Option<Value> {
    raw?;
    let Some(r) = record(raw) else {
        fatal(d, "TOOLS_CONFIG_INVALID", "tools config must be an object.", "tools");
        return None;
    };
    let web = parse_web_search(r.get("webSearch"), d);
    for k in r.keys() {
        if k != "webSearch" {
            warn(d, "TOOLS_UNKNOWN_FIELD", &format!("Unknown tools config field {k}."), &format!("tools.{k}"));
        }
    }
    web.map(|w| json!({ "webSearch": w }))
}

/// `field` re-export for callers composing configs.
pub fn get<'a>(v: Option<&'a Value>, k: &str) -> Option<&'a Value> {
    field(v, k)
}
