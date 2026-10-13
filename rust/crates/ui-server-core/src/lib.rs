//! Pure service layer of `ui/server` (the Node web backend), ported service by service.
#![allow(clippy::result_large_err, clippy::result_unit_err, clippy::type_complexity, clippy::comparison_chain)] // JsError mirrors thrown JS values; host traits report bare failure like a JS catch; JS-ordered if-chains kept for review against the source.
pub mod js_object;
pub mod js_number;
pub mod frontmatter;
pub mod js_yaml;
pub mod model_references;
pub mod command_parser;
pub mod node_path;
pub mod shell_quote;
pub mod settings_files;
pub mod services;
pub mod model_probe;
pub mod network_fetch;
pub mod web_update_command;
pub mod release_service;
pub mod config_parsers;
pub mod pilotdeck_config;
pub mod pilotdeck_config_write;
pub mod connection_test_tasks;
pub mod terminal_runtime;

/// Node's fs error message: `<CODE>: <description>, <syscall> '<path>'`.
pub fn node_fs_message(e: &std::io::Error, syscall: &str, path: &str) -> String {
    let (code, desc) = match e.raw_os_error() {
        Some(2) => ("ENOENT", "no such file or directory"),
        Some(13) => ("EACCES", "permission denied"),
        Some(20) => ("ENOTDIR", "not a directory"),
        Some(21) => ("EISDIR", "illegal operation on a directory"),
        Some(17) => ("EEXIST", "file already exists"),
        _ => return e.to_string(),
    };
    format!("{code}: {desc}, {syscall} '{path}'")
}

/// `String(v)` for a JSON value.
pub fn js_object_string(v: &serde_json::Value) -> String {
    use serde_json::Value;
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => js_number::js_number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(|x| if x.is_null() { String::new() } else { js_object_string(x) }).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}
