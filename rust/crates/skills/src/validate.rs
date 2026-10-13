//! Port of `validateFromDisk` / frontmatter parsing in SkillManager.ts.

use crate::jsjson::{s, JsObject, JsValue};
use pilotdeck_session_search::js::{js_trim, utf16_len};
use pilotdeck_session_search::paths;
use regex::Regex;
use std::fs;
use std::sync::OnceLock;

pub const MAX_TOTAL_BYTES: u64 = 50 * 1024 * 1024;
pub const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_FILE_COUNT: u64 = 500;
const RISKY_EXTS: &[&str] = &[".sh", ".bash", ".zsh", ".fish", ".exe", ".bat", ".cmd", ".dll", ".so", ".dylib"];

#[derive(Debug, Clone)]
pub struct Issue {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Validation {
    pub hard_fails: Vec<Issue>,
    pub warnings: Vec<Issue>,
    pub file_count: u64,
    pub total_bytes: u64,
    pub frontmatter: Option<JsObject>,
}

impl Validation {
    pub fn ok(&self) -> bool {
        self.hard_fails.is_empty()
    }

    pub fn to_js(&self, source_path: Option<&str>) -> JsValue {
        let issues = |v: &Vec<Issue>| {
            JsValue::Arr(
                v.iter()
                    .map(|i| JsValue::Obj(JsObject::new().with("code", s(&i.code)).with("message", s(&i.message))))
                    .collect(),
            )
        };
        let mut o = JsObject::new()
            .with("ok", JsValue::Bool(self.ok()))
            .with("hardFails", issues(&self.hard_fails))
            .with("warnings", issues(&self.warnings))
            .with(
                "stats",
                JsValue::Obj(
                    JsObject::new()
                        .with("fileCount", JsValue::Num(self.file_count as f64))
                        .with("totalBytes", JsValue::Num(self.total_bytes as f64)),
                ),
            )
            .with("frontmatter", self.frontmatter.clone().map(JsValue::Obj).unwrap_or(JsValue::Null));
        if let Some(p) = source_path {
            o.set("sourcePath", s(p));
        }
        JsValue::Obj(o)
    }

    pub fn hard_fail_messages(&self) -> String {
        self.hard_fails.iter().map(|i| i.message.as_str()).collect::<Vec<_>>().join("; ")
    }
}

fn push(v: &mut Vec<Issue>, code: &str, message: String) {
    v.push(Issue { code: code.into(), message });
}

fn yaml_key(k: &serde_yaml::Value) -> String {
    match k {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Null => String::new(),
        serde_yaml::Value::Number(n) => crate::jsjson::js_number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        other => serde_yaml::to_string(other).unwrap_or_default().trim_end().to_string(),
    }
}

fn yaml_to_js(v: &serde_yaml::Value) -> JsValue {
    match v {
        serde_yaml::Value::Null => JsValue::Null,
        serde_yaml::Value::Bool(b) => JsValue::Bool(*b),
        serde_yaml::Value::Number(n) => JsValue::Num(n.as_f64().unwrap_or(f64::NAN)),
        serde_yaml::Value::String(s) => JsValue::Str(s.clone()),
        serde_yaml::Value::Sequence(items) => JsValue::Arr(items.iter().map(yaml_to_js).collect()),
        serde_yaml::Value::Mapping(map) => {
            let mut o = JsObject::new();
            for (k, v) in map {
                o.set(yaml_key(k), yaml_to_js(v));
            }
            JsValue::Obj(o)
        }
        serde_yaml::Value::Tagged(t) => yaml_to_js(&t.value),
    }
}

/// JS `split(/\r?\n/)`
fn split_lines(s: &str) -> Vec<&str> {
    s.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect()
}

fn parse_compat(fm_raw: &str) -> JsObject {
    static SCALAR: OnceLock<Regex> = OnceLock::new();
    static KEYLINE: OnceLock<Regex> = OnceLock::new();
    static BLOCK: OnceLock<Regex> = OnceLock::new();
    let scalar = SCALAR.get_or_init(|| Regex::new(r"^([A-Za-z][A-Za-z0-9_-]*)\s*:\s*(.+?)\s*$").unwrap());
    let keyline = KEYLINE.get_or_init(|| Regex::new(r"^[A-Za-z][A-Za-z0-9_-]*\s*:").unwrap());
    let block = BLOCK.get_or_init(|| Regex::new(r"^([>|])\s*$").unwrap());
    let lines = split_lines(fm_raw);
    let mut out = JsObject::new();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        if let Some(c) = scalar.captures(line) {
            let key = c[1].to_string();
            let raw = c[2].to_string();
            if block.is_match(&raw) {
                let mut block_lines = Vec::new();
                i += 1;
                while i < lines.len() {
                    let next = lines[i];
                    if keyline.is_match(next) {
                        i -= 1;
                        break;
                    }
                    let stripped = next.strip_prefix("  ").or_else(|| next.strip_prefix(' ')).unwrap_or(next);
                    block_lines.push(stripped.to_string());
                    i += 1;
                }
                out.set(key, s(js_trim(&block_lines.join("\n"))));
            } else {
                let t = js_trim(&raw);
                let t = t.strip_prefix(['"', '\'']).unwrap_or(t);
                let t = t.strip_suffix(['"', '\'']).unwrap_or(t);
                out.set(key, s(t));
            }
        }
        i += 1;
    }
    out
}

