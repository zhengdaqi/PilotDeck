//! JS coercions over JSON values (`String(v)`, truthiness).
use serde_json::Value;

pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// `Number.prototype.toString()` for the finite numbers JSON carries. Exact for integers below 1e21 and for
/// decimals whose shortest round-trip form has no exponent (Rust's `{}` and V8 agree there).
pub fn number_to_string(f: f64) -> String {
    if f == 0.0 {
        return "0".into();
    }
    if f.fract() == 0.0 && f.abs() < 1e21 {
        return format!("{}", f as i128);
    }
    format!("{f}")
}

/// `String(v)`: arrays join with "," (null becomes ""), plain objects give "[object Object]".
pub fn to_js_string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => number_to_string(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(|x| if x.is_null() { String::new() } else { to_js_string(x) }).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// `String(v || '')`.
pub fn string_or_empty(v: Option<&Value>) -> String {
    match v {
        Some(x) if truthy(x) => to_js_string(x),
        _ => String::new(),
    }
}

/// `ToNumber(v)` (NaN for objects/arrays except the trivial cases handled by callers).
pub fn to_number(v: &Value) -> f64 {
    match v {
        Value::Null => 0.0,
        Value::Bool(b) => *b as u8 as f64,
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => {
            let t = crate::js::trim(s);
            if t.is_empty() {
                return 0.0;
            }
            match t {
                "Infinity" | "+Infinity" => return f64::INFINITY,
                "-Infinity" => return f64::NEG_INFINITY,
                _ => {}
            }
            for (pre, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
                if let Some(d) = t.strip_prefix(pre) {
                    return u128::from_str_radix(d, radix).map_or(f64::NAN, |n| n as f64);
                }
            }
            if t.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E')) {
                t.parse::<f64>().unwrap_or(f64::NAN)
            } else {
                f64::NAN
            }
        }
        _ => f64::NAN,
    }
}
