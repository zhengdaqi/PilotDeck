//! Port of `src/permission/settings.ts` (PILOT_HOME/permissions.json).
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct PermissionSettings {
    pub version: u8,
    #[serde(rename = "allowedTools")]
    pub allowed_tools: Vec<String>,
    #[serde(rename = "askTools")]
    pub ask_tools: Vec<String>,
    #[serde(rename = "disallowedTools")]
    pub disallowed_tools: Vec<String>,
    #[serde(rename = "skipPermissions")]
    pub skip_permissions: bool,
    #[serde(rename = "lastUpdated", skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,
}

pub fn default_settings() -> PermissionSettings {
    PermissionSettings { version: 1, ..Default::default() }
}

/// Partial update, mirroring `Partial<PermissionSettings>`.
#[derive(Debug, Clone, Default)]
pub struct SettingsPatch {
    pub allowed_tools: Option<Vec<String>>,
    pub disallowed_tools: Option<Vec<String>>,
    pub skip_permissions: Option<bool>,
}

pub fn settings_path(pilot_home: &Path) -> PathBuf {
    pilot_home.join("permissions.json")
}

pub fn is_js_ws(c: char) -> bool {
    pilotdeck_tui_core::truncate::is_js_ws(c)
}
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_ws)
}

const ALIASES: [(&str, &str); 12] = [
    ("Read", "read_file"), ("Write", "write_file"), ("Edit", "edit_file"), ("NotebookEdit", "edit_notebook"),
    ("MultiEdit", "edit_file"), ("Glob", "glob"), ("Grep", "grep"), ("Bash", "bash"), ("Task", "agent"),
    ("TodoWrite", "todo_write"), ("WebFetch", "web_fetch"), ("WebSearch", "web_search"),
];

pub fn normalize_permission_entry(entry: &str) -> String {
    let t = js_trim(entry);
    if t.is_empty() {
        return String::new();
    }
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^Bash\(([^\n\r\u{2028}\u{2029}]*)\)$").unwrap());
    if let Some(c) = re.captures(t) {
        let p = js_trim(c.get(1).map(|m| m.as_str()).unwrap_or(""));
        return if p.is_empty() { "bash".into() } else { format!("bash:{p}") };
    }
    ALIASES.iter().find(|(k, _)| *k == t).map(|(_, v)| v.to_string()).unwrap_or_else(|| t.to_string())
}

fn norm_array(v: Option<&Value>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(Value::Array(a)) = v {
        for item in a {
            if let Value::String(s) = item {
                let n = normalize_permission_entry(s);
                if n.is_empty() || out.contains(&n) {
                    continue;
                }
                out.push(n);
            }
        }
    }
    out
}

/// JS `Boolean(x)` for JSON values.
pub fn js_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0 && !f.is_nan()).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

pub fn normalize_permission_settings(v: &Value) -> PermissionSettings {
    let empty = serde_json::Map::new();
    let r = v.as_object().unwrap_or(&empty);
    PermissionSettings {
        version: 1,
        allowed_tools: norm_array(r.get("allowedTools")),
        ask_tools: norm_array(r.get("askTools")),
        disallowed_tools: norm_array(r.get("disallowedTools")),
        skip_permissions: js_truthy(r.get("skipPermissions")),
        last_updated: r.get("lastUpdated").and_then(Value::as_str).map(str::to_string),
    }
}

pub fn read_permission_settings(pilot_home: &Path) -> PermissionSettings {
    std::fs::read_to_string(settings_path(pilot_home))
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .map(|v| normalize_permission_settings(&v))
        .unwrap_or_else(default_settings)
}

fn to_value(s: &PermissionSettings) -> Value {
    serde_json::to_value(s).unwrap()
}

pub fn write_permission_settings(pilot_home: &Path, patch: SettingsPatch) -> PermissionSettings {
    let mut cur = to_value(&read_permission_settings(pilot_home));
    let o = cur.as_object_mut().unwrap();
    if let Some(a) = patch.allowed_tools {
        o.insert("allowedTools".into(), a.into());
    }
    if let Some(a) = patch.disallowed_tools {
        o.insert("disallowedTools".into(), a.into());
    }
    if let Some(b) = patch.skip_permissions {
        o.insert("skipPermissions".into(), b.into());
    }
    o.insert("lastUpdated".into(), chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string().into());
    let next = normalize_permission_settings(&cur);
    let path = settings_path(pilot_home);
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    // JSON.stringify(next, null, 2) + "\n"; field order follows the TS object literal.
    let _ = std::fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&next).unwrap()));
    next
}
