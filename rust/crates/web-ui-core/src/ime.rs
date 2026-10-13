//! Port of `ui/src/utils/ime.ts` over a JSON-shaped keyboard event.
use crate::js_value::truthy;
use serde_json::Value;

const IME_PROCESS_KEY_CODE: f64 = 229.0;

fn is229(v: Option<&Value>) -> bool {
    v.and_then(|x| x.as_f64()) == Some(IME_PROCESS_KEY_CODE)
}

pub fn is_ime_composition_event(event: &Value) -> bool {
    let native = event.get("nativeEvent").filter(|n| truthy(n));
    let nf = |k: &str| native.and_then(|n| n.get(k));
    event.get("isComposing").is_some_and(truthy)
        || nf("isComposing").is_some_and(truthy)
        || is229(event.get("keyCode"))
        || is229(event.get("which"))
        || is229(nf("keyCode"))
        || is229(nf("which"))
}

pub fn is_ime_enter_event(event: &Value) -> bool {
    event.get("key") == Some(&Value::String("Enter".into())) && is_ime_composition_event(event)
}
