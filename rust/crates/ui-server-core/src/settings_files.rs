//! Ports of `ui/server/services/permissionSettings.js` and `ui/server/services/mcpConfig.js`. The PilotDeck home
//! and env are parameters; `resolvePilotHome` is done by the caller.
use crate::node_path::resolve;
use pilotdeck_skills::jsjson::{stringify_pretty, JsObject, JsValue};
use std::collections::HashMap;

pub type Env = HashMap<String, String>;

fn alias(t: &str) -> Option<&'static str> {
    Some(match t {
        "Read" => "read_file",
        "Write" => "write_file",
        "Edit" | "MultiEdit" => "edit_file",
        "NotebookEdit" => "edit_notebook",
        "Glob" => "glob",
        "Grep" => "grep",
        "Bash" => "bash",
        "Task" => "agent",
        "TodoWrite" => "todo_write",
        "WebFetch" => "web_fetch",
        "WebSearch" => "web_search",
        _ => return None,
    })
}

/// `normalizePermissionEntry(entry)`.
pub fn normalize_permission_entry(entry: &JsValue) -> String {
    let JsValue::Str(s) = entry else { return String::new() };
    let trimmed = pilotdeck_web_ui_core::js::trim(s);
    if trimmed.is_empty() {
        return String::new();
    }
    // /^Bash\((.*)\)$/ : `.` excludes line terminators.
    if let Some(inner) = trimmed.strip_prefix("Bash(").and_then(|r| r.strip_suffix(')')) {
        if !inner.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
            let pattern = pilotdeck_web_ui_core::js::trim(inner);
            return if pattern.is_empty() { "bash".into() } else { format!("bash:{pattern}") };
        }
    }
    alias(trimmed).map(str::to_string).unwrap_or_else(|| trimmed.to_string())
}

fn normalize_string_array(v: Option<&JsValue>) -> JsValue {
    let Some(JsValue::Arr(a)) = v else { return JsValue::Arr(vec![]) };
    let mut out: Vec<String> = vec![];
    for item in a {
        let n = normalize_permission_entry(item);
        if n.is_empty() || out.contains(&n) {
            continue;
        }
        out.push(n);
    }
    JsValue::Arr(out.into_iter().map(JsValue::Str).collect())
}

