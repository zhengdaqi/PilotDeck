//! `Number.prototype.toString()` (radix 10) for any f64, following ECMA-262 Number::toString.
pub fn js_number_to_string(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f == 0.0 {
        return "0".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if f < 0.0 {
        return format!("-{}", js_number_to_string(-f));
    }
    // Shortest round-trip digits: "d.ddde<exp>".
    let e = format!("{f:e}");
    let (mant, exp) = e.split_once('e').unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp.parse::<i32>().unwrap() + 1;
    if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let sign = if n > 0 { "+" } else { "-" };
        let first = &digits[..1];
        let rest = if k > 1 { format!(".{}", &digits[1..]) } else { String::new() };
        format!("{first}{rest}e{sign}{}", (n - 1).abs())
    }
}

pub fn is_js_ws(c: char) -> bool {
    matches!(c, '\u{9}' | '\u{a}' | '\u{b}' | '\u{c}' | '\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

/// `String.prototype.trim`.
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_ws)
}

/// `Number(s)` for a string.
pub fn number(s: &str) -> f64 {
    let t = js_trim(s);
    if t.is_empty() {
        return 0.0;
    }
    let radix = |p: &str, r: u32| u64::from_str_radix(p, r).map(|n| n as f64).unwrap_or(f64::NAN);
    let lower = t.to_ascii_lowercase();
    if let Some(p) = lower.strip_prefix("0x") {
        return if p.is_empty() || !p.bytes().all(|b| b.is_ascii_hexdigit()) { f64::NAN } else { radix(p, 16) };
    }
    if let Some(p) = lower.strip_prefix("0o") {
        return if p.is_empty() { f64::NAN } else { radix(p, 8) };
    }
    if let Some(p) = lower.strip_prefix("0b") {
        return if p.is_empty() { f64::NAN } else { radix(p, 2) };
    }
    let (sign, body) = match t.as_bytes()[0] {
        b'-' => (-1.0, &t[1..]),
        b'+' => (1.0, &t[1..]),
        _ => (1.0, t),
    };
    if body == "Infinity" {
        return sign * f64::INFINITY;
    }
    // StrDecimalLiteral: digits [. digits] [e[+-]digits], at least one digit in the mantissa.
    let b = body.as_bytes();
    let mut i = 0;
    let mut digits = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return f64::NAN;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let s0 = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == s0 {
            return f64::NAN;
        }
    }
    if i != b.len() {
        return f64::NAN;
    }
    sign * body.parse::<f64>().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::js_number_to_string as s;
    #[test]
    fn matches_v8() {
        for (f, want) in [(1e21, "1e+21"), (1e-7, "1e-7"), (123.456, "123.456"), (0.000001, "0.000001"), (1.5e300, "1.5e+300"), (-2.5, "-2.5"), (100.0, "100"), (5e-324, "5e-324")] {
            assert_eq!(s(f), want);
        }
    }
}
