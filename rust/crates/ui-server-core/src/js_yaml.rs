//! Port of js-yaml 3.14.2 `lib/js-yaml/loader.js` (`safeLoad`, DEFAULT_SAFE_SCHEMA). It works on UTF-16 code units
//! like the original, so positions, indentation and leniency (for example single-line `? k: v` forms) match.
//! Error messages carry js-yaml's reason; the "at line X, column Y" snippet is not reproduced.
//! Known difference: an alias that points into the collection still being built gives the partial value. In TS it
//! gives a circular object, which JSON serialization later rejects.
use crate::frontmatter::{base64_decode, construct_float, construct_int, js_keys, key_string, resolve_float, resolve_int, timestamp, Y};
use std::collections::{HashMap, HashSet};

const NAN: u32 = u32::MAX;
const FLOW_IN: u8 = 1;
const FLOW_OUT: u8 = 2;
const BLOCK_IN: u8 = 3;
const BLOCK_OUT: u8 = 4;
const Y_TAG: &str = "tag:yaml.org,2002:";

fn is_eol(c: u32) -> bool {
    c == 0x0A || c == 0x0D
}
fn is_ws(c: u32) -> bool {
    c == 0x09 || c == 0x20
}
fn is_ws_or_eol(c: u32) -> bool {
    is_ws(c) || is_eol(c)
}
fn is_flow(c: u32) -> bool {
    matches!(c, 0x2C | 0x5B | 0x5D | 0x7B | 0x7D)
}
fn from_hex(c: u32) -> i64 {
    if (0x30..=0x39).contains(&c) {
        return (c - 0x30) as i64;
    }
    let lc = c | 0x20;
    if (0x61..=0x66).contains(&lc) {
        return (lc - 0x61 + 10) as i64;
    }
    -1
}
fn simple_escape(c: u32) -> Option<&'static str> {
    Some(match c {
        0x30 => "\x00",
        0x61 => "\x07",
        0x62 => "\x08",
        0x74 | 0x09 => "\t",
        0x6E => "\n",
        0x76 => "\x0B",
        0x66 => "\x0C",
        0x72 => "\r",
        0x65 => "\x1B",
        0x20 => " ",
        0x22 => "\"",
        0x2F => "/",
        0x5C => "\\",
        0x4E => "\u{85}",
        0x5F => "\u{A0}",
        0x4C => "\u{2028}",
        0x50 => "\u{2029}",
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Scalar,
    Sequence,
    Mapping,
}

struct State {
    input: Vec<u16>,
    length: usize,
    position: usize,
    line: usize,
    line_start: usize,
    line_indent: i64,
    documents: Vec<Option<Y>>,
    tag_map: HashMap<String, String>,
    anchor_map: HashMap<String, Y>,
    version: Option<String>,
    tag: Option<String>,
    anchor: Option<String>,
    kind: Option<Kind>,
    /// UTF-16 string being built (scalars) or the collection value.
    result: Option<Y>,
    sres: Vec<u16>,
}

type R<T> = Result<T, String>;

impl State {
    fn ch(&self, i: usize) -> u32 {
        self.input.get(i).map_or(NAN, |&c| c as u32)
    }
    fn cur(&self) -> u32 {
        self.ch(self.position)
    }
    fn slice(&self, a: usize, b: usize) -> String {
        String::from_utf16_lossy(&self.input[a.min(self.input.len())..b.min(self.input.len())])
    }

    fn capture_segment(&mut self, start: usize, end: usize, check_json: bool) -> R<()> {
        if start < end {
            let seg = &self.input[start..end];
            if check_json {
                if seg.iter().any(|&c| !(c == 0x09 || c >= 0x20)) {
                    return Err("expected valid JSON character".into());
                }
            } else if non_printable(seg) {
                return Err("the stream contains non-printable characters".into());
            }
            self.sres.extend_from_slice(seg);
        }
        Ok(())
    }

    fn read_line_break(&mut self) -> R<()> {
        match self.cur() {
            0x0A => self.position += 1,
            0x0D => {
                self.position += 1;
                if self.cur() == 0x0A {
                    self.position += 1;
                }
            }
            _ => return Err("a line break is expected".into()),
        }
        self.line += 1;
        self.line_start = self.position;
        Ok(())
    }

    fn skip_separation_space(&mut self, allow_comments: bool, _check_indent: i64) -> R<usize> {
        let mut line_breaks = 0;
        let mut ch = self.cur();
        while ch != 0 {
            while is_ws(ch) {
                self.position += 1;
                ch = self.cur();
            }
            if allow_comments && ch == 0x23 {
                loop {
                    self.position += 1;
                    ch = self.cur();
                    if ch == 0x0A || ch == 0x0D || ch == 0 {
                        break;
                    }
                }
            }
            if is_eol(ch) {
                self.read_line_break()?;
                ch = self.cur();
                line_breaks += 1;
                self.line_indent = 0;
                while ch == 0x20 {
                    self.line_indent += 1;
                    self.position += 1;
                    ch = self.cur();
                }
            } else {
                break;
            }
        }
        Ok(line_breaks)
    }

    fn test_document_separator(&self) -> bool {
        let p = self.position;
        let ch = self.ch(p);
        if (ch == 0x2D || ch == 0x2E) && ch == self.ch(p + 1) && ch == self.ch(p + 2) {
            let c = self.ch(p + 3);
            return c == 0 || is_ws_or_eol(c);
        }
        false
    }

    fn write_folded_lines(&mut self, count: usize) {
        if count == 1 {
            self.sres.push(0x20);
        } else if count > 1 {
            self.sres.extend(std::iter::repeat_n(0x0A, count - 1));
        }
    }

    fn read_plain_scalar(&mut self, node_indent: i64, within_flow: bool) -> R<bool> {
        let (saved_kind, saved_sres) = (self.kind, self.sres.clone());
        let mut ch = self.cur();
        if is_ws_or_eol(ch) || is_flow(ch) || matches!(ch, 0x23 | 0x26 | 0x2A | 0x21 | 0x7C | 0x3E | 0x27 | 0x22 | 0x25 | 0x40 | 0x60) {
            return Ok(false);
        }
        if ch == 0x3F || ch == 0x2D {
            let f = self.ch(self.position + 1);
            if is_ws_or_eol(f) || within_flow && is_flow(f) {
                return Ok(false);
            }
        }
        self.kind = Some(Kind::Scalar);
        self.sres.clear();
        let (mut cs, mut ce) = (self.position, self.position);
        let mut pending = false;
        let mut l_line = 0;
        while ch != 0 {
            if ch == 0x3A {
                let f = self.ch(self.position + 1);
                if is_ws_or_eol(f) || within_flow && is_flow(f) {
                    break;
                }
            } else if ch == 0x23 {
                let p = if self.position == 0 { NAN } else { self.ch(self.position - 1) };
                if is_ws_or_eol(p) {
                    break;
                }
            } else if (self.position == self.line_start && self.test_document_separator()) || within_flow && is_flow(ch) {
                break;
            } else if is_eol(ch) {
                l_line = self.line;
                let (l_start, l_indent) = (self.line_start, self.line_indent);
                self.skip_separation_space(false, -1)?;
                if self.line_indent >= node_indent {
                    pending = true;
                    ch = self.cur();
                    continue;
                } else {
                    self.position = ce;
                    self.line = l_line;
                    self.line_start = l_start;
                    self.line_indent = l_indent;
                    break;
                }
            }
            if pending {
                self.capture_segment(cs, ce, false)?;
                self.write_folded_lines(self.line - l_line);
                cs = self.position;
                ce = self.position;
                pending = false;
            }
            if !is_ws(ch) {
                ce = self.position + 1;
            }
            self.position += 1;
            ch = self.cur();
        }
        self.capture_segment(cs, ce, false)?;
        if !self.sres.is_empty() {
            return Ok(true);
        }
        self.kind = saved_kind;
        self.sres = saved_sres;
        Ok(false)
    }

    fn read_single_quoted(&mut self, node_indent: i64) -> R<bool> {
        if self.cur() != 0x27 {
            return Ok(false);
        }
        self.kind = Some(Kind::Scalar);
        self.sres.clear();
        self.position += 1;
        let (mut cs, mut ce) = (self.position, self.position);
        loop {
            let ch = self.cur();
            if ch == 0 {
                break;
            }
            if ch == 0x27 {
                self.capture_segment(cs, self.position, true)?;
                self.position += 1;
                if self.cur() == 0x27 {
                    cs = self.position;
                    self.position += 1;
                    ce = self.position;
                } else {
                    return Ok(true);
                }
            } else if is_eol(ch) {
                self.capture_segment(cs, ce, true)?;
                let n = self.skip_separation_space(false, node_indent)?;
                self.write_folded_lines(n);
                cs = self.position;
                ce = self.position;
            } else if self.position == self.line_start && self.test_document_separator() {
                return Err("unexpected end of the document within a single quoted scalar".into());
            } else {
                self.position += 1;
                ce = self.position;
            }
        }
        Err("unexpected end of the stream within a single quoted scalar".into())
    }

    fn read_double_quoted(&mut self, node_indent: i64) -> R<bool> {
        if self.cur() != 0x22 {
            return Ok(false);
        }
        self.kind = Some(Kind::Scalar);
        self.sres.clear();
        self.position += 1;
        let (mut cs, mut ce) = (self.position, self.position);
        loop {
            let mut ch = self.cur();
            if ch == 0 {
                break;
            }
            if ch == 0x22 {
                self.capture_segment(cs, self.position, true)?;
                self.position += 1;
                return Ok(true);
            } else if ch == 0x5C {
                self.capture_segment(cs, self.position, true)?;
                self.position += 1;
                ch = self.cur();
                if is_eol(ch) {
                    self.skip_separation_space(false, node_indent)?;
                } else if let Some(e) = simple_escape(ch) {
                    self.sres.extend(e.encode_utf16());
                    self.position += 1;
                } else if let Some(mut len) = match ch { 0x78 => Some(2), 0x75 => Some(4), 0x55 => Some(8), _ => None } {
                    let mut v: u64 = 0;
                    while len > 0 {
                        self.position += 1;
                        let t = from_hex(self.cur());
                        if t < 0 {
                            return Err("expected hexadecimal character".into());
                        }
                        // JS `(hexResult << 4) + tmp` is 32-bit.
                        v = (((v as u32) << 4) as u64).wrapping_add(t as u64);
                        len -= 1;
                    }
                    push_codepoint(&mut self.sres, v as i64 as i32 as i64);
                    self.position += 1;
                } else {
                    return Err("unknown escape sequence".into());
                }
                cs = self.position;
                ce = self.position;
            } else if is_eol(ch) {
                self.capture_segment(cs, ce, true)?;
                let n = self.skip_separation_space(false, node_indent)?;
                self.write_folded_lines(n);
                cs = self.position;
                ce = self.position;
            } else if self.position == self.line_start && self.test_document_separator() {
                return Err("unexpected end of the document within a double quoted scalar".into());
            } else {
                self.position += 1;
                ce = self.position;
            }
        }
        Err("unexpected end of the stream within a double quoted scalar".into())
    }

    fn read_flow_collection(&mut self, node_indent: i64) -> R<bool> {
        let (tag, anchor) = (self.tag.clone(), self.anchor.clone());
        let ch0 = self.cur();
        let (terminator, is_mapping) = match ch0 {
            0x5B => (0x5D, false),
            0x7B => (0x7D, true),
            _ => return Ok(false),
        };
        let mut seq: Vec<Y> = vec![];
        let mut map: Vec<(String, Y)> = vec![];
        let mut over: HashSet<String> = HashSet::new();
        if let Some(a) = &self.anchor {
            self.anchor_map.insert(a.clone(), if is_mapping { Y::Obj(vec![]) } else { Y::Arr(vec![]) });
        }
        let mut read_next = true;
        self.position += 1;
        let mut ch = self.cur();
        while ch != 0 {
            self.skip_separation_space(true, node_indent)?;
            ch = self.cur();
            if ch == terminator {
                self.position += 1;
                self.tag = tag;
                self.anchor = anchor;
                self.kind = Some(if is_mapping { Kind::Mapping } else { Kind::Sequence });
                self.result = Some(if is_mapping { Y::Obj(map) } else { Y::Arr(seq) });
                return Ok(true);
            } else if !read_next {
                return Err("missed comma between flow collection entries".into());
            }
            let mut value: Option<Y> = None;
            let (mut is_pair, mut explicit) = (false, false);
            if ch == 0x3F && is_ws_or_eol(self.ch(self.position + 1)) {
                is_pair = true;
                explicit = true;
                self.position += 1;
                self.skip_separation_space(true, node_indent)?;
            }
            let line = self.line;
            self.compose_node(node_indent, FLOW_IN, false, true)?;
            let key_tag = self.tag.clone();
            let key = self.result.take();
            self.skip_separation_space(true, node_indent)?;
            ch = self.cur();
            if (explicit || self.line == line) && ch == 0x3A {
                is_pair = true;
                self.position += 1;
                self.skip_separation_space(true, node_indent)?;
                self.compose_node(node_indent, FLOW_IN, false, true)?;
                value = self.result.take();
            }
            if is_mapping {
                store_mapping_pair(&mut map, &mut over, key_tag.as_deref(), key, value)?;
            } else if is_pair {
                let mut m = vec![];
                let mut o = HashSet::new();
                store_mapping_pair(&mut m, &mut o, key_tag.as_deref(), key, value)?;
                seq.push(Y::Obj(m));
            } else {
                seq.push(key.unwrap_or(Y::Null));
            }
            self.skip_separation_space(true, node_indent)?;
            ch = self.cur();
            if ch == 0x2C {
                read_next = true;
                self.position += 1;
                ch = self.cur();
            } else {
                read_next = false;
            }
        }
        Err("unexpected end of the stream within a flow collection".into())
    }

    fn read_block_scalar(&mut self, node_indent: i64) -> R<bool> {
        let mut ch = self.cur();
        let folding = match ch {
            0x7C => false,
            0x3E => true,
            _ => return Ok(false),
        };
        // chomping: 1 clip, 2 strip, 3 keep
        let mut chomping = 1;
        let (mut did_read, mut detected) = (false, false);
        let mut text_indent = node_indent;
        let mut empty_lines = 0usize;
        let mut at_more = false;
        self.kind = Some(Kind::Scalar);
        self.sres.clear();
        while ch != 0 {
            self.position += 1;
            ch = self.cur();
            if ch == 0x2B || ch == 0x2D {
                if chomping == 1 {
                    chomping = if ch == 0x2B { 3 } else { 2 };
                } else {
                    return Err("repeat of a chomping mode identifier".into());
                }
            } else if (0x30..=0x39).contains(&ch) {
                let t = (ch - 0x30) as i64;
                if t == 0 {
                    return Err("bad explicit indentation width of a block scalar; it cannot be less than one".into());
                } else if !detected {
                    text_indent = node_indent + t - 1;
                    detected = true;
                } else {
                    return Err("repeat of an indentation width identifier".into());
                }
            } else {
                break;
            }
        }
        if is_ws(ch) {
            loop {
                self.position += 1;
                ch = self.cur();
                if !is_ws(ch) {
                    break;
                }
            }
            if ch == 0x23 {
                loop {
                    self.position += 1;
                    ch = self.cur();
                    if is_eol(ch) || ch == 0 {
                        break;
                    }
                }
            }
        }
        let nl = |n: usize| std::iter::repeat_n(0x0Au16, n);
        while ch != 0 {
            self.read_line_break()?;
            self.line_indent = 0;
            ch = self.cur();
            while (!detected || self.line_indent < text_indent) && ch == 0x20 {
                self.line_indent += 1;
                self.position += 1;
                ch = self.cur();
            }
            if !detected && self.line_indent > text_indent {
                text_indent = self.line_indent;
            }
            if is_eol(ch) {
                empty_lines += 1;
                continue;
            }
            if self.line_indent < text_indent {
                if chomping == 3 {
                    self.sres.extend(nl(if did_read { 1 + empty_lines } else { empty_lines }));
                } else if chomping == 1 && did_read {
                    self.sres.push(0x0A);
                }
                break;
            }
            if folding {
                if is_ws(ch) {
                    at_more = true;
                    self.sres.extend(nl(if did_read { 1 + empty_lines } else { empty_lines }));
                } else if at_more {
                    at_more = false;
                    self.sres.extend(nl(empty_lines + 1));
                } else if empty_lines == 0 {
                    if did_read {
                        self.sres.push(0x20);
                    }
                } else {
                    self.sres.extend(nl(empty_lines));
                }
            } else {
                self.sres.extend(nl(if did_read { 1 + empty_lines } else { empty_lines }));
            }
            did_read = true;
            detected = true;
            empty_lines = 0;
            let cs = self.position;
            while !is_eol(ch) && ch != 0 {
                self.position += 1;
                ch = self.cur();
            }
            self.capture_segment(cs, self.position, false)?;
        }
        Ok(true)
    }

    fn read_block_sequence(&mut self, node_indent: i64) -> R<bool> {
        let (tag, anchor) = (self.tag.clone(), self.anchor.clone());
        let mut out: Vec<Y> = vec![];
        if let Some(a) = &self.anchor {
            self.anchor_map.insert(a.clone(), Y::Arr(vec![]));
        }
        let mut detected = false;
        let mut ch = self.cur();
        while ch != 0 {
            if ch != 0x2D || !is_ws_or_eol(self.ch(self.position + 1)) {
                break;
            }
            detected = true;
            self.position += 1;
            if self.skip_separation_space(true, -1)? > 0 && self.line_indent <= node_indent {
                out.push(Y::Null);
                ch = self.cur();
                continue;
            }
            let line = self.line;
            self.compose_node(node_indent, BLOCK_IN, false, true)?;
            out.push(self.result.take().unwrap_or(Y::Null));
            self.skip_separation_space(true, -1)?;
            ch = self.cur();
            if (self.line == line || self.line_indent > node_indent) && ch != 0 {
                return Err("bad indentation of a sequence entry".into());
            } else if self.line_indent < node_indent {
                break;
            }
        }
        if detected {
            self.tag = tag;
            self.anchor = anchor;
            self.kind = Some(Kind::Sequence);
            self.result = Some(Y::Arr(out));
            return Ok(true);
        }
        Ok(false)
    }

    fn read_block_mapping(&mut self, node_indent: i64, flow_indent: i64) -> R<bool> {
        let (tag, anchor) = (self.tag.clone(), self.anchor.clone());
        let mut res: Vec<(String, Y)> = vec![];
        let mut over: HashSet<String> = HashSet::new();
        let (mut key_tag, mut key, mut value): (Option<String>, Option<Y>, Option<Y>) = (None, None, None);
        let (mut at_explicit, mut detected) = (false, false);
        let mut allow_compact;
        if let Some(a) = &self.anchor {
            self.anchor_map.insert(a.clone(), Y::Obj(vec![]));
        }
        let mut ch = self.cur();
        while ch != 0 {
            let following = self.ch(self.position + 1);
            let line = self.line;
            if (ch == 0x3F || ch == 0x3A) && is_ws_or_eol(following) {
                if ch == 0x3F {
                    if at_explicit {
                        store_mapping_pair(&mut res, &mut over, key_tag.as_deref(), key.take(), None)?;
                        key_tag = None;
                        value = None;
                    }
                    detected = true;
                    at_explicit = true;
                    allow_compact = true;
                } else if at_explicit {
                    at_explicit = false;
                    allow_compact = true;
                } else {
                    return Err("incomplete explicit mapping pair; a key node is missed; or followed by a non-tabulated empty line".into());
                }
                self.position += 1;
                ch = following;
            } else if self.compose_node(flow_indent, FLOW_OUT, false, true)? {
                if self.line == line {
                    ch = self.cur();
                    while is_ws(ch) {
                        self.position += 1;
                        ch = self.cur();
                    }
                    if ch == 0x3A {
                        self.position += 1;
                        ch = self.cur();
                        if !is_ws_or_eol(ch) {
                            return Err("a whitespace character is expected after the key-value separator within a block mapping".into());
                        }
                        if at_explicit {
                            store_mapping_pair(&mut res, &mut over, key_tag.as_deref(), key.take(), None)?;
                            value = None;
                        }
                        detected = true;
                        at_explicit = false;
                        allow_compact = false;
                        key_tag = self.tag.clone();
                        key = self.result.take();
                    } else if detected {
                        return Err("can not read an implicit mapping pair; a colon is missed".into());
                    } else {
                        self.tag = tag;
                        self.anchor = anchor;
                        return Ok(true);
                    }
                } else if detected {
                    return Err("can not read a block mapping entry; a multiline key may not be an implicit key".into());
                } else {
                    self.tag = tag;
                    self.anchor = anchor;
                    return Ok(true);
                }
            } else {
                break;
            }
            if self.line == line || self.line_indent > node_indent {
                if self.compose_node(node_indent, BLOCK_OUT, true, allow_compact)? {
                    if at_explicit {
                        key = self.result.take();
                    } else {
                        value = self.result.take();
                    }
                }
                if !at_explicit {
                    store_mapping_pair(&mut res, &mut over, key_tag.as_deref(), key.take(), value.take())?;
                    key_tag = None;
                }
                self.skip_separation_space(true, -1)?;
                ch = self.cur();
            }
            if self.line_indent > node_indent && ch != 0 {
                return Err("bad indentation of a mapping entry".into());
            } else if self.line_indent < node_indent {
                break;
            }
        }
        if at_explicit {
            store_mapping_pair(&mut res, &mut over, key_tag.as_deref(), key.take(), None)?;
        }
        if detected {
            self.tag = tag;
            self.anchor = anchor;
            self.kind = Some(Kind::Mapping);
            self.result = Some(Y::Obj(res));
        }
        Ok(detected)
    }

    fn read_tag_property(&mut self) -> R<bool> {
        if self.cur() != 0x21 {
            return Ok(false);
        }
        if self.tag.is_some() {
            return Err("duplication of a tag property".into());
        }
        self.position += 1;
        let mut ch = self.cur();
        let (mut verbatim, mut named) = (false, false);
        let mut handle = String::from("!");
        if ch == 0x3C {
            verbatim = true;
            self.position += 1;
        } else if ch == 0x21 {
            named = true;
            handle = "!!".into();
            self.position += 1;
        }
        ch = self.cur();
        let mut start = self.position;
        let name;
        if verbatim {
            loop {
                self.position += 1;
                ch = self.cur();
                if ch == 0 || ch == 0x3E {
                    break;
                }
            }
            if self.position < self.length {
                name = self.slice(start, self.position);
                self.position += 1;
            } else {
                return Err("unexpected end of the stream within a verbatim tag".into());
            }
        } else {
            while ch != 0 && !is_ws_or_eol(ch) {
                if ch == 0x21 {
                    if !named {
                        handle = self.slice(start - 1, self.position + 1);
                        if !tag_handle_ok(&handle) {
                            return Err("named tag handle cannot contain such characters".into());
                        }
                        named = true;
                        start = self.position + 1;
                    } else {
                        return Err("tag suffix cannot contain exclamation marks".into());
                    }
                }
                self.position += 1;
                ch = self.cur();
            }
            name = self.slice(start, self.position);
            if name.contains([',', '[', ']', '{', '}']) {
                return Err("tag suffix cannot contain flow indicator characters".into());
            }
        }
        if !name.is_empty() && !tag_uri_ok(&name) {
            return Err(format!("tag name cannot contain such characters: {name}"));
        }
        self.tag = Some(if verbatim {
            name
        } else if let Some(p) = self.tag_map.get(&handle) {
            format!("{p}{name}")
        } else if handle == "!" {
            format!("!{name}")
        } else if handle == "!!" {
            format!("{Y_TAG}{name}")
        } else {
            return Err(format!("undeclared tag handle \"{handle}\""));
        });
        Ok(true)
    }

    fn read_anchor_property(&mut self) -> R<bool> {
        if self.cur() != 0x26 {
            return Ok(false);
        }
        if self.anchor.is_some() {
            return Err("duplication of an anchor property".into());
        }
        self.position += 1;
        let start = self.position;
        let mut ch = self.cur();
        while ch != 0 && !is_ws_or_eol(ch) && !is_flow(ch) {
            self.position += 1;
            ch = self.cur();
        }
        if self.position == start {
            return Err("name of an anchor node must contain at least one character".into());
        }
        self.anchor = Some(self.slice(start, self.position));
        Ok(true)
    }

    fn read_alias(&mut self) -> R<bool> {
        if self.cur() != 0x2A {
            return Ok(false);
        }
        self.position += 1;
        let start = self.position;
        let mut ch = self.cur();
        while ch != 0 && !is_ws_or_eol(ch) && !is_flow(ch) {
            self.position += 1;
            ch = self.cur();
        }
        if self.position == start {
            return Err("name of an alias node must contain at least one character".into());
        }
        let alias = self.slice(start, self.position);
        let Some(v) = self.anchor_map.get(&alias) else { return Err(format!("unidentified alias \"{alias}\"")) };
        self.result = Some(v.clone());
        self.skip_separation_space(true, -1)?;
        Ok(true)
    }

    /// Takes the scalar text built so far as the node result.
    fn scalar_result(&mut self) {
        self.result = Some(Y::Str(String::from_utf16_lossy(&self.sres)));
    }

    fn compose_node(&mut self, parent_indent: i64, ctx: u8, allow_to_seek: bool, allow_compact: bool) -> R<bool> {
        self.tag = None;
        self.anchor = None;
        self.kind = None;
        self.result = None;
        let block = ctx == BLOCK_OUT || ctx == BLOCK_IN;
        let (allow_block_styles, allow_block_scalars) = (block, block);
        let mut allow_block_collections = block;
        let mut indent_status = 1i32;
        let mut at_new_line = false;
        let mut has_content = false;
        let status = |li: i64| if li > parent_indent { 1 } else if li == parent_indent { 0 } else { -1 };
        if allow_to_seek && self.skip_separation_space(true, -1)? > 0 {
            at_new_line = true;
            indent_status = status(self.line_indent);
        }
        if indent_status == 1 {
            while self.read_tag_property()? || self.read_anchor_property()? {
                if self.skip_separation_space(true, -1)? > 0 {
                    at_new_line = true;
                    allow_block_collections = allow_block_styles;
                    indent_status = status(self.line_indent);
                } else {
                    allow_block_collections = false;
                }
            }
        }
        if allow_block_collections {
            allow_block_collections = at_new_line || allow_compact;
        }
        if indent_status == 1 || ctx == BLOCK_OUT {
            let flow_indent = if ctx == FLOW_IN || ctx == FLOW_OUT { parent_indent } else { parent_indent + 1 };
            let block_indent = (self.position - self.line_start) as i64;
            if indent_status == 1 {
                if allow_block_collections && (self.read_block_sequence(block_indent)? || self.read_block_mapping(block_indent, flow_indent)?) || self.read_flow_collection(flow_indent)? {
                    has_content = true;
                } else {
                    if (allow_block_scalars && self.read_block_scalar(flow_indent)?) || self.read_single_quoted(flow_indent)? || self.read_double_quoted(flow_indent)? {
                        has_content = true;
                        self.scalar_result();
                    } else if self.read_alias()? {
                        has_content = true;
                        if self.tag.is_some() || self.anchor.is_some() {
                            return Err("alias node should not have any properties".into());
                        }
                    } else if self.read_plain_scalar(flow_indent, ctx == FLOW_IN)? {
                        has_content = true;
                        self.scalar_result();
                        if self.tag.is_none() {
                            self.tag = Some("?".into());
                        }
                    }
                    if let Some(a) = &self.anchor {
                        self.anchor_map.insert(a.clone(), self.result.clone().unwrap_or(Y::Null));
                    }
                }
            } else if indent_status == 0 {
                has_content = allow_block_collections && self.read_block_sequence(block_indent)?;
            }
        }
        if let Some(tag) = self.tag.clone().filter(|t| t != "!") {
            if tag == "?" {
                if self.result.is_some() && self.kind != Some(Kind::Scalar) {
                    return Err(format!("unacceptable node kind for !<?> tag; it should be \"scalar\", not \"{}\"", match self.kind { Some(Kind::Sequence) => "sequence", Some(Kind::Mapping) => "mapping", _ => "undefined" }));
                }
                if let Some(Y::Str(s)) = &self.result {
                    let s = s.clone();
                    let (v, t) = resolve_implicit(&s);
                    self.result = Some(v);
                    self.tag = Some(t.into());
                }
            } else {
                let v = construct_explicit(&tag, self.kind, self.result.take())?;
                self.result = Some(v);
            }
        }
        // Anchors always see the final (constructed) value; js-yaml shares the object, which is the same once
        // construction finishes.
        if let Some(a) = &self.anchor {
            if let Some(r) = &self.result {
                self.anchor_map.insert(a.clone(), r.clone());
            }
        }
        Ok(self.tag.is_some() || self.anchor.is_some() || has_content)
    }

    fn read_document(&mut self) -> R<()> {
        self.version = None;
        self.tag_map.clear();
        self.anchor_map.clear();
        let mut has_directives = false;
        while self.cur() != 0 {
            self.skip_separation_space(true, -1)?;
            let mut ch = self.cur();
            if self.line_indent > 0 || ch != 0x25 {
                break;
            }
            has_directives = true;
            self.position += 1;
            ch = self.cur();
            let start = self.position;
            while ch != 0 && !is_ws_or_eol(ch) {
                self.position += 1;
                ch = self.cur();
            }
            let name = self.slice(start, self.position);
            let mut args: Vec<String> = vec![];
            if name.is_empty() {
                return Err("directive name must not be less than one character in length".into());
            }
            while ch != 0 {
                while is_ws(ch) {
                    self.position += 1;
                    ch = self.cur();
                }
                if ch == 0x23 {
                    loop {
                        self.position += 1;
                        ch = self.cur();
                        if ch == 0 || is_eol(ch) {
                            break;
                        }
                    }
                    break;
                }
                if is_eol(ch) {
                    break;
                }
                let s = self.position;
                while ch != 0 && !is_ws_or_eol(ch) {
                    self.position += 1;
                    ch = self.cur();
                }
                args.push(self.slice(s, self.position));
            }
            if ch != 0 {
                self.read_line_break()?;
            }
            match name.as_str() {
                "YAML" => {
                    if self.version.is_some() {
                        return Err("duplication of %YAML directive".into());
                    }
                    if args.len() != 1 {
                        return Err("YAML directive accepts exactly one argument".into());
                    }
                    let a = &args[0];
                    let parts: Vec<&str> = a.split('.').collect();
                    if parts.len() != 2 || parts.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit())) {
                        return Err("ill-formed argument of the YAML directive".into());
                    }
                    if parts[0].parse::<f64>().unwrap() != 1.0 {
                        return Err("unacceptable YAML version of the document".into());
                    }
                    self.version = Some(a.clone());
                }
                "TAG" => {
                    if args.len() != 2 {
                        return Err("TAG directive accepts exactly two arguments".into());
                    }
                    if !tag_handle_ok(&args[0]) {
                        return Err("ill-formed tag handle (first argument) of the TAG directive".into());
                    }
                    if self.tag_map.contains_key(&args[0]) {
                        return Err(format!("there is a previously declared suffix for \"{}\" tag handle", args[0]));
                    }
                    if !tag_uri_ok(&args[1]) {
                        return Err("ill-formed tag prefix (second argument) of the TAG directive".into());
                    }
                    self.tag_map.insert(args[0].clone(), args[1].clone());
                }
                _ => {}
            }
        }
        self.skip_separation_space(true, -1)?;
        if self.line_indent == 0 && self.cur() == 0x2D && self.ch(self.position + 1) == 0x2D && self.ch(self.position + 2) == 0x2D {
            self.position += 3;
            self.skip_separation_space(true, -1)?;
        } else if has_directives {
            return Err("directives end mark is expected".into());
        }
        self.compose_node(self.line_indent - 1, BLOCK_OUT, false, true)?;
        self.skip_separation_space(true, -1)?;
        self.documents.push(self.result.take());
        if self.position == self.line_start && self.test_document_separator() {
            if self.cur() == 0x2E {
                self.position += 3;
                self.skip_separation_space(true, -1)?;
            }
            return Ok(());
        }
        if self.position + 1 < self.length {
            return Err("end of the stream or a document separator is expected".into());
        }
        Ok(())
    }
}

