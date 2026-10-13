//! Port of the pure part of `ui/src/utils/officePreviewStatus.ts`.
use crate::js::trim;
use crate::js_value::string_or_empty;
use serde_json::Value;

pub fn normalize_office_preview_service(value: Option<&Value>) -> &'static str {
    if trim(&string_or_empty(value)).to_lowercase() == "libreoffice" {
        "libreoffice"
    } else {
        "builtin"
    }
}

pub fn office_preview_renderer_signature(service: Option<&Value>, binary_path: Option<&Value>) -> String {
    let arr = Value::Array(vec![
        Value::String(normalize_office_preview_service(service).into()),
        Value::String(trim(&string_or_empty(binary_path)).into()),
    ]);
    serde_json::to_string(&arr).expect("json")
}

fn rec(v: Option<&Value>) -> Option<&serde_json::Map<String, Value>> {
    match v {
        Some(Value::Object(m)) => Some(m),
        _ => None,
    }
}

pub fn office_preview_renderer_signature_from_config(config: &Value) -> Option<String> {
    let c = rec(Some(config))?;
    let webui = rec(c.get("webui"))?;
    let op = rec(webui.get("officePreview"))?;
    Some(office_preview_renderer_signature(op.get("service"), op.get("binaryPath")))
}
