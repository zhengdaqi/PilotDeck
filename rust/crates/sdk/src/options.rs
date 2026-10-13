//! Port of `validateOptions` (and the helpers it calls) from `packages/sdk/src/client.ts`.
//!
//! The SDK options arrive from JavaScript callers (wasm wrapper) or as loosely typed host config, so the
//! validator works on a JS-shaped value ([`Js`]) and keeps every check, its order, its message and the
//! TypeErrors V8 raises when a member has the wrong type (`server.command.trim is not a function`, ...).
use crate::js::trim;
use serde_json::Value;

/// A JavaScript value as seen by the validator. `Obj` keeps own enumerable keys in insertion order.
#[derive(Clone, Debug, PartialEq)]
pub enum Js {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Js>),
    Obj(Vec<(String, Js)>),
    Func,
}

impl Js {
    /// From JSON, with the markers `{"$fn":1}`, `{"$undef":1}` and `{"$num":"NaN"|"Infinity"|"-Infinity"}`.
    pub fn from_json(v: &Value) -> Js {
        match v {
            Value::Null => Js::Null,
            Value::Bool(b) => Js::Bool(*b),
            Value::Number(n) => Js::Num(n.as_f64().unwrap_or(f64::NAN)),
            Value::String(s) => Js::Str(s.clone()),
            Value::Array(a) => Js::Arr(a.iter().map(Js::from_json).collect()),
            Value::Object(o) => {
                if o.len() == 1 {
                    if o.contains_key("$fn") {
                        return Js::Func;
                    }
                    if o.contains_key("$undef") {
                        return Js::Undefined;
                    }
                    if let Some(Value::String(n)) = o.get("$num") {
                        return Js::Num(match n.as_str() {
                            "Infinity" => f64::INFINITY,
                            "-Infinity" => f64::NEG_INFINITY,
                            _ => f64::NAN,
                        });
                    }
                }
                Js::Obj(o.iter().map(|(k, v)| (k.clone(), Js::from_json(v))).collect())
            }
        }
    }

    /// `v[key]` for property reads that cannot throw (caller handled null/undefined); missing → undefined.
    pub fn get(&self, key: &str) -> &Js {
        static U: Js = Js::Undefined;
        match self {
            Js::Obj(m) => m.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v).unwrap_or(&U),
            _ => &U,
        }
    }

    fn is_undef(&self) -> bool {
        matches!(self, Js::Undefined)
    }

    fn nullish(&self) -> bool {
        matches!(self, Js::Undefined | Js::Null)
    }

    pub fn truthy(&self) -> bool {
        match self {
            Js::Undefined | Js::Null => false,
            Js::Bool(b) => *b,
            Js::Num(n) => *n != 0.0 && !n.is_nan(),
            Js::Str(s) => !s.is_empty(),
            _ => true,
        }
    }

    fn is_object(&self) -> bool {
        matches!(self, Js::Arr(_) | Js::Obj(_))
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Js::Str(s) => Some(s),
            _ => None,
        }
    }

    fn num(&self) -> Option<f64> {
        match self {
            Js::Num(n) => Some(*n),
            _ => None,
        }
    }

    fn strict_eq_str(&self, s: &str) -> bool {
        self.as_str() == Some(s)
    }

    /// `Object.entries(v)` (own enumerable string keys).
    fn entries(&self) -> Vec<(String, Js)> {
        match self {
            Js::Obj(m) => m.clone(),
            Js::Arr(a) => a.iter().enumerate().map(|(i, v)| (i.to_string(), v.clone())).collect(),
            Js::Str(s) => s.encode_utf16().enumerate().map(|(i, u)| (i.to_string(), Js::Str(String::from_utf16_lossy(&[u])))).collect(),
            _ => vec![],
        }
    }

    /// SameValueZero, as used by `Set` and `Array.prototype.includes`. Distinct objects never compare equal.
    fn same_value_zero(&self, o: &Js) -> bool {
        match (self, o) {
            (Js::Num(a), Js::Num(b)) => a == b || (a.is_nan() && b.is_nan()),
            (Js::Undefined, Js::Undefined) | (Js::Null, Js::Null) => true,
            (Js::Bool(a), Js::Bool(b)) => a == b,
            (Js::Str(a), Js::Str(b)) => a == b,
            _ => false,
        }
    }
}