fn push_codepoint(out: &mut Vec<u16>, c: i64) {
    // String.fromCharCode semantics (ToUint16) on the computed values.
    if c <= 0xFFFF {
        out.push(c as u16);
    } else {
        out.push((((c - 0x10000) >> 10) + 0xD800) as u16);
        out.push((((c - 0x10000) & 0x3FF) + 0xDC00) as u16);
    }
}

/// PATTERN_NON_PRINTABLE over UTF-16.
fn non_printable(s: &[u16]) -> bool {
    for (i, &c) in s.iter().enumerate() {
        if matches!(c, 0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F..=0x84 | 0x86..=0x9F | 0xFFFE | 0xFFFF) {
            return true;
        }
        if (0xD800..=0xDBFF).contains(&c) && !s.get(i + 1).is_some_and(|n| (0xDC00..=0xDFFF).contains(n)) {
            return true;
        }
        if (0xDC00..=0xDFFF).contains(&c) && (i == 0 || !(0xD800..=0xDBFF).contains(&s[i - 1])) {
            return true;
        }
    }
    false
}

/// /^(?:!|!!|![a-z\-]+!)$/i
fn tag_handle_ok(h: &str) -> bool {
    h == "!" || h == "!!" || (h.len() > 2 && h.starts_with('!') && h.ends_with('!') && h[1..h.len() - 1].chars().all(|c| c.is_ascii_alphabetic() || c == '-'))
}

