//! Port of `ui/server/utils/frontmatter.js`: `gray-matter@4.0.3` with js-yaml 3.14 `safeLoad`.
//! JS/JSON engines are disabled and return `{}`.
//!
//! The YAML syntax is parsed by `yaml-rust2`. Node construction follows js-yaml's DEFAULT_SAFE_SCHEMA:
//! - implicit types: null, bool, int (including 0b/0x/legacy octal/sexagesimal), float, timestamp, merge;
//! - explicit types: !!str/!!seq/!!map/!!binary/!!omap/!!pairs/!!set;
//! - keys are converted to strings; duplicate keys are an error; `<<` merges.
//!
//! YAML error messages are not reproduced; only whether parsing throws is.
//!
//! Intended differences (ADR-0004):
//! - SRC-DEFECT-013: TS turns a timestamp used as a mapping key into `String(date)`. That string depends on the
//!   server's time zone and locale. Rust keeps the key's source text.
//! - SRC-DEFECT-014: TS also accepts `Object.prototype` member names (`constructor`, `toString`, ...) as engine names
//!   after `---`. Rust looks up own engine names only and reports the engine as not registered.
use crate::js_number::js_number_to_string;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser, Tag};
use yaml_rust2::scanner::{Marker, TScalarStyle};

/// A js-yaml result value.
#[derive(Debug, Clone)]
pub enum Y {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    /// Milliseconds since the epoch (NaN when invalid), plus the source text.
    Date(f64, String),
    Buffer(Vec<u8>),
    Arr(Vec<Y>),
    Obj(Vec<(String, Y)>),
}

impl Y {
    /// `JSON.stringify` semantics, in V8 key order.
    pub fn to_json(&self) -> Value {
        match self {
            Y::Null => Value::Null,
            Y::Bool(b) => Value::Bool(*b),
            Y::Num(f) => num_json(*f),
            Y::Str(s) => Value::String(s.clone()),
            Y::Date(ms, _) => iso_string(*ms).map_or(Value::Null, Value::String),
            Y::Buffer(b) => json!({"type": "Buffer", "data": b}),
            Y::Arr(a) => Value::Array(a.iter().map(Y::to_json).collect()),
            Y::Obj(m) => crate::js_object::js_order(Value::Object(m.iter().map(|(k, v)| (k.clone(), v.to_json())).collect::<Map<_, _>>())),
        }
    }
    fn truthy(&self) -> bool {
        match self {
            Y::Null => false,
            Y::Bool(b) => *b,
            Y::Num(f) => *f != 0.0 && !f.is_nan(),
            Y::Str(s) => !s.is_empty(),
            _ => true,
        }
    }
}

fn num_json(f: f64) -> Value {
    if !f.is_finite() {
        return Value::Null;
    }
    if f.fract() == 0.0 && f.abs() < 1e21 {
        if f.abs() < 9.2e18 {
            return Value::from(f as i64);
        }
        if f > 0.0 && f < 1.8e19 {
            return Value::from(f as u64);
        }
    }
    serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { yoe + era * 400 + 1 } else { yoe + era * 400 }, m, d)
}

/// `Date.UTC(year, month, day, h, mi, s, ms)` including the 0..99 → 1900s year rule and TimeClip.
fn date_utc(year: i64, month: i64, day: i64, h: i64, mi: i64, s: i64, ms: i64) -> f64 {
    let year = if (0..=99).contains(&year) { 1900 + year } else { year };
    let ym = year + month.div_euclid(12);
    let mn = month.rem_euclid(12);
    let days = days_from_civil(ym, mn + 1, 1) + day - 1;
    time_clip((days * 86_400_000 + h * 3_600_000 + mi * 60_000 + s * 1000 + ms) as f64)
}

fn time_clip(t: f64) -> f64 {
    if !t.is_finite() || t.abs() > 8.64e15 { f64::NAN } else { t }
}

