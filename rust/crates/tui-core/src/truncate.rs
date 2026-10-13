//! `truncate.ts` (ANSI-free domain).
use crate::width::{slice_columns, string_width};

const MAX_LINES: usize = 3;
const PADDING: i64 = 10;

fn wrap_width(tw: i64) -> usize {
    (tw - PADDING).max(10) as usize
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// JS `String.prototype.slice(0, n)` in UTF-16 units (n never splits a pair in-domain;
/// a split surrogate is dropped, which is the lossy UTF-8 view of the lone unit).
fn js_slice_prefix(s: &str, n: usize) -> String {
    let u: Vec<u16> = s.encode_utf16().take(n).collect();
    String::from_utf16_lossy(&u)
}

/// JS `trimEnd` (ECMAScript WhiteSpace + LineTerminator).
pub fn is_js_ws(c: char) -> bool {
    matches!(c, '\u{9}' | '\u{a}' | '\u{b}' | '\u{c}' | '\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

pub fn js_trim_end(s: &str) -> &str {
    s.trim_end_matches(is_js_ws)
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut result = Vec::new();
    for raw in text.split('\n') {
        if string_width(raw) <= width {
            result.push(raw.to_string());
            continue;
        }
        let mut remaining = raw.to_string();
        while !remaining.is_empty() {
            if string_width(&remaining) <= width {
                result.push(remaining);
                break;
            }
            let (mut lo, mut hi, mut best) = (1usize, utf16_len(&remaining), 1usize);
            while lo <= hi {
                let mid = (lo + hi) / 2;
                if string_width(&slice_columns(&remaining, 0, Some(mid))) <= width {
                    best = mid;
                    lo = mid + 1;
                } else {
                    hi = mid - 1;
                }
            }
            result.push(slice_columns(&remaining, 0, Some(best)));
            remaining = slice_columns(&remaining, best, None);
        }
    }
    result
}

pub fn truncate_for_display(content: &str, terminal_width: i64) -> String {
    if content.is_empty() {
        return String::new();
    }
    let trimmed = js_trim_end(content);
    if trimmed.is_empty() {
        return String::new();
    }
    let ww = wrap_width(terminal_width);
    let budget = MAX_LINES * ww * 4;
    let source = if utf16_len(trimmed) > budget { js_slice_prefix(trimmed, budget) } else { trimmed.to_string() };
    let lines = wrap_text(&source, ww);
    if lines.len() <= MAX_LINES + 1 {
        return trimmed.to_string();
    }
    lines[..MAX_LINES].join("\n")
}

pub fn count_visual_lines(content: &str, terminal_width: i64) -> usize {
    if content.is_empty() {
        return 0;
    }
    wrap_text(content, wrap_width(terminal_width)).len()
}
