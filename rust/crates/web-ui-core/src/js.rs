//! Small JS string semantics helpers (UTF-16 indices, `\s`, `trim`).

/// ECMAScript WhiteSpace + LineTerminator (`\s`, `String.prototype.trim`).
pub fn is_js_ws(c: char) -> bool {
    matches!(c, '\u{9}' | '\u{a}' | '\u{b}' | '\u{c}' | '\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_js_ws)
}
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}
/// Byte offset of UTF-16 index `i` (must fall on a char boundary; callers clamp).
pub fn utf16_to_byte(s: &str, i: usize) -> usize {
    let mut u = 0;
    for (b, c) in s.char_indices() {
        if u >= i {
            return b;
        }
        u += c.len_utf16();
    }
    s.len()
}
