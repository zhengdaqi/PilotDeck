//! Width functions matching `string-width@8` (stringWidth) and `slice-ansi@9`
//! (getGraphemeWidth) on ANSI-free input. Tables are generated from Node.
use crate::tables::{EMOJI_PRESENTATION, FULLWIDTH, STRING_WIDTH};
use unicode_segmentation::UnicodeSegmentation;

fn lookup(t: &[(u32, i8)], cp: u32) -> i8 {
    let i = t.partition_point(|&(s, _)| s <= cp);
    t[i - 1].1
}

/// Grapheme clusters (Intl.Segmenter granularity "grapheme").
pub fn graphemes(s: &str) -> Vec<&str> {
    s.graphemes(true).collect()
}

/// `stringWidth(s)` for ANSI-free input. Fast path: printable ASCII.
/// Per-cluster: zero-width clusters add 0; otherwise the sum of per-code-point
/// widths (matches EAW of the first visible scalar plus trailing halfwidth forms
/// for the supported domain; RGI emoji sequences are out of domain).
pub fn string_width(s: &str) -> usize {
    if s.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
        return s.len();
    }
    s.chars().map(|c| lookup(STRING_WIDTH, c as u32).max(0) as usize).sum()
}

fn is_fullwidth(cp: u32) -> bool {
    lookup(FULLWIDTH, cp) == 1
}

/// slice-ansi `getGraphemeWidth`.
pub fn grapheme_width(g: &str) -> usize {
    let mut ri = 0;
    for c in g.chars() {
        let cp = c as u32;
        if is_fullwidth(cp) {
            return 2;
        }
        if (127_462..=127_487).contains(&cp) {
            ri += 1;
        }
    }
    if ri >= 1 {
        return 2;
    }
    if g.chars().any(|c| lookup(EMOJI_PRESENTATION, c as u32) == 1 || c as u32 == 0xFE0F || c as u32 == 0x20E3) {
        return 2;
    }
    1
}

/// `sliceAnsi(s, start, end)` on ANSI-free input: graphemes whose start column
/// is >= start, stopping at the first grapheme that would cross `end`.
pub fn slice_columns(s: &str, start: usize, end: Option<usize>) -> String {
    let mut out = String::new();
    let mut pos = 0usize;
    let mut include = false;
    for g in s.graphemes(true) {
        let w = grapheme_width(g);
        if let Some(e) = end {
            if pos >= e || pos + w > e {
                break;
            }
        }
        if !include && pos >= start {
            include = true;
        }
        if include {
            out.push_str(g);
        }
        pos += w;
    }
    out
}
