//! Small helpers over the JS object model (`JsValue`) so ported code reads close to the TS.
pub use pilotdeck_skills::jsjson::{stringify_compact, JsObject, JsValue};

pub static UNDEF: JsValue = JsValue::Undefined;

pub trait IntoJs {
    fn into_js(self) -> JsValue;
}
impl IntoJs for JsValue {
    fn into_js(self) -> JsValue {
        self
    }
}
impl IntoJs for &JsValue {
    fn into_js(self) -> JsValue {
        self.clone()
    }
}
impl IntoJs for &str {
    fn into_js(self) -> JsValue {
        JsValue::Str(self.to_string())
    }
}
impl IntoJs for String {
    fn into_js(self) -> JsValue {
        JsValue::Str(self)
    }
}
impl IntoJs for &String {
    fn into_js(self) -> JsValue {
        JsValue::Str(self.clone())
    }
}
impl IntoJs for bool {
    fn into_js(self) -> JsValue {
        JsValue::Bool(self)
    }
}
impl IntoJs for f64 {
    fn into_js(self) -> JsValue {
        JsValue::Num(self)
    }
}
impl IntoJs for i64 {
    fn into_js(self) -> JsValue {
        JsValue::Num(self as f64)
    }
}
impl IntoJs for usize {
    fn into_js(self) -> JsValue {
        JsValue::Num(self as f64)
    }
}
impl IntoJs for Vec<JsValue> {
    fn into_js(self) -> JsValue {
        JsValue::Arr(self)
    }
}
impl IntoJs for JsObject {
    fn into_js(self) -> JsValue {
        JsValue::Obj(self)
    }
}
impl<T: IntoJs> IntoJs for Option<T> {
    fn into_js(self) -> JsValue {
        match self {
            Some(v) => v.into_js(),
            None => JsValue::Undefined,
        }
    }
}

#[macro_export]
macro_rules! obj {
    ($($k:expr => $v:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut o = $crate::json::JsObject::new();
        $( o.set($k, $crate::json::IntoJs::into_js($v)); )*
        $crate::json::JsValue::Obj(o)
    }};
}

pub fn get<'a>(v: &'a JsValue, k: &str) -> &'a JsValue {
    match v {
        JsValue::Obj(o) => o.get(k).unwrap_or(&UNDEF),
        _ => &UNDEF,
    }
}
pub fn str_of(v: &JsValue) -> Option<&str> {
    match v {
        JsValue::Str(s) => Some(s),
        _ => None,
    }
}
pub fn gs<'a>(v: &'a JsValue, k: &str) -> Option<&'a str> {
    str_of(get(v, k))
}
pub fn num(v: &JsValue) -> Option<f64> {
    match v {
        JsValue::Num(n) => Some(*n),
        _ => None,
    }
}
pub fn gn(v: &JsValue, k: &str) -> Option<f64> {
    num(get(v, k))
}
pub fn gb(v: &JsValue, k: &str) -> Option<bool> {
    match get(v, k) {
        JsValue::Bool(b) => Some(*b),
        _ => None,
    }
}
pub fn arr(v: &JsValue) -> Option<&Vec<JsValue>> {
    match v {
        JsValue::Arr(a) => Some(a),
        _ => None,
    }
}
pub fn ga<'a>(v: &'a JsValue, k: &str) -> &'a [JsValue] {
    arr(get(v, k)).map(|a| a.as_slice()).unwrap_or(&[])
}
pub fn is_obj(v: &JsValue) -> bool {
    matches!(v, JsValue::Obj(_))
}
pub fn is_undef(v: &JsValue) -> bool {
    matches!(v, JsValue::Undefined)
}
/// Truthiness for JS values.
pub fn truthy(v: &JsValue) -> bool {
    match v {
        JsValue::Undefined | JsValue::Null => false,
        JsValue::Bool(b) => *b,
        JsValue::Num(n) => *n != 0.0 && !n.is_nan(),
        JsValue::Str(s) => !s.is_empty(),
        _ => true,
    }
}
pub fn set(v: &mut JsValue, k: &str, val: impl IntoJs) {
    if let JsValue::Obj(o) = v {
        o.set(k, val.into_js());
    }
}
pub fn remove(v: &mut JsValue, k: &str) {
    if let JsValue::Obj(o) = v {
        o.remove(k);
    }
}
/// `{ ...base, ...extra }`
pub fn spread(base: &JsValue, extra: JsValue) -> JsValue {
    let mut out = match base {
        JsValue::Obj(o) => o.clone(),
        _ => JsObject::new(),
    };
    if let JsValue::Obj(e) = extra {
        for (k, v) in e.iter() {
            out.set(k.clone(), v.clone());
        }
    }
    JsValue::Obj(out)
}
/// JSON round-trip (structuredClone for protocol values; drops undefined like JSON).
pub fn json_clone(v: &JsValue) -> JsValue {
    pilotdeck_skills::v8json::parse(&stringify_compact(v)).unwrap_or(JsValue::Undefined)
}
pub fn s(v: &str) -> JsValue {
    JsValue::Str(v.to_string())
}
pub fn to_json(v: &JsValue) -> String {
    stringify_compact(v)
}