/// `parseSkillFrontmatterWithMeta` → (frontmatter, usedCompatibilityFallback)
pub fn parse_frontmatter(content: &str) -> (JsObject, bool) {
    let Some(rest) = content.strip_prefix("---") else { return (JsObject::new(), false) };
    static END: OnceLock<Regex> = OnceLock::new();
    let end = END.get_or_init(|| Regex::new(r"\r?\n---").unwrap());
    let Some(m) = end.find(rest) else { return (JsObject::new(), false) };
    let raw = &rest[..m.start()];
    let raw = raw.strip_prefix("\r\n").or_else(|| raw.strip_prefix('\n')).unwrap_or(raw);
    match serde_yaml::from_str::<serde_yaml::Value>(raw) {
        Ok(v) => match yaml_to_js(&v) {
            JsValue::Obj(o) => (o, false),
            _ => (JsObject::new(), false),
        },
        Err(_) => {
            let compat = parse_compat(raw);
            let used = !compat.is_empty();
            (compat, used)
        }
    }
}

fn validate_required_frontmatter(content: &str, hard: &mut Vec<Issue>, warn: &mut Vec<Issue>) -> Option<JsObject> {
    if js_trim(content).is_empty() {
        push(hard, "no_skill_md", "SKILL.md is empty or missing.".into());
        return None;
    }
    let (fm, compat) = parse_frontmatter(content);
    if fm.is_empty() && !content.starts_with("---") {
        push(hard, "frontmatter_missing", "SKILL.md does not start with a YAML frontmatter block.".into());
        return Some(fm);
    }
    if compat {
        push(
            warn,
            "frontmatter_compat_fallback",
            "Frontmatter was parsed with compatibility fallback; consider standard YAML formatting.".into(),
        );
    }
    let nonblank = |k: &str| matches!(fm.get(k), Some(JsValue::Str(v)) if !js_trim(v).is_empty());
    if !nonblank("name") {
        push(hard, "frontmatter_missing_name", "Frontmatter is missing required field: name.".into());
    }
    if !nonblank("description") {
        push(
            hard,
            "frontmatter_missing_description",
            "Frontmatter is missing required field: description (skill won't surface in the slash menu without it)."
                .into(),
        );
    } else if let Some(JsValue::Str(d)) = fm.get("description") {
        let len = utf16_len(js_trim(d));
        if len < 20 {
            push(warn, "description_short", format!("Description is short ({len} chars). Consider expanding for better discovery."));
        }
        if len > 1024 {
            push(warn, "description_long", format!("Description is very long ({len} chars). Most slash-menu surfaces truncate this."));
        }
    }
    Some(fm)
}