/// `String(v)` for the values that reach an error message.
pub fn js_string(v: &Js) -> String {
    match v {
        Js::Undefined => "undefined".into(),
        Js::Null => "null".into(),
        Js::Bool(b) => b.to_string(),
        Js::Num(n) => crate::transport_core::js_number_to_string(*n),
        Js::Str(s) => s.clone(),
        Js::Arr(a) => a.iter().map(|x| if x.nullish() { String::new() } else { js_string(x) }).collect::<Vec<_>>().join(","),
        Js::Obj(_) => "[object Object]".into(),
        Js::Func => "() => {}".into(),
    }
}

/// What `validateOptions` throws: a `PilotDeckError` (code `validation_error` / `unsupported_capability`)
/// or a V8 `TypeError`.
#[derive(Clone, Debug, PartialEq)]
pub struct OptionsError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for OptionsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

type R = Result<(), OptionsError>;

fn verr(m: impl Into<String>) -> OptionsError {
    OptionsError { code: "validation_error", message: m.into() }
}
fn unsup(m: impl Into<String>) -> OptionsError {
    OptionsError { code: "unsupported_capability", message: m.into() }
}
fn type_err(m: impl Into<String>) -> OptionsError {
    OptionsError { code: "TypeError", message: m.into() }
}
/// `unsupported(name)`.
fn unsupported(name: &str) -> OptionsError {
    unsup(format!("{name} is not exposed by the current Gateway protocol."))
}

/// `x.prop` where x may be null/undefined (V8 TypeError).
fn read<'a>(x: &'a Js, prop: &str) -> Result<&'a Js, OptionsError> {
    match x {
        Js::Undefined => Err(type_err(format!("Cannot read properties of undefined (reading '{prop}')"))),
        Js::Null => Err(type_err(format!("Cannot read properties of null (reading '{prop}')"))),
        _ => Ok(x.get(prop)),
    }
}

/// `expr.trim()` (`expr` is the source text V8 puts in "is not a function").
fn call_trim<'a>(v: &'a Js, expr: &str) -> Result<&'a str, OptionsError> {
    match v {
        Js::Str(s) => Ok(trim(s)),
        Js::Undefined | Js::Null => Err(type_err(format!("Cannot read properties of {} (reading 'trim')", js_string(v)))),
        _ => Err(type_err(format!("{expr}.trim is not a function"))),
    }
}

/// `v?.trim()`: undefined/null short-circuit to undefined.
fn opt_trim<'a>(v: &'a Js, expr: &str) -> Result<Option<&'a str>, OptionsError> {
    match v {
        Js::Undefined | Js::Null => Ok(None),
        Js::Str(s) => Ok(Some(trim(s))),
        _ => Err(type_err(format!("{expr}?.trim is not a function"))),
    }
}

fn is_integer(v: &Js) -> bool {
    v.num().is_some_and(|n| n.is_finite() && n.trunc() == n)
}
fn is_finite(v: &Js) -> bool {
    v.num().is_some_and(f64::is_finite)
}
fn has_duplicates(a: &[Js]) -> bool {
    a.iter().enumerate().any(|(i, x)| a[..i].iter().any(|y| y.same_value_zero(x)))
}
fn non_empty_str(v: &Js) -> bool {
    v.as_str().is_some_and(|s| !trim(s).is_empty())
}

/// `isAbsolutePath` (`node:path` isAbsolute for the platform).
pub fn is_absolute_path(v: &Js) -> bool {
    let Some(p) = v.as_str() else { return false };
    if cfg!(windows) {
        let b = p.as_bytes();
        matches!(b.first(), Some(b'/' | b'\\')) || (b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'/' | b'\\'))
    } else {
        p.starts_with('/')
    }
}

fn is_hosted_mcp_server(server: &Js) -> bool {
    matches!(server, Js::Obj(_) | Js::Arr(_)) && matches!(server.get("start"), Js::Func)
}

/// `supportsUserDialogKind(options, kind)`.
fn supports_user_dialog_kind(options: &Js, kind: &str) -> Result<bool, OptionsError> {
    let k = options.get("supportedDialogKinds");
    if k.is_undef() {
        return Ok(kind == "elicitation");
    }
    includes(k, kind, "options.supportedDialogKinds.includes")
}

