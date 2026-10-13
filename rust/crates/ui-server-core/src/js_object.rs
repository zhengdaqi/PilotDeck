//! JS object semantics on `serde_json::Value` (needs `preserve_order`).
use serde_json::{Map, Value};

/// A canonical array-index key ("0", "17"; below 2^32-1); JS lists these first, in ascending order.
pub fn array_index(k: &str) -> Option<u64> {
    if k == "0" {
        return Some(0);
    }
    if k.is_empty() || k.starts_with('0') || !k.bytes().all(|b| b.is_ascii_digit()) || k.len() > 10 {
        return None;
    }
    k.parse::<u64>().ok().filter(|n| *n < 4_294_967_295)
}

/// Reorders every object like `JSON.parse` + V8 would: index keys first (ascending), then the rest in insertion order.
pub fn js_order(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut idx: Vec<(u64, String, Value)> = vec![];
            let mut rest: Vec<(String, Value)> = vec![];
            for (k, v) in m {
                match array_index(&k) {
                    Some(n) => idx.push((n, k, js_order(v))),
                    None => rest.push((k, js_order(v))),
                }
            }
            idx.sort_by_key(|x| x.0);
            let mut out = Map::new();
            for (_, k, v) in idx {
                out.insert(k, v);
            }
            for (k, v) in rest {
                out.insert(k, v);
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(js_order).collect()),
        other => other,
    }
}

/// `value && typeof value === 'object' && !Array.isArray(value)` over JSON.
pub fn is_record(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Object(_)))
}

/// `obj?.name` for named (non-index) properties within the JSON domain.
pub fn field<'a>(v: Option<&'a Value>, name: &str) -> Option<&'a Value> {
    match v {
        Some(Value::Object(m)) => m.get(name),
        _ => None,
    }
}

pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        _ => true,
    }
}

/// `typeof v` and `String(v)` for a primitive, as used in V8's "Cannot create property" TypeError.
pub fn create_property_error(key: &str, on: &Value) -> String {
    let (ty, s) = match on {
        Value::String(s) => ("string", s.clone()),
        Value::Number(n) => ("number", n.to_string()),
        Value::Bool(b) => ("boolean", b.to_string()),
        _ => ("object", String::new()),
    };
    format!("TypeError: Cannot create property '{key}' on {ty} '{s}'")
}
