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
        let sign = if n - 1 >= 0 { "+" } else { "-" };
        let first = &digits[..1];
        let rest = if k > 1 { format!(".{}", &digits[1..]) } else { String::new() };
        format!("{first}{rest}e{sign}{}", (n - 1).abs())
    }
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