/// `v.includes(kind)` on an array (SameValueZero) or a string (substring).
fn includes(v: &Js, kind: &str, expr: &str) -> Result<bool, OptionsError> {
    match v {
        Js::Arr(a) => Ok(a.iter().any(|x| x.strict_eq_str(kind))),
        Js::Str(s) => Ok(s.contains(kind)),
        Js::Null | Js::Undefined => Err(type_err(format!("Cannot read properties of {} (reading 'includes')", js_string(v)))),
        _ => Err(type_err(format!("{expr} is not a function"))),
    }
}

/// `normalizeSandbox(value)` (validation part).
fn normalize_sandbox(value: &Js) -> R {
    if value.is_undef() {
        return Ok(());
    }
    if !matches!(value, Js::Obj(_)) {
        return Err(verr("sandbox must be a tool_policy object."));
    }
    for (key, _) in value.entries() {
        if !matches!(key.as_str(), "type" | "filesystem" | "network" | "process") {
            return Err(unsupported(&format!("sandbox.{key}")));
        }
    }
    let t = value.get("type");
    if !t.is_undef() && !t.strict_eq_str("tool_policy") {
        return Err(unsupported(&format!("sandbox.type={}", js_string(t))));
    }
    let f = value.get("filesystem");
    if !f.is_undef() && !f.strict_eq_str("read_only") && !f.strict_eq_str("deny") {
        return Err(unsupported(&format!("sandbox.filesystem={}", js_string(f))));
    }
    let n = value.get("network");
    if !n.is_undef() && !n.strict_eq_str("deny") {
        return Err(unsupported(&format!("sandbox.network={}", js_string(n))));
    }
    let p = value.get("process");
    if !p.is_undef() && !p.strict_eq_str("deny") {
        return Err(unsupported(&format!("sandbox.process={}", js_string(p))));
    }
    if f.is_undef() && n.is_undef() && p.is_undef() {
        return Err(verr("tool_policy sandbox must declare at least one restriction."));
    }
    Ok(())
}

/// `validateDeferredTools(value, label)`.
fn validate_deferred_tools(value: &Js, label: &str) -> R {
    if value.is_undef() {
        return Ok(());
    }
    let bad = || verr(format!("{label} must be a non-empty array of non-empty names and optional non-empty searchHint strings."));
    let Js::Arr(items) = value else { return Err(bad()) };
    if items.is_empty() {
        return Err(bad());
    }
    for tool in items {
        if !matches!(tool, Js::Obj(_)) {
            return Err(bad());
        }
        let hint = tool.get("searchHint");
        if !non_empty_str(tool.get("name")) || (!hint.is_undef() && !non_empty_str(hint)) {
            return Err(bad());
        }
    }
    let names: Vec<Js> = items.iter().map(|t| Js::Str(trim(t.get("name").as_str().unwrap_or("")).to_string())).collect();
    if has_duplicates(&names) {
        return Err(verr(format!("{label} cannot contain duplicate names.")));
    }
    if names.iter().any(|n| n.strict_eq_str("search_tools")) {
        return Err(verr(format!("{label} cannot include the reserved search_tools name.")));
    }
    Ok(())
}

/// `normalizeSystemPrompt(value)` (validation part).
fn normalize_system_prompt(value: &Js) -> R {
    match value {
        Js::Str(_) => Ok(()),
        Js::Arr(parts) => {
            if parts.iter().any(|p| !matches!(p, Js::Str(_))) {
                return Err(verr("systemPrompt arrays must contain strings."));
            }
            Ok(())
        }
        _ => {
            if read(value, "type")?.strict_eq_str("custom") {
                return normalize_system_prompt(value.get("prompt"));
            }
            Err(unsup("The Claude Code preset systemPrompt has no PilotDeck Gateway equivalent."))
        }
    }
}

/// `isPilotDeckJsonSchema(value)`.
pub fn is_pilotdeck_json_schema(value: &Js) -> bool {
    if !matches!(value, Js::Obj(_)) {
        return false;
    }
    let all_strings = |v: &Js| matches!(v, Js::Arr(a) if a.iter().all(|x| matches!(x, Js::Str(_))));
    let t = value.get("type");
    if !t.is_undef() && !matches!(t, Js::Str(_)) && !all_strings(t) {
        return false;
    }
    let r = value.get("required");
    if !r.is_undef() && !all_strings(r) {
        return false;
    }
    let ap = value.get("additionalProperties");
    if !ap.is_undef() && !matches!(ap, Js::Bool(_)) {
        return false;
    }
    let e = value.get("enum");
    if !e.is_undef() && !matches!(e, Js::Arr(_)) {
        return false;
    }
    let p = value.get("properties");
    if !p.is_undef() {
        if !matches!(p, Js::Obj(_)) {
            return false;
        }
        if !p.entries().iter().all(|(_, v)| is_pilotdeck_json_schema(v)) {
            return false;
        }
    }
    let items = value.get("items");
    if !items.is_undef() && !is_pilotdeck_json_schema(items) {
        return false;
    }
    true
}

