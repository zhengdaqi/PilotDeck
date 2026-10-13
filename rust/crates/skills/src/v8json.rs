//! `JSON.parse` with V8 (Node 22) error messages, positions in UTF-16 code units.

use crate::jsjson::{JsObject, JsValue};

struct P<'a> {
    s: &'a [u16],
    i: usize,
}

const MAX_CTX: usize = 10;

impl P<'_> {
    fn peek(&self) -> Option<u16> {
        self.s.get(self.i).copied()
    }
    fn ws(&mut self) {
        while matches!(self.peek(), Some(0x20 | 0x09 | 0x0a | 0x0d)) {
            self.i += 1;
        }
    }
    fn line_col(&self, pos: usize) -> (usize, usize) {
        let mut line = 1;
        let mut col = 1;
        let upto = &self.s[..pos.min(self.s.len())];
        for (k, &c) in upto.iter().enumerate() {
            if c == 0x0d || (c == 0x0a && (k == 0 || upto[k - 1] != 0x0d)) {
                line += 1;
                col = 1;
            } else if c == 0x0a {
            } else {
                col += 1;
            }
        }
        (line, col)
    }
    fn at(&self, msg: &str, pos: usize) -> String {
        let (l, c) = self.line_col(pos);
        format!("{msg} in JSON at position {pos} (line {l} column {c})")
    }
    /// Unexpected token at `self.i` (or end of input).
    fn unexpected(&self) -> String {
        let Some(c) = self.peek() else { return "Unexpected end of JSON input".into() };
        if c == b'-' as u16 || (b'0' as u16..=b'9' as u16).contains(&c) {
            return self.at("Unexpected number", self.i);
        }
        if c == 0x22 {
            return self.at("Unexpected string", self.i);
        }
        let len = self.s.len();
        let pos = self.i;
        let tok = String::from_utf16_lossy(&[c]);
        let text = |a: usize, b: usize| String::from_utf16_lossy(&self.s[a..b]);
        if len <= MAX_CTX * 2 {
            format!("Unexpected token '{tok}', \"{}\" is not valid JSON", text(0, len))
        } else if pos < MAX_CTX {
            format!("Unexpected token '{tok}', \"{}\"... is not valid JSON", text(0, pos + MAX_CTX))
        } else if pos >= len - MAX_CTX {
            format!("Unexpected token '{tok}', ...\"{}\" is not valid JSON", text(pos - MAX_CTX, len))
        } else {
            format!("Unexpected token '{tok}', ...\"{}\"... is not valid JSON", text(pos - MAX_CTX, pos + MAX_CTX))
        }
    }
    fn expect_or(&self, msg: &str) -> String {
        self.at(msg, self.i)
    }
    fn value(&mut self) -> Result<JsValue, String> {
        self.ws();
        match self.peek() {
            Some(0x22) => self.string().map(JsValue::Str),
            Some(0x7b) => self.object(),
            Some(0x5b) => self.array(),
            Some(c) if c == b'-' as u16 || (b'0' as u16..=b'9' as u16).contains(&c) => self.number(),
            Some(0x74) => self.lit("true", JsValue::Bool(true)),
            Some(0x66) => self.lit("false", JsValue::Bool(false)),
            Some(0x6e) => self.lit("null", JsValue::Null),
            _ => Err(self.unexpected()),
        }
    }
    fn lit(&mut self, word: &str, v: JsValue) -> Result<JsValue, String> {
        for b in word.bytes() {
            if self.peek() != Some(b as u16) {
                return Err(self.unexpected());
            }
            self.i += 1;
        }
        Ok(v)
    }
    fn digit(&self) -> bool {
        matches!(self.peek(), Some(c) if (b'0' as u16..=b'9' as u16).contains(&c))
    }
    fn number(&mut self) -> Result<JsValue, String> {
        let start = self.i;
        if self.peek() == Some(b'-' as u16) {
            self.i += 1;
            if !self.digit() {
                return Err(self.expect_or("No number after minus sign"));
            }
        }
        if self.peek() == Some(b'0' as u16) {
            self.i += 1;
            if self.digit() {
                return Err(self.at("Unexpected number", self.i));
            }
        } else {
            while self.digit() {
                self.i += 1;
            }
        }
        if self.peek() == Some(b'.' as u16) {
            self.i += 1;
            if !self.digit() {
                return Err(self.expect_or("Unterminated fractional number"));
            }
            while self.digit() {
                self.i += 1;
            }
        }
        if matches!(self.peek(), Some(0x65 | 0x45)) {
            self.i += 1;
            if matches!(self.peek(), Some(0x2b | 0x2d)) {
                self.i += 1;
            }
            if !self.digit() {
                return Err(self.expect_or("Exponent part is missing a number"));
            }
            while self.digit() {
                self.i += 1;
            }
        }
        let text = String::from_utf16_lossy(&self.s[start..self.i]);
        Ok(JsValue::Num(text.parse::<f64>().unwrap_or(f64::NAN)))
    }
    fn hex(&self, off: usize) -> Option<u16> {
        let c = *self.s.get(self.i + off)?;
        char::from_u32(c as u32)?.to_digit(16).map(|d| d as u16)
    }
    fn string(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut out: Vec<u16> = Vec::new();
        loop {
            let Some(c) = self.peek() else { return Err(self.at("Unterminated string", self.i)) };
            match c {
                0x22 => {
                    self.i += 1;
                    return Ok(String::from_utf16_lossy(&out));
                }
                0x5c => {
                    self.i += 1;
                    let Some(e) = self.peek() else { return Err("Unexpected end of JSON input".into()) };
                    let mapped = match e {
                        0x22 => 0x22,
                        0x5c => 0x5c,
                        0x2f => 0x2f,
                        0x62 => 0x08,
                        0x66 => 0x0c,
                        0x6e => 0x0a,
                        0x72 => 0x0d,
                        0x74 => 0x09,
                        0x75 => {
                            let mut v: u16 = 0;
                            for k in 1..=4 {
                                match self.hex(k) {
                                    Some(d) => v = v * 16 + d,
                                    None => {
                                        let p = self.i + k;
                                        return Err(self.at("Bad Unicode escape", p.min(self.s.len())));
                                    }
                                }
                            }
                            self.i += 4;
                            v
                        }
                        e if e > 0x7f => return Err(self.unexpected()),
                        _ => return Err(self.at("Bad escaped character", self.i)),
                    };
                    out.push(mapped);
                    self.i += 1;
                }
                c if c < 0x20 => return Err(self.at("Bad control character in string literal", self.i)),
                c => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
    }
    fn object(&mut self) -> Result<JsValue, String> {
        self.i += 1;
        let mut o = JsObject::new();
        self.ws();
        if self.peek() == Some(0x7d) {
            self.i += 1;
            return Ok(JsValue::Obj(o));
        }
        if self.peek() != Some(0x22) {
            return Err(self.expect_or("Expected property name or '}'"));
        }
        loop {
            let k = self.string()?;
            self.ws();
            if self.peek() != Some(0x3a) {
                return Err(self.expect_or("Expected ':' after property name"));
            }
            self.i += 1;
            let v = self.value()?;
            o.set(k, v);
            self.ws();
            match self.peek() {
                Some(0x2c) => {
                    self.i += 1;
                    self.ws();
                    if self.peek() != Some(0x22) {
                        return Err(self.expect_or("Expected double-quoted property name"));
                    }
                }
                Some(0x7d) => {
                    self.i += 1;
                    return Ok(JsValue::Obj(o));
                }
                _ => return Err(self.expect_or("Expected ',' or '}' after property value")),
            }
        }
    }
    fn array(&mut self) -> Result<JsValue, String> {
        self.i += 1;
        let mut a = Vec::new();
        self.ws();
        if self.peek() == Some(0x5d) {
            self.i += 1;
            return Ok(JsValue::Arr(a));
        }
        loop {
            a.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(0x2c) => self.i += 1,
                Some(0x5d) => {
                    self.i += 1;
                    return Ok(JsValue::Arr(a));
                }
                _ => return Err(self.expect_or("Expected ',' or ']' after array element")),
            }
        }
    }
}

/// `JSON.parse(text)`; `Err` carries V8's `SyntaxError.message`.
pub fn parse(text: &str) -> Result<JsValue, String> {
    if matches!(text, "NaN" | "Infinity" | "undefined" | "[object Object]") {
        if let Err(e) = parse_inner(text) {
            if e.starts_with("Unexpected token") {
                return Err(format!("\"{text}\" is not valid JSON"));
            }
            return Err(e);
        }
    }
    parse_inner(text)
}

fn parse_inner(text: &str) -> Result<JsValue, String> {
    let s: Vec<u16> = text.encode_utf16().collect();
    let mut p = P { s: &s, i: 0 };
    let v = p.value()?;
    p.ws();
    if p.peek().is_some() {
        let (l, c) = p.line_col(p.i);
        return Err(format!("Unexpected non-whitespace character after JSON at position {} (line {l} column {c})", p.i));
    }
    Ok(v)
}
