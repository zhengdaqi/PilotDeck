//! Small helpers that reproduce JavaScript string/number semantics the TS
//! implementation relies on (UTF-16 indices, `String.prototype.trim`, `\s`,
//! `Number(value)`), so observable output stays byte-compatible.

/// Characters matched by JS `\s` and stripped by `String.prototype.trim`.
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// `text.replace(/\s+/g, " ").trim()`
pub fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if is_js_space(c) {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
        } else {
            out.push(c);
            in_space = false;
        }
    }
    js_trim(&out).to_string()
}

pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Convert a UTF-8 byte offset into a UTF-16 code-unit offset.
pub fn byte_to_utf16(s: &str, byte: usize) -> usize {
    utf16_len(&s[..byte])
}

/// `s.slice(start, end)` using UTF-16 code-unit indices.
pub fn utf16_slice(s: &str, start: usize, end: usize) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    let end = end.min(units.len());
    let start = start.min(end);
    String::from_utf16_lossy(&units[start..end])
}

/// UTF-16 index of `needle` in `haystack` (like `indexOf`), or None.
pub fn utf16_index_of(haystack: &str, needle: &str) -> Option<usize> {
    haystack.find(needle).map(|b| byte_to_utf16(haystack, b))
}

/// JavaScript `Number(value)` for a string argument.
pub fn js_number(value: &str) -> f64 {
    let t = js_trim(value);
    if t.is_empty() {
        return 0.0;
    }
    let radix = |p: &str, r: u32| -> Option<f64> {
        let body = t.strip_prefix(p).or_else(|| t.strip_prefix(&p.to_uppercase()))?;
        if body.is_empty() || !body.chars().all(|c| c.is_digit(r)) {
            return Some(f64::NAN);
        }
        let mut acc = 0f64;
        for c in body.chars() {
            acc = acc * r as f64 + c.to_digit(r).unwrap() as f64;
        }
        Some(acc)
    };
    if let Some(v) = radix("0x", 16).or_else(|| radix("0o", 8)).or_else(|| radix("0b", 2)) {
        return v;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    // StrDecimalLiteral: [+-] digits [. digits] [e[+-]digits]
    let bytes = t.as_bytes();
    let mut i = 0;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i - int_start;
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let f = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        digits += i - f;
    }
    if digits == 0 {
        return f64::NAN;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        let e = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == e {
            return f64::NAN;
        }
    }
    if i != bytes.len() {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_semantics() {
        assert_eq!(js_number("5"), 5.0);
        assert_eq!(js_number(" 7 "), 7.0);
        assert_eq!(js_number("0x10"), 16.0);
        assert_eq!(js_number("1e2"), 100.0);
        assert_eq!(js_number(".5"), 0.5);
        assert_eq!(js_number("5."), 5.0);
        assert!(js_number("abc").is_nan());
        assert!(js_number("1_0").is_nan());
        assert!(js_number("Infinity").is_infinite());
        assert_eq!(js_number(""), 0.0);
    }

    #[test]
    fn whitespace_and_slices() {
        assert_eq!(collapse_whitespace("  a \n\t b\u{FEFF} "), "a b");
        assert_eq!(utf16_slice("a😀b", 1, 3), "😀");
        assert_eq!(utf16_index_of("😀x", "x"), Some(2));
    }
}