fn js_truthy(v: Option<&JsValue>) -> bool {
    match v {
        None | Some(JsValue::Undefined) | Some(JsValue::Null) => false,
        Some(JsValue::Bool(b)) => *b,
        Some(JsValue::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(JsValue::Str(s)) => !s.is_empty(),
        _ => true,
    }
}

/// `normalizePermissionSettings(value)` (`lastUpdated: undefined` is dropped by JSON).
pub fn normalize_permission_settings(value: &JsValue) -> JsValue {
    let empty = JsObject::new();
    let obj = match value {
        JsValue::Obj(o) => o,
        _ => &empty,
    };
    let mut out = JsObject::new();
    out.set("version", JsValue::Num(1.0));
    for k in ["allowedTools", "askTools", "disallowedTools"] {
        out.set(k, normalize_string_array(obj.get(k)));
    }
    out.set("skipPermissions", JsValue::Bool(js_truthy(obj.get("skipPermissions"))));
    out.set("lastUpdated", match obj.get("lastUpdated") {
        Some(JsValue::Str(s)) => JsValue::Str(s.clone()),
        _ => JsValue::Undefined,
    });
    JsValue::Obj(out)
}

fn default_permission_settings() -> JsValue {
    let mut o = JsObject::new();
    o.set("version", JsValue::Num(1.0));
    for k in ["allowedTools", "askTools", "disallowedTools"] {
        o.set(k, JsValue::Arr(vec![]));
    }
    o.set("skipPermissions", JsValue::Bool(false));
    JsValue::Obj(o)
}

pub fn permission_settings_path(pilot_home: &str) -> String {
    resolve(&[pilot_home, "permissions.json"])
}

/// `readPermissionSettings(env)`: any read or parse failure gives the defaults.
pub fn read_permission_settings(pilot_home: &str) -> JsValue {
    match std::fs::read(permission_settings_path(pilot_home)) {
        Ok(b) => match pilotdeck_skills::v8json::parse(&String::from_utf8_lossy(&b)) {
            Ok(v) => normalize_permission_settings(&v),
            Err(_) => default_permission_settings(),
        },
        Err(_) => default_permission_settings(),
    }
}

/// `writePermissionSettings(updates, env)` with `now_iso` for `new Date().toISOString()`.
pub fn write_permission_settings(updates: &JsValue, pilot_home: &str, now_iso: &str) -> Result<JsValue, String> {
    let JsValue::Obj(mut merged) = read_permission_settings(pilot_home) else { unreachable!() };
    // `{ ...current, ...(updates || {}) , lastUpdated }`: spreading a string copies its characters as index keys,
    // and other primitives copy nothing.
    match updates {
        JsValue::Obj(u) => {
            for (k, v) in u.iter() {
                merged.set(k.clone(), v.clone());
            }
        }
        JsValue::Arr(a) => {
            for (i, v) in a.iter().enumerate() {
                merged.set(i.to_string(), v.clone());
            }
        }
        JsValue::Str(s) => {
            for (i, u) in s.encode_utf16().enumerate() {
                merged.set(i.to_string(), JsValue::Str(String::from_utf16_lossy(&[u])));
            }
        }
        _ => {}
    }
    merged.set("lastUpdated", JsValue::Str(now_iso.into()));
    let next = normalize_permission_settings(&JsValue::Obj(merged));
    let path = permission_settings_path(pilot_home);
    let dir = std::path::Path::new(&path).parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(&path, format!("{}\n", stringify_pretty(&next))).map_err(|e| e.to_string())?;
    Ok(next)
}

fn is_string_record(v: &JsValue) -> bool {
    matches!(v, JsValue::Obj(o) if o.iter().all(|(_, x)| matches!(x, JsValue::Str(_))))
}

/// `normalizeMcpConfig(input)`.
pub fn normalize_mcp_config(input: &JsValue) -> Result<JsValue, String> {
    let JsValue::Obj(o) = input else { return Err("MCP config root must be an object.".into()) };
    match o.get("mcpServers") {
        None | Some(JsValue::Undefined) => {
            let mut c = o.clone();
            c.set("mcpServers", JsValue::Obj(JsObject::new()));
            Ok(JsValue::Obj(c))
        }
        Some(JsValue::Obj(servers)) => {
            for (name, server) in servers.iter() {
                let JsValue::Obj(s) = server else { return Err(format!("Server \"{name}\" must be an object.")) };
                if name.is_empty() {
                    return Err(format!("Server \"{name}\" must be an object."));
                }
                let is_str = |k: &str| matches!(s.get(k), Some(JsValue::Str(_)));
                if !is_str("command") && !is_str("url") && !is_str("httpUrl") {
                    return Err(format!("Server \"{name}\" must define command, url, or httpUrl."));
                }
                match s.get("args") {
                    None | Some(JsValue::Undefined) => {}
                    Some(JsValue::Arr(a)) if a.iter().all(|x| matches!(x, JsValue::Str(_))) => {}
                    _ => return Err(format!("Server \"{name}\" args must be an array of strings.")),
                }
                for (k, label) in [("env", "env"), ("headers", "headers")] {
                    match s.get(k) {
                        None | Some(JsValue::Undefined) => {}
                        Some(v) if is_string_record(v) => {}
                        _ => return Err(format!("Server \"{name}\" {label} must be an object with string values.")),
                    }
                }
            }
            Ok(input.clone())
        }
        _ => Err("mcpServers must be an object.".into()),
    }
}

/// `isVirtualProjectPath(projectRoot, pilotHome, env)` (ui/server/utils/pilotPaths.js).
pub fn is_virtual_project_path(project_root: &str, pilot_home: &str, env: &Env, cwd: &str) -> bool {
    if project_root.is_empty() {
        return false;
    }
    let resolved = resolve(&[cwd, project_root]);
    if resolved == resolve(&[cwd, pilot_home]) {
        return true;
    }
    if env.get("PILOTDECK_DESKTOP").map(String::as_str) != Some("1") {
        return false;
    }
    if let Some(rt) = env.get("PILOTDECK_DESKTOP_RUNTIME_ROOT").filter(|s| !s.is_empty()) {
        if resolved == resolve(&[cwd, rt]) {
            return true;
        }
    }
    let lower = resolved.to_lowercase();
    lower.ends_with("/resources/runtime") || lower.ends_with("\\resources/runtime") || lower.ends_with("/resources\\runtime") || lower.ends_with("\\resources\\runtime")
}

fn default_mcp_config() -> JsValue {
    JsValue::Obj(JsObject::new().with("mcpServers", JsValue::Obj(JsObject::new())))
}

/// `readMcpConfigFile(scope, projectPath)` -> the result object, or the thrown message.
pub fn read_mcp_config_file(scope: &str, project_path: Option<&str>, pilot_home: &str, env: &Env, cwd: &str) -> Result<JsValue, String> {
    let project_disabled = scope == "project" && project_path.is_none_or(|p| p.is_empty() || is_virtual_project_path(p, pilot_home, env, cwd));
    if project_disabled {
        let mut o = JsObject::new();
        o.set("exists", JsValue::Bool(false));
        o.set("path", JsValue::Null);
        o.set("raw", JsValue::Str(stringify_pretty(&default_mcp_config())));
        o.set("config", default_mcp_config());
        o.set("disabled", JsValue::Bool(true));
        o.set("reason", JsValue::Str("Project MCP config requires a real project. Use global scope for general chat.".into()));
        return Ok(JsValue::Obj(o));
    }
    let file_path = if scope == "project" { resolve(&[cwd, project_path.unwrap(), ".pilotdeck", "mcp.json"]) } else { resolve(&[pilot_home, "mcp.json"]) };
    let mut o = JsObject::new();
    match std::fs::read(&file_path) {
        Ok(b) => {
            let parsed = normalize_mcp_config(&pilotdeck_skills::v8json::parse(&String::from_utf8_lossy(&b))?)?;
            o.set("exists", JsValue::Bool(true));
            o.set("path", JsValue::Str(file_path));
            o.set("raw", JsValue::Str(stringify_pretty(&parsed)));
            o.set("config", parsed);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            o.set("exists", JsValue::Bool(false));
            o.set("path", JsValue::Str(file_path));
            o.set("raw", JsValue::Str(stringify_pretty(&default_mcp_config())));
            o.set("config", default_mcp_config());
        }
        Err(e) if e.raw_os_error() == Some(21) => return Err("EISDIR: illegal operation on a directory, read".into()),
        Err(e) => return Err(crate::node_fs_message(&e, "open", &file_path)),
    }
    Ok(JsValue::Obj(o))
}

/// `writeMcpConfigFile(scope, raw, projectPath)`.
pub fn write_mcp_config_file(scope: &str, raw: &str, project_path: Option<&str>, pilot_home: &str, env: &Env, cwd: &str) -> Result<JsValue, String> {
    let parsed = normalize_mcp_config(&pilotdeck_skills::v8json::parse(raw)?)?;
    if scope == "project" && project_path.is_none_or(|p| p.is_empty() || is_virtual_project_path(p, pilot_home, env, cwd)) {
        return Err("Project MCP config requires a real project. Use global scope for general chat.".into());
    }
    let file_path = if scope == "project" { resolve(&[cwd, project_path.unwrap(), ".pilotdeck", "mcp.json"]) } else { resolve(&[pilot_home, "mcp.json"]) };
    let dir = std::path::Path::new(&file_path).parent().unwrap().to_path_buf();
    create_dir_all_mode(&dir).map_err(|e| crate::node_fs_message(&e, "mkdir", &dir.to_string_lossy()))?;
    write_file_mode(&file_path, format!("{}\n", stringify_pretty(&parsed)).as_bytes()).map_err(|e| crate::node_fs_message(&e, "open", &file_path))?;
    read_mcp_config_file(scope, project_path, pilot_home, env, cwd)
}

#[cfg(unix)]
fn create_dir_all_mode(p: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(p)
}
#[cfg(not(unix))]
fn create_dir_all_mode(p: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(p)
}
#[cfg(unix)]
fn write_file_mode(p: &str, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(p)?.write_all(data)
}
#[cfg(not(unix))]
fn write_file_mode(p: &str, data: &[u8]) -> std::io::Result<()> {
    std::fs::write(p, data)
}