/// /^(?:!|[^,\[\]\{\}])(?:%[0-9a-f]{2}|[0-9a-z\-#;\/\?:@&=\+\$,_\.!~\*'\(\)\[\]])*$/i
fn tag_uri_ok(s: &str) -> bool {
    let c: Vec<char> = s.chars().collect();
    let Some(&first) = c.first() else { return false };
    if matches!(first, ',' | '[' | ']' | '{' | '}') {
        return false;
    }
    let mut i = 1;
    while i < c.len() {
        if c[i] == '%' && c.get(i + 1).is_some_and(|x| x.is_ascii_hexdigit()) && c[i + 1].is_ascii_hexdigit() && c.get(i + 2).is_some_and(|x| x.is_ascii_hexdigit()) {
            i += 3;
            continue;
        }
        if c[i].is_ascii_alphanumeric() || "-#;/?:@&=+$,_.!~*'()[]".contains(c[i]) {
            i += 1;
            continue;
        }
        return false;
    }
    true
}

/// Implicit types in DEFAULT_SAFE_SCHEMA order: null, bool, int, float, timestamp, merge. Returns the value and tag.
fn resolve_implicit(s: &str) -> (Y, &'static str) {
    match s {
        "~" | "null" | "Null" | "NULL" => return (Y::Null, "tag:yaml.org,2002:null"),
        "true" | "True" | "TRUE" => return (Y::Bool(true), "tag:yaml.org,2002:bool"),
        "false" | "False" | "FALSE" => return (Y::Bool(false), "tag:yaml.org,2002:bool"),
        _ => {}
    }
    if resolve_int(s) {
        return (Y::Num(construct_int(s)), "tag:yaml.org,2002:int");
    }
    if resolve_float(s) {
        return (Y::Num(construct_float(s)), "tag:yaml.org,2002:float");
    }
    if let Some(ms) = timestamp(s) {
        return (Y::Date(ms, s.to_string()), "tag:yaml.org,2002:timestamp");
    }
    if s == "<<" {
        return (Y::Str(s.into()), "tag:yaml.org,2002:merge");
    }
    (Y::Str(s.to_string()), "?")
}

