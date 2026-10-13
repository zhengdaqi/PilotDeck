//! A JS-object-model value plus a `JSON.stringify(value, null, 2)` writer:
//! integer-like keys enumerate first (ascending), other keys keep insertion
//! order, numbers use ECMAScript `Number#toString`, non-finite numbers → null.

#[derive(Debug, Clone, PartialEq)]
pub enum JsValue {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<JsValue>),
    Obj(JsObject),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct JsObject {
    entries: Vec<(String, JsValue)>,
}

fn array_index(key: &str) -> Option<u32> {
    if key == "0" {
        return Some(0);
    }
    if key.is_empty() || key.starts_with('0') || !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    key.parse::<u64>().ok().filter(|v| *v < u32::MAX as u64).map(|v| v as u32)
}

impl JsObject {
    pub fn new() -> Self {
        Self::default()
    }

    /// `obj[key] = value` semantics.
    pub fn set(&mut self, key: impl Into<String>, value: JsValue) {
        let key = key.into();
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
            return;
        }
        self.entries.push((key, value));
    }

    pub fn with(mut self, key: &str, value: JsValue) -> Self {
        self.set(key, value);
        self
    }

    pub fn get(&self, key: &str) -> Option<&JsValue> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Own enumerable keys in ECMAScript order.
    fn ordered(&self) -> Vec<&(String, JsValue)> {
        let mut ints: Vec<(u32, &(String, JsValue))> =
            self.entries.iter().filter_map(|e| array_index(&e.0).map(|i| (i, e))).collect();
        ints.sort_by_key(|(i, _)| *i);
        let mut out: Vec<&(String, JsValue)> = ints.into_iter().map(|(_, e)| e).collect();
        out.extend(self.entries.iter().filter(|e| array_index(&e.0).is_none()));
        out
    }
}

pub fn js_number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if n == 0.0 {
        return "0".into();
    }
    let mut buf = ryu_js::Buffer::new();
    buf.format(n).to_string()
}

pub fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write(v: &JsValue, indent: usize, out: &mut String) -> bool {
    let pad = |n: usize| "  ".repeat(n);
    match v {
        JsValue::Undefined => return false,
        JsValue::Null => out.push_str("null"),
        JsValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JsValue::Num(n) => {
            if n.is_finite() {
                out.push_str(&js_number_to_string(*n))
            } else {
                out.push_str("null")
            }
        }
        JsValue::Str(s) => quote(s, out),
        JsValue::Arr(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return true;
            }
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                out.push_str(&pad(indent + 1));
                if !write(item, indent + 1, out) {
                    out.push_str("null");
                }
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad(indent));
            out.push(']');
        }
        JsValue::Obj(obj) => {
            let entries: Vec<_> =
                obj.ordered().into_iter().filter(|(_, v)| !matches!(v, JsValue::Undefined)).collect();
            if entries.is_empty() {
                out.push_str("{}");
                return true;
            }
            out.push_str("{\n");
            for (i, (k, v)) in entries.iter().enumerate() {
                out.push_str(&pad(indent + 1));
                quote(k, out);
                out.push_str(": ");
                write(v, indent + 1, out);
                if i + 1 < entries.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad(indent));
            out.push('}');
        }
    }
    true
}

/// `JSON.stringify(v, null, 2)`
pub fn stringify_pretty(v: &JsValue) -> String {
    let mut out = String::new();
    write(v, 0, &mut out);
    out
}

pub fn s(v: impl Into<String>) -> JsValue {
    JsValue::Str(v.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_order_and_numbers() {
        let o = JsObject::new()
            .with("b", JsValue::Num(1.0))
            .with("10", JsValue::Num(0.1))
            .with("2", JsValue::Num(1e21))
            .with("a", JsValue::Arr(vec![]));
        assert_eq!(
            stringify_pretty(&JsValue::Obj(o)),
            "{\n  \"2\": 1e+21,\n  \"10\": 0.1,\n  \"b\": 1,\n  \"a\": []\n}"
        );
    }
}

fn write_compact(v: &JsValue, out: &mut String) -> bool {
    match v {
        JsValue::Undefined => return false,
        JsValue::Null => out.push_str("null"),
        JsValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JsValue::Num(n) => out.push_str(&if n.is_finite() { js_number_to_string(*n) } else { "null".into() }),
        JsValue::Str(s) => quote(s, out),
        JsValue::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if !write_compact(item, out) {
                    out.push_str("null");
                }
            }
            out.push(']');
        }
        JsValue::Obj(obj) => {
            out.push('{');
            let mut first = true;
            for (k, v) in obj.ordered() {
                if matches!(v, JsValue::Undefined) {
                    continue;
                }
                if !first {
                    out.push(',');
                }
                first = false;
                quote(k, out);
                out.push(':');
                write_compact(v, out);
            }
            out.push('}');
        }
    }
    true
}

/// `JSON.stringify(v)`
pub fn stringify_compact(v: &JsValue) -> String {
    let mut out = String::new();
    write_compact(v, &mut out);
    out
}