/// Node `readdir` order (libuv scandir sorts names bytewise).
pub fn read_dir_sorted(dir: &str) -> std::io::Result<Vec<(String, fs::FileType)>> {
    let mut v: Vec<(String, fs::FileType)> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), e.file_type().ok()?)))
        .collect();
    v.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    Ok(v)
}

fn ext_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[i..],
        _ => "",
    }
}

/// `(size / 1024 / 1024).toFixed(1)` computed exactly (round half up).
fn mb_to_fixed1(size: u64) -> String {
    let n = size as u128 * 10;
    let d = 1024u128 * 1024;
    let mut q = n / d;
    if (n % d) * 2 >= d {
        q += 1;
    }
    format!("{}.{}", q / 10, q % 10)
}

fn walk(dir: &str, rel_prefix: &str, v: &mut Validation) {
    let Ok(entries) = read_dir_sorted(dir) else { return };
    for (name, ft) in entries {
        if v.file_count > MAX_FILE_COUNT {
            return;
        }
        let rel = if rel_prefix.is_empty() { paths::normalize(&name) } else { paths::join(&[rel_prefix, &name]) };
        let abs = paths::join(&[dir, &name]);
        if ft.is_symlink() {
            push(&mut v.warnings, "contains_symlink", format!("Bundle contains a symlink: {rel}"));
            continue;
        }
        if ft.is_dir() {
            walk(&abs, &rel, v);
            continue;
        }
        v.file_count += 1;
        if let Ok(md) = fs::metadata(&abs) {
            let size = md.len();
            v.total_bytes += size;
            if size > MAX_FILE_BYTES {
                push(&mut v.hard_fails, "file_too_large", format!("File exceeds {MAX_FILE_BYTES} bytes: {rel} ({size} bytes)"));
            } else if size > 1024 * 1024 {
                push(&mut v.warnings, "file_large", format!("Large file: {rel} ({} MB)", mb_to_fixed1(size)));
            }
        }
        let ext = ext_of(&name).to_lowercase();
        if RISKY_EXTS.contains(&ext.as_str()) {
            push(&mut v.warnings, "risky_extension", format!("Executable-style file ({ext}): {rel}"));
        }
    }
}

/// `validateFromDisk(sourcePath)`
pub fn validate_from_disk(source: &str) -> Validation {
    let mut v = Validation { hard_fails: vec![], warnings: vec![], file_count: 0, total_bytes: 0, frontmatter: None };
    let md = match fs::metadata(source) {
        Ok(m) => m,
        Err(_) => {
            push(&mut v.hard_fails, "source_missing", format!("Source path does not exist: {source}"));
            return v;
        }
    };
    if !md.is_dir() {
        push(&mut v.hard_fails, "source_not_directory", format!("Source path is not a directory: {source}"));
        return v;
    }
    let content = match fs::read(paths::join(&[source, "SKILL.md"])) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(_) => {
            push(&mut v.hard_fails, "no_skill_md", "Source folder does not contain a SKILL.md at the root.".into());
            return v;
        }
    };
    let (mut hard, mut warn) = (vec![], vec![]);
    v.frontmatter = validate_required_frontmatter(&content, &mut hard, &mut warn);
    v.hard_fails = hard;
    v.warnings = warn;
    walk(source, "", &mut v);
    if v.file_count > MAX_FILE_COUNT {
        push(&mut v.hard_fails, "too_many_files", format!("Bundle has more than {MAX_FILE_COUNT} files."));
    }
    if v.total_bytes > MAX_TOTAL_BYTES {
        push(
            &mut v.hard_fails,
            "total_too_large",
            format!("Bundle total size exceeds {MAX_TOTAL_BYTES} bytes ({}).", v.total_bytes),
        );
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_fixed() {
        assert_eq!(mb_to_fixed1(1310720), "1.3");
        assert_eq!(mb_to_fixed1(1048577), "1.0");
    }

    #[test]
    fn compat_frontmatter() {
        let (fm, compat) = parse_frontmatter("---\nname: a: b\ndescription: |\n  line one\n  line two\n---\nbody");
        assert!(compat);
        assert_eq!(fm.get("name"), Some(&s("a: b")));
        assert_eq!(fm.get("description"), Some(&s("line one\nline two")));
    }
}