fn construct_explicit(tag: &str, kind: Option<Kind>, data: Option<Y>) -> R<Y> {
    let unknown = || format!("unknown tag !<{tag}>");
    let bad = || format!("cannot resolve a node with !<{tag}> explicit tag");
    let Some(name) = tag.strip_prefix(Y_TAG) else { return Err(unknown()) };
    let scalar_types = ["str", "null", "bool", "int", "float", "timestamp", "merge", "binary"];
    let ok_kind = match kind {
        None => true,
        Some(Kind::Scalar) => scalar_types.contains(&name),
        Some(Kind::Sequence) => ["seq", "omap", "pairs"].contains(&name),
        Some(Kind::Mapping) => ["map", "set"].contains(&name),
    };
    if !ok_kind || !(scalar_types.contains(&name) || ["seq", "omap", "pairs", "map", "set"].contains(&name)) {
        return Err(unknown());
    }
    let s = match &data {
        Some(Y::Str(s)) => Some(s.as_str()),
        _ => None,
    };
    Ok(match name {
        "str" => data.unwrap_or(Y::Str(String::new())),
        "null" => match s {
            None if data.is_none() => Y::Null,
            Some("~" | "null" | "Null" | "NULL") => Y::Null,
            _ => return Err(bad()),
        },
        "bool" => match s {
            Some("true" | "True" | "TRUE") => Y::Bool(true),
            Some("false" | "False" | "FALSE") => Y::Bool(false),
            _ => return Err(bad()),
        },
        "int" => match s {
            Some(s) if resolve_int(s) => Y::Num(construct_int(s)),
            _ => return Err(bad()),
        },
        "float" => match s {
            Some(s) if resolve_float(s) => Y::Num(construct_float(s)),
            _ => return Err(bad()),
        },
        "timestamp" => match s.and_then(|s| timestamp(s).map(|t| (t, s))) {
            Some((t, s)) => Y::Date(t, s.to_string()),
            None => return Err(bad()),
        },
        "merge" => match (&data, s) {
            (None, _) => Y::Null,
            (_, Some("<<")) => data.unwrap(),
            _ => return Err(bad()),
        },
        "binary" => match s.and_then(base64_decode) {
            Some(b) => Y::Buffer(b),
            None => return Err(bad()),
        },
        "seq" => data.unwrap_or(Y::Arr(vec![])),
        "map" => data.unwrap_or(Y::Obj(vec![])),
        "set" => match data {
            None => Y::Obj(vec![]),
            Some(Y::Obj(m)) if m.iter().all(|(_, v)| matches!(v, Y::Null)) => Y::Obj(m),
            _ => return Err(bad()),
        },
        "omap" => match data {
            None => Y::Arr(vec![]),
            Some(Y::Arr(a)) => {
                let mut seen: Vec<String> = vec![];
                for p in &a {
                    let Y::Obj(m) = p else { return Err(bad()) };
                    if m.len() != 1 {
                        return Err(bad());
                    }
                    let k = js_keys(m).pop().unwrap();
                    if seen.contains(&k) {
                        return Err(bad());
                    }
                    seen.push(k);
                }
                Y::Arr(a)
            }
            _ => return Err(bad()),
        },
        "pairs" => match data {
            None => Y::Arr(vec![]),
            Some(Y::Arr(a)) => {
                let mut out = vec![];
                for p in a {
                    let Y::Obj(m) = p else { return Err(bad()) };
                    if m.len() != 1 {
                        return Err(bad());
                    }
                    let (k, v) = m.into_iter().next().unwrap();
                    out.push(Y::Arr(vec![Y::Str(k), v]));
                }
                Y::Arr(out)
            }
            _ => return Err(bad()),
        },
        _ => return Err(unknown()),
    })
}

