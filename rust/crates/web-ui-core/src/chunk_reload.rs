//! Port of `ui/src/utils/reloadOnChunkError.ts` (pure parts + the storage/reload protocol behind a trait).
use crate::js::is_js_ws;
use crate::js_value::to_js_string;
use serde_json::Value;

pub const RELOAD_MARKER: &str = "pilotdeck:chunk-load-reloaded";

/// A thrown value: an `Error` instance (name + message) or any other JS value (`None` = undefined).
#[derive(Clone, Debug, PartialEq)]
pub enum Thrown {
    Error { name: String, message: String },
    Value(Option<Value>),
}

/// `errorToText(error)`.
pub fn error_to_text(error: &Thrown) -> String {
    match error {
        Thrown::Error { name, message } => format!("{name}: {message}"),
        Thrown::Value(Some(Value::String(s))) => s.clone(),
        Thrown::Value(Some(v @ (Value::Object(_) | Value::Array(_)))) => match v.get("message") {
            Some(Value::String(m)) if v.is_object() => m.clone(),
            _ => to_js_string(v),
        },
        Thrown::Value(None) | Thrown::Value(Some(Value::Null)) => String::new(),
        Thrown::Value(Some(v)) => to_js_string(v),
    }
}

/// ASCII-only case folding, as non-unicode `/i` regexes do for these ASCII patterns.
fn find_ci(hay: &[char], needle: &str, from: usize) -> Option<usize> {
    let n: Vec<char> = needle.chars().collect();
    (from..=hay.len().checked_sub(n.len())?).find(|&i| n.iter().enumerate().all(|(k, c)| hay[i + k].to_ascii_lowercase() == *c && (hay[i + k].is_ascii() || hay[i + k] == *c)))
}

/// `/loading chunk \S+ failed/i`.
fn loading_chunk_failed(hay: &[char]) -> bool {
    let mut from = 0;
    while let Some(i) = find_ci(hay, "loading chunk ", from) {
        let start = i + "loading chunk ".len();
        let mut j = start;
        while j < hay.len() && !is_js_ws(hay[j]) {
            j += 1;
        }
        if j > start && find_ci(hay, " failed", j) == Some(j) {
            return true;
        }
        from = i + 1;
    }
    false
}

/// `isDynamicImportLoadError(error)`.
pub fn is_dynamic_import_load_error(error: &Thrown) -> bool {
    let text: Vec<char> = error_to_text(error).chars().collect();
    ["failed to fetch dynamically imported module", "importing a module script failed", "error loading dynamically imported module", "chunkloaderror", "unable to preload css"]
        .iter()
        .any(|p| find_ci(&text, p, 0).is_some())
        || loading_chunk_failed(&text)
}

/// `win.sessionStorage` + `win.location.reload` (storage calls may throw: `Err(())`).
pub trait ReloadWindow {
    fn get_item(&mut self, key: &str) -> Result<Option<String>, ()>;
    fn set_item(&mut self, key: &str, value: &str) -> Result<(), ()>;
    fn remove_item(&mut self, key: &str) -> Result<(), ()>;
    fn reload(&mut self);
}

/// `reloadOnceForDynamicImportError(error, win)`.
pub fn reload_once_for_dynamic_import_error(error: &Thrown, win: &mut dyn ReloadWindow) -> bool {
    if !is_dynamic_import_load_error(error) {
        return false;
    }
    if let Ok(v) = win.get_item(RELOAD_MARKER) {
        if v.as_deref() == Some("1") {
            return false;
        }
        let _ = win.set_item(RELOAD_MARKER, "1");
    }
    win.reload();
    true
}

/// `clearDynamicImportReloadMarker(win)`.
pub fn clear_dynamic_import_reload_marker(win: &mut dyn ReloadWindow) {
    let _ = win.remove_item(RELOAD_MARKER);
}
