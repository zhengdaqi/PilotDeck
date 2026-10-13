//! Port of ui/server/services/releaseService.js: `normalizeRepository`, `releaseVersion`, `compareVersions`,
//! `getLatestRelease` (host performs the GET; 15 s timeout and the `Accept` / `User-Agent` headers are the host's
//! request contract). Regex classes are ASCII as in JS (`\w`, `\d`); `String(v)` coercions are kept.
use crate::js_number::js_trim;
use crate::js_object::{field, truthy};
use crate::js_object_string;
use crate::model_probe::encode_uri_component;
use serde_json::{json, Map, Value};
use std::future::Future;

#[derive(Clone, Debug, PartialEq)]
pub struct ReleaseError {
    /// "Error" | "TypeError" | "SyntaxError"
    pub name: &'static str,
    /// `None` for a JSON SyntaxError (V8 text is not part of the contract).
    pub message: Option<String>,
}

fn err(m: &str) -> ReleaseError {
    ReleaseError { name: "Error", message: Some(m.into()) }
}

pub const RELEASE_REPOSITORY: &str = "OpenBMB/PilotDeck";

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `String(v || '')`
fn str_or_empty(v: Option<&Value>) -> String {
    if truthy(v) { js_object_string(v.unwrap()) } else { String::new() }
}

/// `normalizeRepository(value)`; `None` = undefined (the default applies).
pub fn normalize_repository(value: Option<&Value>) -> Result<String, ReleaseError> {
    let s = match value {
        None => RELEASE_REPOSITORY.to_string(),
        Some(v) => js_object_string(v),
    };
    let mut r = js_trim(&s).to_string();
    if let Some(x) = r.strip_prefix("https://github.com/") {
        r = x.to_string();
    }
    if let Some(x) = r.strip_suffix(".git/").or_else(|| r.strip_suffix(".git")) {
        r = x.to_string();
    }
    if let Some(x) = r.strip_suffix('/') {
        r = x.to_string();
    }
    let part_ok = |p: &str| !p.is_empty() && p.chars().all(|c| is_word(c) || c == '.' || c == '-');
    let valid = match r.split_once('/') {
        Some((a, b)) => part_ok(a) && part_ok(b),
        None => false,
    };
    if !valid || r.split('/').any(|p| !p.is_empty() && p.chars().all(|c| c == '.')) {
        return Err(err("Invalid release repository."));
    }
    Ok(r)
}

/// `RELEASE_TAG = /^v\d{4}\.\d{2}\.\d{2}(?:-r[1-9]\d*)?$/`
fn is_release_tag(s: &str) -> bool {
    let b = s.as_bytes();
    let d = |r: std::ops::Range<usize>| b.get(r).is_some_and(|x| x.iter().all(u8::is_ascii_digit));
    if b.len() < 11 || b[0] != b'v' || !d(1..5) || b[5] != b'.' || !d(6..8) || b[8] != b'.' || !d(9..11) {
        return false;
    }
    let rest = &b[11..];
    rest.is_empty() || (rest.len() >= 3 && &rest[..2] == b"-r" && (b'1'..=b'9').contains(&rest[2]) && rest[3..].iter().all(u8::is_ascii_digit))
}

const MAX_SAFE: f64 = 9_007_199_254_740_991.0;

fn days_in_month(year: f64, month: f64) -> f64 {
    // `new Date(Date.UTC(year, month, 0)).getUTCDate()`; Date.UTC maps years 0..=99 to 1900..=1999.
    let y = if (0.0..=99.0).contains(&year) { 1900.0 + year } else { year };
    let leap = (y % 4.0 == 0.0 && y % 100.0 != 0.0) || y % 400.0 == 0.0;
    match month as i64 {
        2 => if leap { 29.0 } else { 28.0 },
        4 | 6 | 9 | 11 => 30.0,
        _ => 31.0,
    }
}

/// `releaseVersion(tag)`
pub fn release_version(tag: Option<&Value>) -> Result<String, ReleaseError> {
    if !is_release_tag(&str_or_empty(tag)) {
        return Err(err("Invalid release tag."));
    }
    let Some(Value::String(t)) = tag else {
        return Err(ReleaseError { name: "TypeError", message: Some("tag.match is not a function".into()) });
    };
    let nums: Vec<f64> = t.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).map(|p| p.parse::<f64>().unwrap()).collect();
    let (year, month, day) = (nums[0], nums[1], nums[2]);
    let release = nums.get(3).copied().unwrap_or(1.0);
    if !(1.0..=12.0).contains(&month) || day < 1.0 || day > days_in_month(year, month) || !(1.0..=MAX_SAFE).contains(&release) {
        return Err(err("Invalid release date or revision."));
    }
    Ok(format!("{}.{}.{}", year, month * 100.0 + day, release - 1.0))
}