/// `Date.prototype.toISOString` (None for an invalid date, where `toJSON` gives null).
pub fn iso_string(ms: f64) -> Option<String> {
    if ms.is_nan() {
        return None;
    }
    let ms = ms as i64;
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    let (y, m, d) = civil_from_days(days);
    let year = if (0..=9999).contains(&y) { format!("{y:04}") } else if y < 0 { format!("-{:06}", -y) } else { format!("+{y:06}") };
    Some(format!("{year}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000))
}

fn re(cell: &'static OnceLock<Regex>, src: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(src).unwrap())
}

fn resolve_int(data: &str) -> bool {
    let b: Vec<char> = data.chars().collect();
    let max = b.len();
    if max == 0 {
        return false;
    }
    let mut i = 0;
    let mut ch = b[0];
    if ch == '-' || ch == '+' {
        i += 1;
        // `data[++index]` past the end is undefined.
        match b.get(i) {
            Some(c) => ch = *c,
            None => return false,
        }
    }
    let mut has = false;
    if ch == '0' {
        if i + 1 == max {
            return true;
        }
        i += 1;
        ch = b[i];
        let (radix_ok, start): (fn(char) -> bool, usize) = match ch {
            'b' => (|c| c == '0' || c == '1', i + 1),
            'x' => (|c| c.is_ascii_hexdigit(), i + 1),
            _ => (|c| ('0'..='7').contains(&c), i),
        };
        for &c in &b[start..] {
            ch = c;
            if c == '_' {
                continue;
            }
            if !radix_ok(c) {
                return false;
            }
            has = true;
        }
        return has && ch != '_';
    }
    if ch == '_' {
        return false;
    }
    while i < max {
        ch = b[i];
        if ch == '_' {
            i += 1;
            continue;
        }
        if ch == ':' {
            break;
        }
        if !ch.is_ascii_digit() {
            return false;
        }
        has = true;
        i += 1;
    }
    if !has || ch == '_' {
        return false;
    }
    if ch != ':' {
        return true;
    }
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"^(:[0-5]?[0-9])+$").is_match(&b[i..].iter().collect::<String>())
}

/// JS `parseInt(s, radix)` for the digit strings js-yaml passes (leading digits only).
fn parse_int(s: &str, radix: u32) -> f64 {
    let mut v = f64::NAN;
    for c in s.chars() {
        match c.to_digit(radix) {
            Some(d) => v = if v.is_nan() { d as f64 } else { v * radix as f64 + d as f64 },
            None => break,
        }
    }
    v
}

fn construct_int(data: &str) -> f64 {
    let mut value: String = data.replace('_', "");
    let mut sign = 1.0;
    if value.starts_with('-') || value.starts_with('+') {
        if value.starts_with('-') {
            sign = -1.0;
        }
        value.remove(0);
    }
    if value == "0" {
        return 0.0 * sign;
    }
    if let Some(r) = value.strip_prefix('0') {
        if let Some(bits) = r.strip_prefix('b') {
            return sign * parse_int(bits, 2);
        }
        if let Some(hex) = r.strip_prefix('x') {
            return sign * parse_int(hex, 16);
        }
        return sign * parse_int(&value, 8);
    }
    if value.contains(':') {
        let (mut v, mut base) = (0.0, 1.0);
        for d in value.split(':').rev() {
            v += parse_int(d, 10) * base;
            base *= 60.0;
        }
        return sign * v;
    }
    sign * parse_int(&value, 10)
}

fn resolve_float(data: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"^(?:[-+]?(?:0|[1-9][0-9_]*)(?:\.[0-9_]*)?(?:[eE][-+]?[0-9]+)?|\.[0-9_]+(?:[eE][-+]?[0-9]+)?|[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))$")
        .is_match(data)
        && !data.ends_with('_')
}

/// JS `parseFloat` over the strings js-yaml passes (already validated, `_` removed).
fn parse_float(s: &str) -> f64 {
    static R: OnceLock<Regex> = OnceLock::new();
    let m = re(&R, r"^[-+]?(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:[eE][-+]?[0-9]+)?").find(s);
    m.and_then(|m| m.as_str().parse::<f64>().ok()).unwrap_or(f64::NAN)
}

fn construct_float(data: &str) -> f64 {
    let mut value = data.replace('_', "").to_lowercase();
    let sign = if value.starts_with('-') { -1.0 } else { 1.0 };
    if value.starts_with('+') || value.starts_with('-') {
        value.remove(0);
    }
    if value == ".inf" {
        return sign * f64::INFINITY;
    }
    if value == ".nan" {
        return f64::NAN;
    }
    if value.contains(':') {
        let (mut v, mut base) = (0.0, 1.0);
        for d in value.split(':').rev() {
            v += parse_float(d) * base;
            base *= 60.0;
        }
        return sign * v;
    }
    sign * parse_float(&value)
}

