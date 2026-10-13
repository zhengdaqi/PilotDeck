//! Port of the pure part of `ui/src/utils/externalFileDrop.ts` (DOM listeners live in the Leptos components).
use crate::workspace_file_mention::get_workspace_relative_path;
use serde_json::Value;

/// `isExternalFileDrag({ dataTransfer })`: `Array.from(types).includes('Files')`.
pub fn is_external_file_drag(data_transfer: Option<&Value>) -> bool {
    let Some(types) = data_transfer.and_then(|d| d.get("types")) else { return false };
    let files = Value::String("Files".into());
    match types {
        Value::Array(a) => a.contains(&files),
        Value::Object(o) => {
            // Array-like: ToLength(length), then o[0..len).
            let len = o.get("length").map(crate::js_value::to_number).map_or(0.0, |f| if f.is_nan() || f <= 0.0 { 0.0 } else { f.floor() });
            (0..len.min(1e6) as usize).any(|i| o.get(&i.to_string()) == Some(&files))
        }
        // Strings iterate single code points (never "Files"); other primitives give [] (or are falsy).
        _ => false,
    }
}

pub fn resolve_external_file_drop_target_path(drop_path: Option<&str>, drop_type: Option<&str>, workspace_root: &str) -> String {
    let Some(drop_path) = drop_path.filter(|p| !p.is_empty()) else { return String::new() };
    let Some(rel) = get_workspace_relative_path(drop_path, workspace_root).filter(|r| !r.is_empty()) else { return String::new() };
    if drop_type == Some("directory") {
        return rel;
    }
    match rel.rfind('/') {
        None => String::new(),
        Some(i) => rel[..i].to_string(),
    }
}