fn is_http_type(t: &Js) -> bool {
    t.strict_eq_str("http") || t.strict_eq_str("streamable_http") || t.strict_eq_str("sse")
}

/// `validateAgentMcpTransport(agentName, serverName, server)`.
fn validate_agent_mcp_transport(agent: &str, server_name: &str, server: &Js) -> R {
    if is_hosted_mcp_server(server) {
        return Err(unsup(format!("Agent {agent} MCP server {server_name} must be a serializable stdio, streamable_http, or sse endpoint.")));
    }
    let t = read(server, "type")?;
    if t.strict_eq_str("sdk") || t.strict_eq_str("claude_ai_proxy") {
        return Err(unsup(format!("Agent {agent} MCP server {server_name} uses an unsupported transport. PilotDeck supports stdio, streamable_http, and legacy sse.")));
    }
    if t.strict_eq_str("stdio") && call_trim(server.get("command"), "server.command")?.is_empty() {
        return Err(verr(format!("Agent {agent} MCP server {server_name} requires a command.")));
    }
    if is_http_type(t) && call_trim(server.get("url"), "server.url")?.is_empty() {
        return Err(verr(format!("Agent {agent} MCP server {server_name} requires a URL.")));
    }
    validate_deferred_tools(server.get("deferredTools"), &format!("Agent {agent} MCP server {server_name} deferredTools"))
}

/// `validateAgentMcpServers(agentName, value)`.
fn validate_agent_mcp_servers(agent: &str, value: &Js) -> R {
    let specs: Vec<&Js> = match value {
        Js::Arr(a) => a.iter().collect(),
        v => vec![v],
    };
    let mut names: Vec<String> = vec![];
    for spec in specs {
        if let Js::Str(s) = spec {
            let reference = trim(s);
            if reference.is_empty() {
                return Err(verr(format!("Agent {agent} MCP server references must not be empty.")));
            }
            if names.iter().any(|n| n == reference) {
                return Err(verr(format!("Agent {agent} repeats MCP server {reference}.")));
            }
            names.push(reference.into());
            continue;
        }
        if !matches!(spec, Js::Obj(_)) {
            return Err(verr(format!("Agent {agent} MCP specs must be server-name maps or string references.")));
        }
        if matches!(spec.get("start"), Js::Func) {
            return Err(unsup(format!(
                "Agent {agent} cannot place an SDK-hosted MCP server directly in an agent spec; configure it for the session and reference its name."
            )));
        }
        for (server_name, server) in spec.entries() {
            let normalized = trim(&server_name).to_string();
            if normalized.is_empty() {
                return Err(verr(format!("Agent {agent} MCP server names must not be empty.")));
            }
            if names.contains(&normalized) {
                return Err(verr(format!("Agent {agent} repeats MCP server {normalized}.")));
            }
            names.push(normalized.clone());
            validate_agent_mcp_transport(agent, &normalized, &server)?;
        }
    }
    Ok(())
}

fn skills_ok(v: &Js) -> bool {
    match v {
        Js::Arr(a) => !a.is_empty() && a.iter().all(non_empty_str) && !has_duplicates(a),
        _ => false,
    }
}

const UNSUPPORTED_OPTION_KEYS: [&str; 15] = [
    "agent",
    "pathToClaudeCodeExecutable",
    "env",
    "executable",
    "executableArgs",
    "extraArgs",
    "betas",
    "toolConfig",
    "allowDangerouslySkipPermissions",
    "permissionPromptToolName",
    "debug",
    "debugFile",
    "stderr",
    "spawnClaudeCodeProcess",
    "perTaskStopAffordance",
];