fn timestamp(data: &str) -> Option<f64> {
    static D: OnceLock<Regex> = OnceLock::new();
    static T: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = re(&D, r"^([0-9]{4})-([0-9]{2})-([0-9]{2})$").captures(data) {
        let n = |i: usize| c[i].parse::<i64>().unwrap();
        return Some(date_utc(n(1), n(2) - 1, n(3), 0, 0, 0, 0));
    }
    let c = re(&T, r"^([0-9]{4})-([0-9][0-9]?)-([0-9][0-9]?)(?:[Tt]|[ \t]+)([0-9][0-9]?):([0-9]{2}):([0-9]{2})(?:\.([0-9]*))?(?:[ \t]*(Z|([-+])([0-9][0-9]?)(?::([0-9]{2}))?))?$").captures(data)?;
    let n = |i: usize| c.get(i).map_or(0, |m| m.as_str().parse::<i64>().unwrap());
    let mut fraction = 0;
    if let Some(f) = c.get(7).filter(|m| !m.as_str().is_empty()) {
        let mut s: String = f.as_str().chars().take(3).collect();
        while s.len() < 3 {
            s.push('0');
        }
        fraction = s.parse().unwrap();
    }
    let mut t = date_utc(n(1), n(2) - 1, n(3), n(4), n(5), n(6), fraction);
    if let Some(sign) = c.get(9) {
        let mut delta = ((n(10) * 60 + n(11)) * 60000) as f64;
        if sign.as_str() == "-" {
            delta = -delta;
        }
        if delta != 0.0 {
            t = time_clip(t - delta);
        }
    }
    Some(t)
}

/// Implicit resolution of a plain scalar (js-yaml order: null, bool, int, float, timestamp; merge is handled by keys).
fn resolve_plain(s: &str) -> Y {
    match s {
        "" | "~" | "null" | "Null" | "NULL" => return Y::Null,
        "true" | "True" | "TRUE" => return Y::Bool(true),
        "false" | "False" | "FALSE" => return Y::Bool(false),
        _ => {}
    }
    if resolve_int(s) {
        return Y::Num(construct_int(s));
    }
    if resolve_float(s) {
        return Y::Num(construct_float(s));
    }
    if let Some(ms) = timestamp(s) {
        return Y::Date(ms, s.to_string());
    }
    Y::Str(s.to_string())
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    // js-yaml binary: ignores \n\r and requires the rest to be base64 with valid padding positions.
    const MAP: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=\n\r";
    let mut bits = 0usize;
    for c in s.chars() {
        let code = MAP.find(c)?;
        if code > 64 {
            continue;
        }
        bits += 6;
    }
    if bits % 8 != 0 {
        return None;
    }
    let input: Vec<usize> = s.chars().filter_map(|c| MAP.find(c)).filter(|&c| c < 64).collect();
    let mut out = vec![];
    let mut b: u32 = 0;
    for (i, d) in input.iter().enumerate() {
        if i % 4 == 0 && i > 0 {
            out.extend([(b >> 16) as u8, (b >> 8) as u8, b as u8]);
        }
        b = (b << 6) | *d as u32;
    }
    match input.len() % 4 {
        0 if !input.is_empty() => out.extend([(b >> 16) as u8, (b >> 8) as u8, b as u8]),
        3 => out.extend([(b >> 10) as u8, (b >> 2) as u8]),
        2 => out.push((b >> 4) as u8),
        _ => {}
    }
    Some(out)
}

fn tag_name(t: &Tag) -> String {
    format!("{}{}", t.handle, t.suffix)
}
const Y_TAG: &str = "tag:yaml.org,2002:";

/// `String(key)` for a mapping key (SRC-DEFECT-013: dates keep their source text).
fn key_string(k: &Y) -> Result<String, String> {
    Ok(match k {
        Y::Arr(a) => {
            let mut parts = vec![];
            for x in a {
                parts.push(match x {
                    Y::Arr(_) => return Err("nested arrays are not supported inside keys".into()),
                    Y::Null => String::new(),
                    other => key_string(other)?,
                });
            }
            parts.join(",")
        }
        Y::Obj(_) => "[object Object]".into(),
        Y::Null => "null".into(),
        Y::Bool(b) => b.to_string(),
        Y::Num(f) => js_number_to_string(*f),
        Y::Str(s) => s.clone(),
        Y::Date(_, raw) => raw.clone(),
        Y::Buffer(b) => String::from_utf8_lossy(b).into_owned(),
    })
}

