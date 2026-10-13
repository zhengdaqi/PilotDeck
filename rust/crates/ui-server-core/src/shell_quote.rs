//! Port of `shell-quote@1` `parse(s)` with no env (third-party library used by ui/server commandParser).
//! The env is `{}`, so `$name` reads `Object.prototype` members. Functions stringify as V8 native-code source.
//! `__proto__` gives an object, which shell-quote wraps in a random TOKEN; here a fixed `<TOKEN>` marker is used.
//! This is the only intended difference, and it is never observable through validateCommand (the result always
//! contains `{`, which is dangerous or not allowlisted).
use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Str(String),
    Op(String),
    Glob(String),
    Comment(String),
}

const CONTROL: &str = r"(?:\|\||&&|;;|\|&|<\(|<<<|>>|>&|<&|[&;()|<>])";
// JS `\s` (WhiteSpace + LineTerminator); Rust's `\s` also matches U+0085 and misses U+FEFF.
const JS_WS: &str = r"\t\n\v\f\r \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}";

fn chunker() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        let meta = "|&;()<> \t"; // JS `'\\t'` inside a RegExp source is a tab
        let meta_cls = regex::escape(meta);
        let bareword = format!(r#"(?:\\['"{meta_cls}]|[^{JS_WS}'"{meta_cls}])+"#);
        let single = r#""(?:\\"|[^"])*?""#;
        let double = r#"'(?:\\'|[^'])*?'"#;
        Regex::new(&format!(r"({CONTROL})|((?:{bareword}|{single}|{double})+)")).unwrap()
    })
}

fn control_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(&format!("^{CONTROL}$")).unwrap())
}

/// `({})[key]` stringified the way `pre + r` would (undefined is handled by the caller).
fn object_prototype_member(key: &str) -> Option<String> {
    let f = |n: &str| format!("function {n}() {{ [native code] }}");
    Some(match key {
        "constructor" => f("Object"),
        "__defineGetter__" | "__defineSetter__" | "hasOwnProperty" | "__lookupGetter__" | "__lookupSetter__"
        | "isPrototypeOf" | "propertyIsEnumerable" | "toString" | "valueOf" | "toLocaleString" => f(key),
        "__proto__" => "<TOKEN>{}<TOKEN>".to_string(),
        _ => return None,
    })
}

fn get_var(key: &str) -> String {
    match object_prototype_member(key) {
        Some(v) => v,
        None if !key.is_empty() => String::new(),
        None => "$".into(),
    }
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

pub fn parse(string: &str) -> Result<Vec<Token>, String> {
    let mut out_tokens = vec![];
    let mut commented = false;
    for m in chunker().find_iter(string) {
        let s: Vec<char> = m.as_str().chars().collect();
        let st = m.as_str();
        if st.is_empty() || commented {
            continue;
        }
        if control_re().is_match(st) {
            out_tokens.push(Token::Op(st.into()));
            continue;
        }
        let mut quote: Option<char> = None;
        let mut esc = false;
        let mut out = String::new();
        let mut is_glob = false;
        let mut i: isize = 0;
        let at = |i: isize| -> Option<char> { if i >= 0 { s.get(i as usize).copied() } else { None } };
        let slice = |a: isize, b: isize| -> String {
            let a = a.clamp(0, s.len() as isize) as usize;
            let b = b.clamp(0, s.len() as isize) as usize;
            if a < b { s[a..b].iter().collect() } else { String::new() }
        };
        let env_var = |i: &mut isize| -> Result<String, String> {
            *i += 1;
            let c = at(*i);
            if c == Some('{') {
                *i += 1;
                if at(*i) == Some('}') {
                    return Err(format!("Bad substitution: {}", slice(*i - 2, *i + 1)));
                }
                let end = (*i as usize..s.len()).find(|&k| s[k] == '}');
                let Some(end) = end else { return Err(format!("Bad substitution: {}", slice(*i, s.len() as isize))) };
                let name = slice(*i, end as isize);
                *i = end as isize;
                Ok(get_var(&name))
            } else if c.is_some_and(|c| "*@#?$!_-".contains(c)) {
                let name = c.unwrap().to_string();
                *i += 1;
                Ok(get_var(&name))
            } else {
                let rest = slice(*i, s.len() as isize);
                match rest.chars().position(|c| !is_word(c)) {
                    None => {
                        *i = s.len() as isize;
                        Ok(get_var(&rest))
                    }
                    Some(k) => {
                        let name: String = rest.chars().take(k).collect();
                        *i += k as isize - 1;
                        Ok(get_var(&name))
                    }
                }
            }
        };
        let mut pushed = false;
        while (i as usize) < s.len() {
            let mut c = s[i as usize];
            is_glob = is_glob || (quote.is_none() && (c == '*' || c == '?'));
            if esc {
                out.push(c);
                esc = false;
            } else if let Some(q) = quote {
                if c == q {
                    quote = None;
                } else if q == '\'' {
                    out.push(c);
                } else if c == '\\' {
                    i += 1;
                    match at(i) {
                        Some(n) if n == '"' || n == '\\' || n == '$' => out.push(n),
                        // `BS + s.charAt(i)`: charAt past the end is "".
                        n => {
                            out.push('\\');
                            if let Some(n) = n {
                                out.push(n);
                            }
                        }
                    }
                    let _ = &mut c;
                } else if c == '$' {
                    out.push_str(&env_var(&mut i)?);
                } else {
                    out.push(c);
                }
            } else if c == '"' || c == '\'' {
                quote = Some(c);
            } else if control_re().is_match(&c.to_string()) {
                out_tokens.push(Token::Op(st.into()));
                pushed = true;
                break;
            } else if c == '#' {
                commented = true;
                let start = m.start() + s[..i as usize].iter().map(|c| c.len_utf8()).sum::<usize>() + 1;
                if !out.is_empty() {
                    out_tokens.push(Token::Str(std::mem::take(&mut out)));
                }
                out_tokens.push(Token::Comment(string[start.min(string.len())..].to_string()));
                pushed = true;
                break;
            } else if c == '\\' {
                esc = true;
            } else if c == '$' {
                out.push_str(&env_var(&mut i)?);
            } else {
                out.push(c);
            }
            i += 1;
        }
        if pushed {
            continue;
        }
        out_tokens.push(if is_glob { Token::Glob(out) } else { Token::Str(out) });
    }
    Ok(out_tokens)
}