/// `storeMappingPair` (keyTag merge, string keys, duplicate check with overridable keys).
fn store_mapping_pair(res: &mut Vec<(String, Y)>, over: &mut HashSet<String>, key_tag: Option<&str>, key: Option<Y>, value: Option<Y>) -> R<()> {
    let key = key.unwrap_or(Y::Null);
    let k = key_string(&key)?;
    if key_tag == Some("tag:yaml.org,2002:merge") {
        let sources = match value {
            Some(Y::Arr(a)) => a,
            Some(v) => vec![v],
            None => vec![Y::Null],
        };
        for src in sources {
            let entries: Vec<(String, Y)> = match src {
                Y::Obj(m) => {
                    let order = js_keys(&m);
                    order.into_iter().map(|k| { let v = m.iter().find(|x| x.0 == k).unwrap().1.clone(); (k, v) }).collect()
                }
                Y::Arr(a) => a.into_iter().enumerate().map(|(i, v)| (i.to_string(), v)).collect(),
                Y::Buffer(b) => b.into_iter().enumerate().map(|(i, v)| (i.to_string(), Y::Num(v as f64))).collect(),
                Y::Date(..) => vec![],
                _ => return Err("cannot merge mappings; the provided source object is unacceptable".into()),
            };
            for (k, v) in entries {
                if !res.iter().any(|(rk, _)| *rk == k) {
                    res.push((k.clone(), v));
                    over.insert(k);
                }
            }
        }
        return Ok(());
    }
    let value = value.unwrap_or(Y::Null);
    match res.iter_mut().find(|(rk, _)| *rk == k) {
        Some(slot) => {
            if !over.contains(&k) {
                return Err("duplicated mapping key".into());
            }
            slot.1 = value;
        }
        None => res.push((k.clone(), value)),
    }
    over.remove(&k);
    Ok(())
}

