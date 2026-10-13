//! `yaml@2` `stringify(value, { lineWidth: 0, singleQuote: false })` for JSON-like values,
//! and `parse()` → JS value (via serde_yaml) for config documents.

use pilotdeck_skills::jsjson::{js_number_to_string, JsObject, JsValue};

fn is_core_non_string(s: &str) -> bool {
    use std::sync::OnceLock;
    static RE: OnceLock<Vec<regex_lite::Regex>> = OnceLock::new();
    let res = RE.get_or_init(|| {
        [
            r"^(?:~|[Nn]ull|NULL)?$",
            r"^(?:[Tt]rue|TRUE|[Ff]alse|FALSE)$",
            r"^0o[0-7]+$",
            r"^[-+]?[0-9]+$",
            r"^0x[0-9a-fA-F]+$",
            r"^[-+]?(?:\.[0-9]+|[0-9]+(?:\.[0-9]*)?)(?:[eE][-+]?[0-9]+)?$",
            r"^[-+]?\.(?:inf|Inf|INF)$",
            r"^\.(?:nan|NaN|NAN)$",
        ]
        .iter()
        .map(|p| regex_lite::Regex::new(p).unwrap())
        .collect()
    });
    res.iter().any(|r| r.is_match(s))
}

fn plain_ok(s: &str, implicit_key: bool) -> bool {
    use std::sync::OnceLock;
    static BAD: OnceLock<regex_lite::Regex> = OnceLock::new();
    let bad = BAD.get_or_init(|| {
        regex_lite::Regex::new(r#"^[\n\t ,\[\]{}#&*!|>'"%@`]|^[?-]$|^[?-][ \t]|[\n:][ \t]|[ \t]\n|[\n\t ]#|[\n\t :]$"#).unwrap()
    });
    if s.is_empty() || bad.is_match(s) {
        return false;
    }
    if implicit_key && s.contains('\n') {
        return false;
    }
    if s.contains('\n') {
        return false;
    }
    if s.chars().any(nonprintable) {
        return false;
    }
    !is_core_non_string(s)
}

fn nonprintable(c: char) -> bool {
    let u = c as u32;
    (u < 0x20 && c != '\t' && c != '\n') || (0x7f..=0x9f).contains(&u) || u == 0xfeff
}

fn double_quoted(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\0' => out.push_str("\\0"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{b}' => out.push_str("\\v"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '\u{1b}' => out.push_str("\\e"),
            '\u{85}' => out.push_str("\\N"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn block_literal(s: &str, indent: &str) -> Option<String> {
    // yaml@2 blockString: literal style for multi-line strings when not an implicit key.
    if s.chars().any(nonprintable) {
        return None;
    }
    let trimmed_end = s.trim_end_matches('\n');
    let trailing = s.len() - trimmed_end.len();
    let chomp = match trailing {
        0 => "-",
        1 => "",
        _ => "+",
    };
    let ind_indicator = if s.starts_with(' ') || s.starts_with('\n') && s.trim_start_matches('\n').starts_with(' ') { "2" } else { "" };
    let body = if trailing > 1 { &s[..s.len() - 1] } else { trimmed_end };
    let mut out = format!("|{ind_indicator}{chomp}");
    for line in body.split('\n') {
        out.push('\n');
        if !line.is_empty() {
            out.push_str(indent);
            out.push_str(line);
        }
    }
    Some(out)
}

fn scalar(v: &JsValue, implicit_key: bool, indent: &str) -> String {
    match v {
        JsValue::Null | JsValue::Undefined => "null".into(),
        JsValue::Bool(b) => b.to_string(),
        JsValue::Num(n) => {
            if n.is_nan() {
                ".nan".into()
            } else if n.is_infinite() {
                if *n > 0.0 { ".inf".into() } else { "-.inf".into() }
            } else if *n == 0.0 && n.is_sign_negative() {
                "-0".into()
            } else {
                js_number_to_string(*n)
            }
        }
        JsValue::Str(s) => {
            if plain_ok(s, implicit_key) {
                s.clone()
            } else if !implicit_key && s.contains('\n') {
                block_literal(s, indent).unwrap_or_else(|| double_quoted(s))
            } else {
                double_quoted(s)
            }
        }
        _ => unreachable!(),
    }
}

fn is_collection(v: &JsValue) -> bool {
    matches!(v, JsValue::Obj(o) if !o.is_empty()) || matches!(v, JsValue::Arr(a) if !a.is_empty())
}

fn node(v: &JsValue, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    match v {
        JsValue::Obj(o) if !o.is_empty() => {
            let mut first = true;
            for k in o.keys() {
                let val = o.get(&k).unwrap();
                if matches!(val, JsValue::Undefined) {
                    continue;
                }
                if !first {
                    out.push('\n');
                }
                first = false;
                out.push_str(&pad);
                if depth == 0 && (k.starts_with("---") || k.starts_with("...")) {
                    out.push_str(&double_quoted(&k));
                } else {
                    out.push_str(&scalar(&JsValue::Str(k.clone()), true, ""));
                }
                out.push(':');
                child(val, depth, out);
            }
        }
        JsValue::Arr(a) if !a.is_empty() => {
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                out.push_str(&pad);
                out.push('-');
                match item {
                    JsValue::Obj(o) if !o.is_empty() => {
                        // first key inline after "- "
                        let mut sub = String::new();
                        node(item, depth + 1, &mut sub);
                        out.push(' ');
                        out.push_str(sub.trim_start_matches(' '));
                        let _ = o;
                    }
                    JsValue::Arr(_) if is_collection(item) => {
                        let mut sub = String::new();
                        node(item, depth + 1, &mut sub);
                        out.push(' ');
                        out.push_str(sub.trim_start_matches(' '));
                    }
                    _ => {
                        out.push(' ');
                        out.push_str(&inline(item, &"  ".repeat(depth + 1)));
                    }
                }
            }
        }
        other => out.push_str(&inline(other, &pad)),
    }
}

fn inline(v: &JsValue, indent: &str) -> String {
    match v {
        JsValue::Obj(_) => "{}".into(),
        JsValue::Arr(_) => "[]".into(),
        s => scalar(s, false, indent),
    }
}

fn child(val: &JsValue, depth: usize, out: &mut String) {
    if is_collection(val) {
        out.push('\n');
        node(val, depth + 1, out);
    } else {
        out.push(' ');
        out.push_str(&inline(val, &"  ".repeat(depth + 1)));
    }
}

/// `stringify(value, { lineWidth: 0, singleQuote: false })` (always ends with "\n").
pub fn stringify(v: &JsValue) -> String {
    let mut out = String::new();
    node(v, 0, &mut out);
    out.push('\n');
    out
}

/// `yaml.parse(text)` (yaml@2, core schema) → JS value; Err for invalid YAML, multiple documents, duplicate keys.
pub fn parse(text: &str) -> Result<JsValue, String> {
    use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
    use yaml_rust2::scanner::{Marker, TScalarStyle};
    enum Frame {
        Seq(Vec<JsValue>, usize),
        Map(JsObject, Option<String>, usize, Vec<String>),
    }
    struct R {
        stack: Vec<Frame>,
        docs: Vec<JsValue>,
        anchors: std::collections::HashMap<usize, JsValue>,
        err: Option<String>,
    }
    fn key_of(v: &JsValue) -> String {
        match v {
            JsValue::Str(s) => s.clone(),
            JsValue::Null | JsValue::Undefined => "".into(),
            JsValue::Num(n) => js_number_to_string(*n),
            JsValue::Bool(b) => b.to_string(),
            other => pilotdeck_skills::jsjson::stringify_compact(other),
        }
    }
    impl R {
        fn push(&mut self, v: JsValue, anchor: usize) {
            if anchor > 0 {
                self.anchors.insert(anchor, v.clone());
            }
            match self.stack.last_mut() {
                None => self.docs.push(v),
                Some(Frame::Seq(a, _)) => a.push(v),
                Some(Frame::Map(o, pending, _, seen)) => match pending.take() {
                    None => {
                        let k = key_of(&v);
                        if seen.contains(&k) {
                            self.err = Some("Map keys must be unique".into());
                        }
                        seen.push(k.clone());
                        *pending = Some(k);
                    }
                    Some(k) => o.set(k, v),
                },
            }
        }
    }
    impl MarkedEventReceiver for R {
        fn on_event(&mut self, ev: Event, _m: Marker) {
            match ev {
                Event::Scalar(v, style, anchor, tag) => {
                    let is_str_tag = tag.as_ref().is_some_and(|t| t.suffix == "str");
                    let tag_suffix = tag.as_ref().map(|t| t.suffix.clone()).unwrap_or_default();
                    let val = if matches!(tag_suffix.as_str(), "int" | "float") {
                        match resolve_plain(&v) {
                            n @ JsValue::Num(_) => n,
                            _ => JsValue::Num(v.trim().parse::<f64>().unwrap_or(f64::NAN)),
                        }
                    } else if style == TScalarStyle::Plain && !is_str_tag {
                        resolve_plain(&v)
                    } else {
                        JsValue::Str(v)
                    };
                    self.push(val, anchor);
                }
                Event::Alias(id) => {
                    let v = self.anchors.get(&id).cloned().unwrap_or(JsValue::Null);
                    self.push(v, 0);
                }
                Event::SequenceStart(a, _) => self.stack.push(Frame::Seq(Vec::new(), a)),
                Event::MappingStart(a, _) => self.stack.push(Frame::Map(JsObject::new(), None, a, Vec::new())),
                Event::SequenceEnd => {
                    if let Some(Frame::Seq(v, a)) = self.stack.pop() {
                        self.push(JsValue::Arr(v), a);
                    }
                }
                Event::MappingEnd => {
                    if let Some(Frame::Map(mut o, pending, a, _)) = self.stack.pop() {
                        if let Some(k) = pending {
                            o.set(k, JsValue::Null);
                        }
                        self.push(JsValue::Obj(o), a);
                    }
                }
                _ => {}
            }
        }
    }
    if text.lines().any(|l| l.starts_with('\t') && !l.trim().is_empty()) {
        // yaml@2 rejects tab indentation (approximation: any line beginning with a tab).
        return Err("Tabs are not allowed as indentation".into());
    }
    let mut r = R { stack: vec![], docs: vec![], anchors: Default::default(), err: None };
    let mut p = Parser::new_from_str(text);
    p.load(&mut r, true).map_err(|e| e.to_string())?;
    if let Some(e) = r.err {
        return Err(e);
    }
    if r.docs.len() > 1 {
        return Err("Source contains multiple documents".into());
    }
    Ok(r.docs.pop().unwrap_or(JsValue::Null))
}

fn resolve_plain(s: &str) -> JsValue {
    use regex_lite::Regex;
    use std::sync::OnceLock;
    static R: OnceLock<[Regex; 7]> = OnceLock::new();
    let [null, boolean, oct, int, hex, float_exp, float] = R.get_or_init(|| {
        [
            Regex::new(r"^(?:~|[Nn]ull|NULL)?$").unwrap(),
            Regex::new(r"^(?:[Tt]rue|TRUE|[Ff]alse|FALSE)$").unwrap(),
            Regex::new(r"^0o[0-7]+$").unwrap(),
            Regex::new(r"^[-+]?[0-9]+$").unwrap(),
            Regex::new(r"^0x[0-9a-fA-F]+$").unwrap(),
            Regex::new(r"^[-+]?(?:\.[0-9]+|[0-9]+(?:\.[0-9]*)?)[eE][-+]?[0-9]+$").unwrap(),
            Regex::new(r"^[-+]?(?:\.[0-9]+|[0-9]+\.[0-9]*)$").unwrap(),
        ]
    });
    if null.is_match(s) {
        return JsValue::Null;
    }
    if boolean.is_match(s) {
        return JsValue::Bool(s.starts_with(['t', 'T']));
    }
    if oct.is_match(s) {
        return JsValue::Num(u128::from_str_radix(&s[2..], 8).map(|v| v as f64).unwrap_or(f64::INFINITY));
    }
    if hex.is_match(s) {
        return JsValue::Num(u128::from_str_radix(&s[2..], 16).map(|v| v as f64).unwrap_or(f64::INFINITY));
    }
    if int.is_match(s) || float_exp.is_match(s) || float.is_match(s) {
        let t = s.trim_start_matches('+');
        let t = if t.ends_with('.') { format!("{t}0") } else { t.to_string() };
        let t = if t.starts_with('.') { format!("0{t}") } else if t.starts_with("-.") { format!("-0{}", &t[1..]) } else { t };
        return JsValue::Num(t.parse::<f64>().unwrap_or(f64::NAN));
    }
    match s {
        ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" => JsValue::Num(f64::INFINITY),
        "-.inf" | "-.Inf" | "-.INF" => JsValue::Num(f64::NEG_INFINITY),
        ".nan" | ".NaN" | ".NAN" => JsValue::Num(f64::NAN),
        _ => JsValue::Str(s.to_string()),
    }
}