fn version_parts(v: Option<&Value>) -> Result<[f64; 3], ReleaseError> {
    let s = str_or_empty(v);
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty() || !p.bytes().all(|c| c.is_ascii_digit())) {
        return Err(err("Invalid application version."));
    }
    let n: Vec<f64> = parts.iter().map(|p| p.parse::<f64>().unwrap()).collect();
    if n.iter().any(|x| *x > MAX_SAFE) {
        return Err(err("Invalid application version."));
    }
    Ok([n[0], n[1], n[2]])
}

/// `compareVersions(current, latest)` → -1 / 0 / 1.
pub fn compare_versions(current: Option<&Value>, latest: Option<&Value>) -> Result<i32, ReleaseError> {
    let (l, r) = (version_parts(current)?, version_parts(latest)?);
    for i in 0..3 {
        if l[i] != r[i] {
            return Ok(if l[i] < r[i] { -1 } else { 1 });
        }
    }
    Ok(0)
}

pub trait ReleaseHost {
    /// GET `url` with `Accept: application/json`, `User-Agent: PilotDeck-Updater`, 15 s timeout → (status, body text).
    fn get(&mut self, url: &str) -> impl Future<Output = Result<(u16, String), ReleaseError>>;
}

fn is_hex(s: &str, n: usize, ci: bool) -> bool {
    s.len() == n && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c) || (ci && (b'A'..=b'F').contains(&c)))
}

fn all_dots(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c == '.')
}

fn safe_integer(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|f| f.fract() == 0.0 && f.abs() <= MAX_SAFE)
}

/// `getLatestRelease({ repository })`.
pub async fn get_latest_release<H: ReleaseHost>(h: &mut H, repository: Option<&Value>) -> Result<Value, ReleaseError> {
    let repository = normalize_repository(repository)?;
    let (status, text) = h.get(&format!("https://github.com/{repository}/releases/latest/download/release.json")).await?;
    if !(200..300).contains(&status) {
        return Err(err(&format!("Release request failed ({status}).")));
    }
    let manifest: Value = serde_json::from_str(text.strip_prefix('\u{FEFF}').unwrap_or(&text)).map_err(|_| ReleaseError { name: "SyntaxError", message: None })?;
    let m = Some(&manifest);
    let mismatch = || err("Release manifest does not match the published release.");
    let expected = release_version(field(m, "tag")).map_err(|_| mismatch())?;
    let tag = field(m, "tag").and_then(Value::as_str).unwrap().to_string();
    let date_ok = field(m, "date") == Some(&json!(tag.get(1..11).unwrap_or("").replace('.', "-")));
    let assets = match field(m, "assets") {
        Some(Value::Array(a)) if !a.is_empty() => a,
        _ => return Err(mismatch()),
    };
    if field(m, "schemaVersion") != Some(&json!(1))
        || field(m, "repository") != Some(&json!(repository))
        || !is_hex(&str_or_empty(field(m, "sourceSha")), 40, false)
        || field(m, "version") != Some(&json!(expected))
        || !date_ok
    {
        return Err(mismatch());
    }
    let base = format!("https://github.com/{repository}/releases/download/{tag}");
    let mut names: Vec<Value> = Vec::new();
    let mut out = Vec::new();
    for asset in assets {
        if asset.is_null() {
            return Err(ReleaseError { name: "TypeError", message: Some("Cannot read properties of null (reading 'name')".into()) });
        }
        let a = Some(asset);
        let name_v = field(a, "name");
        let name_s = str_or_empty(name_v);
        let sha512 = str_or_empty(field(a, "sha512"));
        let sha512_ok = sha512.len() == 88 && sha512.ends_with("==") && sha512[..86].bytes().all(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'/');
        let size_ok = safe_integer(field(a, "size")).is_some_and(|s| s > 0.0);
        let invalid = name_s.is_empty()
            || !name_s.chars().all(|c| is_word(c) || c == '.' || c == '-')
            || all_dots(&name_v.map(js_object_string).unwrap_or_else(|| "undefined".into()))
            || names.contains(name_v.unwrap_or(&Value::Null))
            || !is_hex(&str_or_empty(field(a, "sha256")), 64, true)
            || !size_ok
            || !sha512_ok
            || !matches!(field(a, "platform"), Some(Value::String(_)))
            || !matches!(field(a, "arch"), Some(Value::String(_)));
        if invalid {
            return Err(err("Invalid installer manifest."));
        }
        let name = name_v.unwrap().clone();
        names.push(name.clone());
        let mut o: Map<String, Value> = asset.as_object().cloned().unwrap_or_default();
        o.insert("sha256".into(), json!(field(a, "sha256").and_then(Value::as_str).unwrap().to_lowercase()));
        o.insert("downloadUrl".into(), json!(format!("{base}/{}", encode_uri_component(&js_object_string(&name)))));
        out.push(Value::Object(o));
    }
    Ok(json!({
        "tagName": tag, "version": field(m, "version"), "sourceSha": field(m, "sourceSha"), "assets": out,
        "publishedAt": field(m, "date"), "body": "", "htmlUrl": format!("https://github.com/{repository}/releases/tag/{tag}"),
    }))
}