/// `validateOptions(options)`.
pub fn validate_options(options: &Js) -> R {
    let o = |k: &str| options.get(k);
    if o("continue").truthy() && o("resume").truthy() {
        return Err(verr("continue and resume cannot be used together."));
    }
    let rsa = o("resumeSessionAt");
    if !rsa.is_undef() && !non_empty_str(rsa) {
        return Err(verr("resumeSessionAt must be a non-empty transcript entry id."));
    }
    if !rsa.is_undef() && !o("resume").truthy() && !o("sessionId").truthy() {
        return Err(verr("resumeSessionAt requires resume or sessionId."));
    }
    let rdt = o("resumeDropsTurn");
    if !rdt.is_undef() && !non_empty_str(rdt) {
        return Err(verr("resumeDropsTurn must be a non-empty accepted-input entry id."));
    }
    if !rdt.is_undef() && rsa.is_undef() {
        return Err(verr("resumeDropsTurn requires resumeSessionAt."));
    }
    if matches!(o("persistSession"), Js::Bool(false))
        && ["sessionId", "resume", "continue", "forkSession", "resumeSessionAt"].iter().any(|k| o(k).truthy())
    {
        return Err(verr(
            "persistSession=false requires a new session and cannot be combined with sessionId, resume, continue, forkSession, or resumeSessionAt.",
        ));
    }
    let ps = o("promptSuggestions");
    if !ps.is_undef() && !matches!(ps, Js::Bool(_)) {
        return Err(verr("promptSuggestions must be a boolean."));
    }
    for key in UNSUPPORTED_OPTION_KEYS {
        if !o(key).is_undef() {
            return Err(unsup(format!("{key} is not yet exposed by the Gateway SDK.")));
        }
    }
    if !o("sandbox").is_undef() {
        normalize_sandbox(o("sandbox"))?;
    }
    let pd = o("pluginDelivery");
    if !pd.is_undef() && !pd.strict_eq_str("initialize") {
        return Err(unsup("PilotDeck loads session plugins through Gateway initialization; pluginDelivery=argv has no Gateway process equivalent."));
    }
    let plugins = o("plugins");
    if !plugins.is_undef() {
        let list = match plugins {
            Js::Arr(a) if !a.is_empty() => a,
            _ => return Err(verr("plugins must be a non-empty array of Gateway-local plugin directories.")),
        };
        let mut paths: Vec<&str> = vec![];
        for plugin in list {
            if !plugin.truthy() || !plugin.get("type").strict_eq_str("local") || !non_empty_str(plugin.get("path")) {
                return Err(verr("Each plugin must be a local plugin with a non-empty path."));
            }
            let p = plugin.get("path");
            if !is_absolute_path(p) {
                return Err(verr("Plugin paths must be absolute paths on the Gateway host."));
            }
            let p = p.as_str().unwrap_or("");
            if paths.contains(&p) {
                return Err(verr(format!("Plugin path is listed more than once: {p}")));
            }
            paths.push(p);
        }
    }
    for (k, msg) in [("agentProgressSummaries", "agentProgressSummaries must be a boolean."), ("forwardSubagentText", "forwardSubagentText must be a boolean.")] {
        if !o(k).is_undef() && !matches!(o(k), Js::Bool(_)) {
            return Err(verr(msg));
        }
    }
    if o("onElicitation").truthy() && o("onUserDialog").truthy() && supports_user_dialog_kind(options, "elicitation")? {
        return Err(verr(
            "onElicitation and onUserDialog are alternative adapters for elicitation; omit elicitation from supportedDialogKinds or configure only one.",
        ));
    }
    let udm = o("userDialogMode");
    if !udm.is_undef() && !udm.strict_eq_str("manual") {
        return Err(unsupported(&format!("userDialogMode={}", js_string(udm))));
    }
    let manual = udm.strict_eq_str("manual");
    let kinds = o("supportedDialogKinds");
    if manual && o("onUserDialog").truthy() {
        return Err(verr("userDialogMode manual and onUserDialog are mutually exclusive."));
    }
    if manual && kinds.is_undef() {
        return Err(verr("userDialogMode manual requires supportedDialogKinds."));
    }
    if manual && !kinds.nullish() && includes(kinds, "elicitation", "options.supportedDialogKinds?.includes")? {
        return Err(verr(
            "userDialogMode manual supports only generic input, select, confirm, and form dialogs; native elicitation requires onElicitation or onUserDialog.",
        ));
    }
    if !kinds.is_undef() {
        if !o("onUserDialog").truthy() && !manual {
            return Err(verr("supportedDialogKinds requires onUserDialog or userDialogMode manual."));
        }
        let list = match kinds {
            Js::Arr(a) if !a.is_empty() => a,
            _ => return Err(verr("supportedDialogKinds must be a non-empty array.")),
        };
        if has_duplicates(list) {
            return Err(verr("supportedDialogKinds cannot contain duplicates."));
        }
        if list.iter().any(|k| !["elicitation", "input", "select", "confirm", "form"].iter().any(|x| k.strict_eq_str(x))) {
            return Err(unsup("PilotDeck onUserDialog supports only elicitation and the experimental input, select, confirm, and form dialog kinds."));
        }
    }
    let skills = o("skills");
    if !skills.is_undef() && !skills.strict_eq_str("all") && !skills_ok(skills) {
        return Err(verr("skills must be \"all\" or a non-empty array of unique skill names."));
    }
    if o("sessionStore").truthy() && !o("projectKey").truthy() {
        return Err(verr("projectKey is required when sessionStore is configured."));
    }
    if o("hookServer").truthy() && !o("hooks").truthy() {
        return Err(verr("hookServer requires hooks."));
    }
    let pp = o("permissionPrompts");
    if !pp.is_undef() && !pp.strict_eq_str("host") && !pp.strict_eq_str("none") {
        return Err(verr("permissionPrompts must be host or none."));
    }
    let lt = o("loadTimeoutMs");
    if !lt.is_undef() && (!is_finite(lt) || lt.num().unwrap_or(0.0) <= 0.0) {
        return Err(verr("loadTimeoutMs must be a positive finite number."));
    }
    let ihe = o("includeHookEvents");
    if !ihe.is_undef() && !matches!(ihe, Js::Bool(_)) {
        return Err(verr("includeHookEvents must be a boolean."));
    }
    let fm = o("fallbackModel");
    if !fm.is_undef() && !non_empty_str(fm) {
        return Err(verr("fallbackModel must be a non-empty Gateway model catalog reference."));
    }
    let thinking = o("thinking");
    let (tt, td) = if thinking.nullish() { (&Js::Undefined, &Js::Undefined) } else { (thinking.get("type"), thinking.get("display")) };
    if !tt.strict_eq_str("disabled") && td.strict_eq_str("summarized") {
        return Err(unsup("PilotDeck exposes raw thinking deltas but does not provide Claude-style summarized thinking display."));
    }
    let mtt = o("maxThinkingTokens");
    if !mtt.nullish() && (!is_integer(mtt) || mtt.num().unwrap_or(0.0) < 0.0) {
        return Err(verr("maxThinkingTokens must be a non-negative integer or null."));
    }
    let rc = o("reconnect");
    if rc.truthy() {
        let (ma, idm, mdm, jit) = (rc.get("maxAttempts"), rc.get("initialDelayMs"), rc.get("maxDelayMs"), rc.get("jitter"));
        if !ma.is_undef() && (!is_integer(ma) || ma.num().unwrap_or(0.0) < 1.0) {
            return Err(verr("reconnect.maxAttempts must be a positive integer."));
        }
        if !idm.is_undef() && (!is_finite(idm) || idm.num().unwrap_or(0.0) < 0.0) {
            return Err(verr("reconnect.initialDelayMs must be non-negative."));
        }
        if !mdm.is_undef() && (!is_finite(mdm) || mdm.num().unwrap_or(0.0) < 0.0) {
            return Err(verr("reconnect.maxDelayMs must be non-negative."));
        }
        if !jit.is_undef() && (!is_finite(jit) || jit.num().is_some_and(|j| !(0.0..=1.0).contains(&j))) {
            return Err(verr("reconnect.jitter must be between 0 and 1."));
        }
        if let (Some(i), Some(m)) = (idm.num(), mdm.num()) {
            if m < i {
                return Err(verr("reconnect.maxDelayMs must be at least initialDelayMs."));
            }
        }
    }
    let sp = o("systemPrompt");
    if !sp.is_undef() {
        if matches!(sp, Js::Obj(_)) && sp.get("type").strict_eq_str("preset") {
            return Err(unsup("The Claude Code preset systemPrompt has no PilotDeck Gateway equivalent."));
        }
        normalize_system_prompt(sp)?;
    }
    let tools = o("tools");
    if tools.truthy() && !matches!(tools, Js::Arr(_)) {
        return Err(unsup("PilotDeck tool names differ from the Claude Code preset; pass an explicit PilotDeck tool-name array instead."));
    }
    for field in ["allowedTools", "disallowedTools"] {
        let v = o(field);
        if !v.is_undef() && !matches!(v, Js::Arr(a) if a.iter().all(non_empty_str)) {
            return Err(verr(format!("{field} must contain non-empty tool names.")));
        }
    }
    validate_deferred_tools(o("deferredTools"), "deferredTools")?;
    let mt = o("maxTurns");
    if !mt.is_undef() && (!is_integer(mt) || mt.num().unwrap_or(0.0) <= 0.0) {
        return Err(verr("maxTurns must be a positive integer."));
    }
    let mb = o("maxBudgetUsd");
    if !mb.is_undef() && (!is_finite(mb) || mb.num().unwrap_or(0.0) <= 0.0) {
        return Err(verr("maxBudgetUsd must be a positive finite number."));
    }
    let tb = o("taskBudget");
    if !tb.is_undef() && (!tb.is_object() || !is_finite(tb.get("total")) || tb.get("total").num().unwrap_or(0.0) <= 0.0) {
        return Err(verr("taskBudget.total must be a positive finite USD amount."));
    }
    let (scope, retention) = if tb.nullish() { (&Js::Undefined, &Js::Undefined) } else { (tb.get("scope"), tb.get("projectRetentionMs")) };
    if !scope.is_undef() && !scope.strict_eq_str("session") && !scope.strict_eq_str("project") {
        return Err(verr("taskBudget.scope must be session or project when provided."));
    }
    if !retention.is_undef() && !(is_integer(retention) && retention.num().is_some_and(|r| r.abs() <= 9007199254740991.0 && r > 0.0)) {
        return Err(verr("taskBudget.projectRetentionMs must be a positive safe integer in milliseconds."));
    }
    if !retention.is_undef() && !scope.strict_eq_str("project") {
        return Err(verr("taskBudget.projectRetentionMs requires taskBudget.scope to be project."));
    }
    let os = o("outputStyle");
    if !os.is_undef() && !non_empty_str(os) {
        return Err(verr("outputStyle must be a non-empty style name."));
    }
    let ad = o("additionalDirectories");
    match ad {
        Js::Undefined | Js::Null => {}
        Js::Arr(a) => {
            if a.iter().any(|d| !is_absolute_path(d)) {
                return Err(verr("additionalDirectories must contain absolute paths."));
            }
        }
        _ => return Err(type_err("options.additionalDirectories?.some is not a function")),
    }
    let ta = o("toolAliases");
    if ta.truthy() {
        for (from, to) in ta.entries() {
            if trim(&from).is_empty() || call_trim(&to, "to")?.is_empty() {
                return Err(verr("toolAliases must be a non-empty string-to-string map."));
            }
        }
    }
    let of = o("outputFormat");
    if of.truthy() && !is_pilotdeck_json_schema(of.get("schema")) {
        return Err(verr("outputFormat.schema must use the PilotDeck JSON-schema subset (type/properties/required/items/enum/additionalProperties)."));
    }
    let mcp = o("mcpServers");
    if mcp.truthy() {
        for (name, server) in mcp.entries() {
            if trim(&name).is_empty() {
                return Err(verr("MCP server names must not be empty."));
            }
            if is_hosted_mcp_server(&server) {
                continue;
            }
            let t = read(&server, "type")?;
            if t.strict_eq_str("stdio") && call_trim(server.get("command"), "server.command")?.is_empty() {
                return Err(verr(format!("MCP server {name} requires a command.")));
            }
            if is_http_type(t) && call_trim(server.get("url"), "server.url")?.is_empty() {
                return Err(verr(format!("MCP server {name} requires a URL.")));
            }
            if !t.strict_eq_str("stdio") && !is_http_type(t) {
                return Err(unsup(format!(
                    "MCP server {name} uses unsupported Claude transport {}. PilotDeck supports stdio, streamable_http, and legacy sse.",
                    js_string(t)
                )));
            }
            validate_deferred_tools(server.get("deferredTools"), &format!("MCP server {name} deferredTools"))?;
        }
    }
    let agents = o("agents");
    if agents.truthy() {
        let list = agents.entries();
        for (name, agent) in &list {
            if trim(name).is_empty() {
                return Err(verr("Agent names must not be empty."));
            }
            let required = |v: &Js, expr: &str| -> Result<bool, OptionsError> { Ok(opt_trim(v, expr)?.is_some_and(|s| !s.is_empty())) };
            if !agent.truthy()
                || !agent.is_object() && !matches!(agent, Js::Func)
                || !required(agent.get("description"), "agent.description")?
                || !required(agent.get("prompt"), "agent.prompt")?
            {
                return Err(verr(format!("Agent {name} requires a non-empty description and prompt.")));
            }
            let g = |k: &str| agent.get(k);
            if !g("background").is_undef() && !matches!(g("background"), Js::Bool(_)) {
                return Err(verr(format!("Agent {name} background must be a boolean when provided.")));
            }
            if !g("observer").is_undef() && !non_empty_str(g("observer")) {
                return Err(verr(format!("Agent {name} observer must name a non-empty AgentDefinition.")));
            }
            let om = g("observerMessage");
            if !om.is_undef() && !matches!(om, Js::Str(_)) {
                return Err(verr(format!("Agent {name} observerMessage must be a string when provided.")));
            }
            if opt_trim(om, "agent.observerMessage")?.is_some_and(|s| !s.is_empty()) && !g("observer").truthy() {
                return Err(verr(format!("Agent {name} observerMessage requires observer.")));
            }
            let any_blank = |v: &Js, expr: &str| -> Result<bool, OptionsError> {
                match v {
                    Js::Undefined | Js::Null => Ok(false),
                    Js::Arr(a) => {
                        for t in a {
                            if call_trim(t, "tool")?.is_empty() {
                                return Ok(true);
                            }
                        }
                        Ok(false)
                    }
                    _ => Err(type_err(format!("{expr}?.some is not a function"))),
                }
            };
            if any_blank(g("tools"), "agent.tools")? || any_blank(g("disallowedTools"), "agent.disallowedTools")? {
                return Err(verr(format!("Agent {name} tools and disallowedTools must contain non-empty names.")));
            }
            let amt = g("maxTurns");
            if !amt.is_undef() && (!is_integer(amt) || amt.num().unwrap_or(0.0) <= 0.0) {
                return Err(verr(format!("Agent {name} maxTurns must be a positive integer.")));
            }
            if !g("model").is_undef() && !non_empty_str(g("model")) {
                return Err(verr(format!("Agent {name} model must be a non-empty model catalog reference.")));
            }
            for field in ["initialPrompt", "criticalSystemReminder_EXPERIMENTAL"] {
                if !g(field).is_undef() && !non_empty_str(g(field)) {
                    return Err(verr(format!("Agent {name} {field} must be a non-empty string when provided.")));
                }
            }
            let effort = g("effort");
            if !effort.is_undef() && !["low", "medium", "high"].iter().any(|x| effort.strict_eq_str(x)) {
                return Err(verr(format!("Agent {name} effort must be low, medium, or high.")));
            }
            let pm = g("permissionMode");
            if !pm.is_undef() && !["default", "plan", "bypassPermissions"].iter().any(|x| pm.strict_eq_str(x)) {
                return Err(unsup(format!("Agent {name} permissionMode has no native equivalent.")));
            }
            let sk = g("skills");
            if !sk.is_undef() && !sk.strict_eq_str("all") && !skills_ok(sk) {
                return Err(verr(format!("Agent {name} skills must be \"all\" or a non-empty array of unique skill names.")));
            }
            let mem = g("memory");
            if !mem.is_undef() && !mem.strict_eq_str("inherit") && !mem.strict_eq_str("disabled") {
                return Err(verr(format!("Agent {name} memory must be \"inherit\" or \"disabled\".")));
            }
            if !g("mcpServers").is_undef() {
                validate_agent_mcp_servers(name, g("mcpServers"))?;
            }
        }
        for (name, agent) in &list {
            let observer = agent.get("observer");
            if !observer.truthy() {
                continue;
            }
            if observer.strict_eq_str(name) {
                return Err(verr(format!("Agent {name} cannot observe itself.")));
            }
            let target = js_string(observer);
            if !agents.get(&target).truthy() {
                return Err(verr(format!("Agent {name} observer {target} is not defined in options.agents.")));
            }
        }
    }
    Ok(())
}