/// js-yaml 3 `safeLoad(input)`: Ok(None) is `undefined` (no document).
pub fn safe_load(input: &str) -> R<Option<Y>> {
    let mut u: Vec<u16> = input.encode_utf16().collect();
    if !u.is_empty() {
        let last = *u.last().unwrap();
        if last != 0x0A && last != 0x0D {
            u.push(0x0A);
        }
        if u[0] == 0xFEFF {
            u.remove(0);
        }
    }
    if u.contains(&0) {
        return Err("null byte is not allowed in input".into());
    }
    let length = u.len();
    u.push(0);
    let mut st = State {
        input: u,
        length,
        position: 0,
        line: 0,
        line_start: 0,
        line_indent: 0,
        documents: vec![],
        tag_map: HashMap::new(),
        anchor_map: HashMap::new(),
        version: None,
        tag: None,
        anchor: None,
        kind: None,
        result: None,
        sres: vec![],
    };
    while st.cur() == 0x20 {
        st.line_indent += 1;
        st.position += 1;
    }
    while st.length > 0 && st.position < st.length - 1 {
        st.read_document()?;
    }
    match st.documents.len() {
        0 => Ok(None),
        1 => Ok(Some(st.documents.pop().unwrap().unwrap_or(Y::Null))),
        _ => Err("expected a single document in the stream, but found more".into()),
    }
}