struct Events(Vec<(Event, Marker)>);
impl MarkedEventReceiver for Events {
    fn on_event(&mut self, ev: Event, m: Marker) {
        self.0.push((ev, m));
    }
}

struct Composer {
    ev: Vec<Event>,
    pos: usize,
    anchors: HashMap<usize, Y>,
}

enum Node {
    Value(Y),
    /// A key that is the merge type (`<<` plain, or `!!merge`).
    Merge,
}

impl Composer {
    fn next(&mut self) -> Event {
        let e = self.ev.get(self.pos).cloned().unwrap_or(Event::StreamEnd);
        self.pos += 1;
        e
    }

    fn node(&mut self, as_key: bool) -> Result<Node, String> {
        match self.next() {
            Event::Alias(id) => self.anchors.get(&id).cloned().map(Node::Value).ok_or_else(|| "unidentified alias".to_string()),
            Event::Scalar(v, style, anchor, tag) => {
                let y = match tag.as_ref().map(tag_name) {
                    None if style == TScalarStyle::Plain => {
                        if as_key && v == "<<" {
                            return Ok(Node::Merge);
                        }
                        resolve_plain(&v)
                    }
                    None => Y::Str(v),
                    Some(t) if t == "!" => Y::Str(v),
                    Some(t) => {
                        let Some(name) = t.strip_prefix(Y_TAG) else { return Err(format!("unknown tag !<{t}>")) };
                        let bad = || format!("cannot resolve a node with !<{t}> explicit tag");
                        let raw = v.clone();
                        match name {
                            "str" => Y::Str(raw),
                            "null" if matches!(raw.as_str(), "~" | "null" | "Null" | "NULL") => Y::Null,
                            "bool" if matches!(raw.as_str(), "true" | "True" | "TRUE") => Y::Bool(true),
                            "bool" if matches!(raw.as_str(), "false" | "False" | "FALSE") => Y::Bool(false),
                            "int" if resolve_int(&raw) => Y::Num(construct_int(&raw)),
                            "float" if resolve_float(&raw) => Y::Num(construct_float(&raw)),
                            "timestamp" => Y::Date(timestamp(&raw).ok_or_else(bad)?, raw),
                            "merge" if raw == "<<" => {
                                if as_key {
                                    return Ok(Node::Merge);
                                }
                                Y::Str(raw)
                            }
                            "binary" => Y::Buffer(base64_decode(&raw).ok_or_else(bad)?),
                            "seq" | "map" | "omap" | "pairs" | "set" => return Err(format!("unacceptable node kind for !<{t}> tag")),
                            _ => return Err(if ["null", "bool", "int", "float", "merge"].contains(&name) { bad() } else { format!("unknown tag !<{t}>") }),
                        }
                    }
                };
                if anchor > 0 {
                    self.anchors.insert(anchor, y.clone());
                }
                Ok(Node::Value(y))
            }
            Event::SequenceStart(anchor, tag) => {
                let mut items = vec![];
                loop {
                    if matches!(self.ev.get(self.pos), Some(Event::SequenceEnd)) {
                        self.pos += 1;
                        break;
                    }
                    match self.node(false)? {
                        Node::Value(v) => items.push(v),
                        Node::Merge => unreachable!(),
                    }
                }
                let y = match tag.as_ref().map(tag_name).as_deref() {
                    None | Some("!") => Y::Arr(items),
                    Some(t) if t == format!("{Y_TAG}seq") => Y::Arr(items),
                    Some(t) if t == format!("{Y_TAG}omap") => {
                        let mut seen = HashSet::new();
                        for it in &items {
                            let Y::Obj(m) = it else { return Err("omap".into()) };
                            if m.len() != 1 || !seen.insert(m[0].0.clone()) {
                                return Err("omap".into());
                            }
                        }
                        Y::Arr(items)
                    }
                    Some(t) if t == format!("{Y_TAG}pairs") => {
                        let mut out = vec![];
                        for it in items {
                            let Y::Obj(m) = it else { return Err("pairs".into()) };
                            if m.len() != 1 {
                                return Err("pairs".into());
                            }
                            let (k, v) = m.into_iter().next().unwrap();
                            out.push(Y::Arr(vec![Y::Str(k), v]));
                        }
                        Y::Arr(out)
                    }
                    Some(t) => return Err(format!("unknown or unacceptable tag !<{t}> for a sequence")),
                };
                if anchor > 0 {
                    self.anchors.insert(anchor, y.clone());
                }
                Ok(Node::Value(y))
            }
            Event::MappingStart(anchor, tag) => {
                let mut result: Vec<(String, Y)> = vec![];
                let mut overridable: HashSet<String> = HashSet::new();
                loop {
                    if matches!(self.ev.get(self.pos), Some(Event::MappingEnd)) {
                        self.pos += 1;
                        break;
                    }
                    let key = self.node(true)?;
                    let Node::Value(value) = self.node(false)? else { unreachable!() };
                    match key {
                        Node::Merge => {
                            let sources = match value {
                                Y::Arr(a) => a,
                                other => vec![other],
                            };
                            for src in sources {
                                let entries: Vec<(String, Y)> = match src {
                                    Y::Obj(m) => m,
                                    Y::Arr(a) => a.into_iter().enumerate().map(|(i, v)| (i.to_string(), v)).collect(),
                                    Y::Buffer(b) => b.into_iter().enumerate().map(|(i, v)| (i.to_string(), Y::Num(v as f64))).collect(),
                                    Y::Date(..) => vec![],
                                    _ => return Err("cannot merge mappings; the provided source object is unacceptable".into()),
                                };
                                // Object.keys order: index keys first.
                                let mut entries = entries;
                                let mut idx: Vec<(u64, (String, Y))> = vec![];
                                let mut rest = vec![];
                                for e in entries.drain(..) {
                                    match crate::js_object::array_index(&e.0) {
                                        Some(n) => idx.push((n, e)),
                                        None => rest.push(e),
                                    }
                                }
                                idx.sort_by_key(|x| x.0);
                                for (k, v) in idx.into_iter().map(|x| x.1).chain(rest) {
                                    if !result.iter().any(|(rk, _)| *rk == k) {
                                        result.push((k.clone(), v));
                                        overridable.insert(k);
                                    }
                                }
                            }
                        }
                        Node::Value(k) => {
                            let k = key_string(&k)?;
                            match result.iter_mut().find(|(rk, _)| *rk == k) {
                                Some(slot) => {
                                    if !overridable.contains(&k) {
                                        return Err("duplicated mapping key".into());
                                    }
                                    slot.1 = value;
                                }
                                None => result.push((k.clone(), value)),
                            }
                            overridable.remove(&k);
                        }
                    }
                }
                let y = match tag.as_ref().map(tag_name).as_deref() {
                    None | Some("!") => Y::Obj(result),
                    Some(t) if t == format!("{Y_TAG}map") => Y::Obj(result),
                    Some(t) if t == format!("{Y_TAG}set") => {
                        if result.iter().any(|(_, v)| !matches!(v, Y::Null)) {
                            return Err("set".into());
                        }
                        Y::Obj(result)
                    }
                    Some(t) => return Err(format!("unknown or unacceptable tag !<{t}> for a mapping")),
                };
                if anchor > 0 {
                    self.anchors.insert(anchor, y.clone());
                }
                Ok(Node::Value(y))
            }
            other => Err(format!("unexpected event {other:?}")),
        }
    }
}

