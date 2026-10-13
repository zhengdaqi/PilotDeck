//! `countTokens` (src/context/budget/tokenizer.ts): js-tiktoken o200k_base, `encode(text, [], [])`.
//! Ranks come from js-tiktoken (`o200k_base.bin`, regenerate with rust/parity/gen_o200k.mjs).
//! The pre-tokenizer is js-tiktoken's `pat_str` run with JS `u` semantics. The `regex` crate has no
//! lookahead, so `\s+(?!\S)` is emulated (see `pieces`), and `\s` is spelled out as the JS class.
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

const JS_WS: &str = r"\t\n\x0B\x0C\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}";
const CONTRACTION: &str = "('s|'S|'t|'T|'re|'rE|'Re|'RE|'ve|'vE|'Ve|'VE|'m|'M|'ll|'lL|'Ll|'LL|'d|'D)?";

pub fn is_js_ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{B}' | '\u{C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}'
        | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}')
}

fn pattern() -> &'static Regex {
    static P: OnceLock<Regex> = OnceLock::new();
    P.get_or_init(|| {
        let pat = format!(
            r"[^\r\n\p{{L}}\p{{N}}]?[\p{{Lu}}\p{{Lt}}\p{{Lm}}\p{{Lo}}\p{{M}}]*[\p{{Ll}}\p{{Lm}}\p{{Lo}}\p{{M}}]+{c}|[^\r\n\p{{L}}\p{{N}}]?[\p{{Lu}}\p{{Lt}}\p{{Lm}}\p{{Lo}}\p{{M}}]+[\p{{Ll}}\p{{Lm}}\p{{Lo}}\p{{M}}]*{c}|\p{{N}}{{1,3}}| ?[^{ws}\p{{L}}\p{{N}}]+[\r\n/]*|[{ws}]*[\r\n]+|[{ws}]+",
            c = CONTRACTION, ws = JS_WS);
        Regex::new(&pat).expect("o200k pattern")
    })
}

fn ranks() -> &'static HashMap<&'static [u8], u32> {
    static R: OnceLock<HashMap<&'static [u8], u32>> = OnceLock::new();
    R.get_or_init(|| {
        let data: &'static [u8] = include_bytes!("o200k_base.bin");
        let mut map = HashMap::with_capacity(200_000);
        let mut i = 0;
        while i + 5 <= data.len() {
            let rank = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
            let len = data[i + 4] as usize;
            map.insert(&data[i + 5..i + 5 + len], rank);
            i += 5 + len;
        }
        map
    })
}

/// Pre-tokenized pieces, with `\s+(?!\S)` semantics: a newline-free whitespace run of 2+ chars that
/// is followed by a non-whitespace char gives up its last char (JS backtracking).
fn pieces(text: &str) -> Vec<&str> {
    let re = pattern();
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < text.len() {
        let Some(m) = re.find_at(text, pos) else { break };
        let (s, mut e) = (m.start(), m.end());
        let piece = &text[s..e];
        if e < text.len() && piece.chars().all(is_js_ws) && !piece.contains(['\r', '\n']) && piece.chars().count() >= 2 {
            let last = piece.chars().last().unwrap();
            e -= last.len_utf8();
        }
        out.push(&text[s..e]);
        pos = e;
    }
    out
}

fn byte_pair_count(piece: &[u8], ranks: &HashMap<&'static [u8], u32>) -> usize {
    if piece.len() == 1 {
        return 1;
    }
    // bytePairMerge: repeatedly merge the adjacent pair with the lowest rank (leftmost on ties).
    let mut bounds: Vec<usize> = (0..=piece.len()).collect();
    while bounds.len() > 2 {
        let mut best: Option<(u32, usize)> = None;
        for i in 0..bounds.len() - 2 {
            if let Some(&r) = ranks.get(&piece[bounds[i]..bounds[i + 2]]) {
                if best.is_none_or(|(b, _)| r < b) {
                    best = Some((r, i));
                }
            }
        }
        match best {
            Some((_, i)) => {
                bounds.remove(i + 1);
            }
            None => break,
        }
    }
    // `.filter((x) => x != null)` drops parts without a rank.
    bounds.windows(2).filter(|w| ranks.contains_key(&piece[w[0]..w[1]])).count()
}

pub fn count_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let ranks = ranks();
    pieces(text).into_iter().map(|p| {
        let b = p.as_bytes();
        if ranks.contains_key(b) { 1 } else { byte_pair_count(b, ranks) }
    }).sum()
}