/// js-yaml 3 `safeLoad` (None when the stream has no document, which JS returns as undefined).
pub fn yaml_safe_load(src: &str) -> Result<Option<Y>, String> {
    let mut rec = Events(vec![]);
    Parser::new_from_str(src).load(&mut rec, true).map_err(|e| e.to_string())?;
    let ev: Vec<Event> = rec.0.into_iter().map(|x| x.0).collect();
    let docs = ev.iter().filter(|e| matches!(e, Event::DocumentStart)).count();
    if docs > 1 {
        return Err("expected a single document in the stream, but found more".into());
    }
    if docs == 0 {
        return Ok(None);
    }
    let start = ev.iter().position(|e| matches!(e, Event::DocumentStart)).unwrap() + 1;
    let mut c = Composer { ev, pos: start, anchors: HashMap::new() };
    match c.node(false)? {
        Node::Value(v) => Ok(Some(v)),
        Node::Merge => Ok(Some(Y::Str("<<".into()))),
    }
}

/// The parts of the gray-matter file object that callers use.
#[derive(Debug, Clone)]
pub struct Matter {
    /// `undefined` (None) only when `safeLoad` saw no document and excerpt did not run.
    pub data: Option<Y>,
    pub content: String,
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `matter.replace(/^\s*#[^\n]+/gm, '')`.
fn strip_comment_lines(s: &str) -> String {
    let is_ws = pilotdeck_web_ui_core::js::is_js_ws;
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let line_start = i == 0 || is_line_terminator(chars[i - 1]);
        if line_start {
            let mut j = i;
            while j < chars.len() && is_ws(chars[j]) {
                j += 1;
            }
            if j < chars.len() && chars[j] == '#' && j + 1 < chars.len() && chars[j + 1] != '\n' {
                let mut e = j + 1;
                while e < chars.len() && chars[e] != '\n' {
                    e += 1;
                }
                i = e;
                // The next position (at '\n') is not a line start, so the loop goes on normally.
                if i < chars.len() {
                    out.push(chars[i]);
                    i += 1;
                }
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// `parseFrontmatter(content)` (gray-matter with the frontmatter.js options).
pub fn parse_frontmatter(input: &str) -> Result<Matter, String> {
    let empty = || Some(Y::Obj(vec![]));
    if input.is_empty() {
        return Ok(Matter { data: empty(), content: String::new() });
    }
    let content = input.strip_prefix('\u{feff}').unwrap_or(input);
    let Some(after_open) = content.strip_prefix("---") else {
        return Ok(Matter { data: empty(), content: content.to_string() });
    };
    if after_open.starts_with('-') {
        return Ok(Matter { data: empty(), content: content.to_string() });
    }
    let mut s = after_open;
    // matter.language: the first line (up to \r?\n); with no newline, every character but the last.
    let raw: &str = match s.find('\n') {
        Some(p) => &s[..if p > 0 && s.as_bytes()[p - 1] == b'\r' { p - 1 } else { p }],
        None => match s.char_indices().last() {
            Some((i, _)) => &s[..i],
            None => "",
        },
    };
    let name = pilotdeck_web_ui_core::js::trim(raw);
    let mut language = "yaml".to_string();
    if !name.is_empty() {
        language = name.to_string();
        s = &s[raw.len()..];
    }
    let len = s.len();
    let close_index = s.find("\n---").unwrap_or(len);
    let matter = &s[..close_index];
    let block = strip_comment_lines(matter);
    let data = if pilotdeck_web_ui_core::js::trim(&block).is_empty() {
        empty()
    } else {
        // Engines: own names only (SRC-DEFECT-014), then the lower-cased alias.
        let engine = |n: &str| -> Option<&'static str> {
            match n {
                "yaml" => Some("yaml"),
                "json" | "js" | "javascript" => Some("disabled"),
                _ => None,
            }
        };
        let alias = match language.to_lowercase().as_str() {
            "js" | "javascript" => "javascript".to_string(),
            "yaml" | "yml" => "yaml".to_string(),
            "coffee" | "coffeescript" | "cson" => "coffee".to_string(),
            _ => language.clone(),
        };
        match engine(&language).or_else(|| engine(&alias)) {
            Some("yaml") => yaml_safe_load(matter)?,
            Some(_) => empty(),
            None => return Err(format!("gray-matter engine \"{language}\" is not registered")),
        }
    };
    let content = if close_index == len {
        String::new()
    } else {
        let c = &s[close_index + 4..];
        let c = c.strip_prefix('\r').unwrap_or(c);
        c.strip_prefix('\n').unwrap_or(c).to_string()
    };
    // excerpt(): `file.data == null` becomes {}.
    let data = match data {
        None | Some(Y::Null) => empty(),
        d => d,
    };
    Ok(Matter { data, content })
}

/// `parseCommand(content)` -> `{ data, content, raw }` as JSON.
pub fn parse_command(content: &str) -> Result<Value, String> {
    let m = parse_frontmatter(content).map_err(|e| format!("Failed to parse command: {e}"))?;
    let data = match m.data {
        Some(d) if d.truthy() => d.to_json(),
        _ => json!({}),
    };
    Ok(json!({ "data": data, "content": m.content, "raw": content }))
}
