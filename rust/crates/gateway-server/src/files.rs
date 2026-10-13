//! Port of `src/adapters/web/projectFiles.ts` (ProjectFileService) and the `/api/web/projects/:key/files/*`
//! routes of `httpRouter.ts`. Error messages follow Node's `fs` format (`CODE: description, syscall 'path'`).
use crate::{obj, s, JsValue};
use pilotdeck_session_search::paths;
use std::fs;
use std::io;

pub enum FileErr {
    Boundary(String),
    Other(String),
}
type R<T> = Result<T, FileErr>;

/// Node errno message (`uvException`), e.g. `ENOENT: no such file or directory, stat '/x'`.
pub fn node_fs_error(e: &io::Error, syscall: &str, path: &str) -> FileErr {
    let (code, desc) = match e.raw_os_error() {
        Some(2) => ("ENOENT", "no such file or directory"),
        Some(13) => ("EACCES", "permission denied"),
        Some(20) => ("ENOTDIR", "not a directory"),
        Some(21) => ("EISDIR", "illegal operation on a directory"),
        Some(40) => ("ELOOP", "too many symbolic links encountered"),
        Some(36) => ("ENAMETOOLONG", "name too long"),
        Some(17) => ("EEXIST", "file already exists"),
        Some(1) => ("EPERM", "operation not permitted"),
        Some(28) => ("ENOSPC", "no space left on device"),
        Some(30) => ("EROFS", "read-only file system"),
        Some(24) => ("EMFILE", "too many open files"),
        Some(5) => ("EIO", "i/o error"),
        _ => ("UNKNOWN", "unknown error"),
    };
    FileErr::Other(format!("{code}: {desc}, {syscall} '{path}'"))
}

/// Node `path.posix.relative(from, to)` for absolute, resolved inputs.
pub fn relative(from: &str, to: &str) -> String {
    let f = paths::resolve(&[from]);
    let t = paths::resolve(&[to]);
    if f == t {
        return String::new();
    }
    let fs_: Vec<&str> = f.split('/').filter(|x| !x.is_empty()).collect();
    let ts: Vec<&str> = t.split('/').filter(|x| !x.is_empty()).collect();
    let mut i = 0;
    while i < fs_.len() && i < ts.len() && fs_[i] == ts[i] {
        i += 1;
    }
    let mut out: Vec<&str> = std::iter::repeat("..").take(fs_.len() - i).collect();
    out.extend(&ts[i..]);
    out.join("/")
}

const TEXT_EXT: [&str; 28] = ["ts", "tsx", "js", "jsx", "json", "md", "mdx", "yml", "yaml", "txt", "css", "html", "py", "go", "rs", "java", "c", "cpp", "h", "hpp", "sh", "bash", "toml", "ini", "csv", "tsv", "env", "lock"];

pub struct ProjectFileService {
    root: String,
    max_read: u64,
}

fn iso(m: &fs::Metadata) -> String {
    // Portable form of `mtime * 1000 + mtime_nsec / 1e6` (floor), also for times before the epoch.
    let ms = match m.modified() {
        Ok(t) => match t.duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => d.as_millis() as i64,
            Err(e) => -(e.duration().as_nanos().div_ceil(1_000_000) as i64),
        },
        Err(_) => 0,
    };
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()).unwrap_or_default()
}

impl ProjectFileService {
    pub fn new(project_root: &str) -> Self {
        let resolved = paths::resolve(&[project_root]);
        let root = fs::canonicalize(&resolved).map(|p| p.to_string_lossy().into_owned()).unwrap_or(resolved);
        Self { root, max_read: 1024 * 1024 }
    }

    fn inside(&self, abs: &str) -> bool {
        let rel = relative(&self.root, abs);
        !rel.starts_with("..") && !rel.starts_with('/')
    }
    fn to_relative(&self, abs: &str) -> String {
        let rel = relative(&self.root, abs);
        if rel.is_empty() { ".".into() } else { rel.split(['/', '\\']).collect::<Vec<_>>().join("/") }
    }

    fn lexical(&self, rel: &str) -> R<String> {
        if rel.starts_with('/') {
            return Err(FileErr::Boundary(rel.into()));
        }
        let n = paths::normalize(rel);
        if n.starts_with("..") {
            return Err(FileErr::Boundary(rel.into()));
        }
        let target = paths::resolve(&[&self.root, &n]);
        if !self.inside(&target) {
            return Err(FileErr::Boundary(rel.into()));
        }
        Ok(target)
    }

    pub fn resolve_safe(&self, rel: &str) -> R<String> {
        let target = self.lexical(rel)?;
        match fs::canonicalize(&target) {
            Ok(real) => {
                let real = real.to_string_lossy().into_owned();
                if !self.inside(&real) {
                    return Err(FileErr::Boundary(rel.into()));
                }
                Ok(real)
            }
            Err(e) if e.raw_os_error() == Some(2) => Ok(target),
            Err(e) => Err(node_fs_error(&e, "realpath", &target)),
        }
    }

    pub fn read_tree(&self, rel: &str) -> R<JsValue> {
        let target = self.resolve_safe(rel)?;
        let st = fs::metadata(&target).map_err(|e| node_fs_error(&e, "stat", &target))?;
        if !st.is_dir() {
            return Err(FileErr::Other(format!("Not a directory: {rel}")));
        }
        let rd = fs::read_dir(&target).map_err(|e| node_fs_error(&e, "scandir", &target))?;
        let mut entries: Vec<(String, &'static str, JsValue)> = Vec::new();
        for d in rd {
            let d = d.map_err(|e| node_fs_error(&e, "scandir", &target))?;
            let name = d.file_name().to_string_lossy().into_owned();
            let child = paths::resolve(&[&target, &name]);
            let ft = d.file_type().ok();
            let ty = match ft {
                Some(t) if t.is_dir() => "directory",
                Some(t) if t.is_file() => "file",
                Some(t) if t.is_symlink() => "symlink",
                _ => "other",
            };
            let mut e = obj(vec![("name", s(&name)), ("path", s(&self.to_relative(&child))), ("type", s(ty))]);
            if let (Ok(m), JsValue::Obj(o)) = (fs::metadata(&child), &mut e) {
                o.set("size", JsValue::Num(m.len() as f64));
                o.set("modifiedAt", s(&iso(&m)));
            }
            entries.push((name, ty, e));
        }
        entries.sort_by(|a, b| match (a.1 == "directory", b.1 == "directory") {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => name_locale_compare(&a.0, &b.0),
        });
        Ok(obj(vec![("projectKey", s(&self.root)), ("entries", JsValue::Arr(entries.into_iter().map(|x| x.2).collect()))]))
    }

    pub fn read_file(&self, rel: &str) -> R<JsValue> {
        let target = self.resolve_safe(rel)?;
        let st = fs::metadata(&target).map_err(|e| node_fs_error(&e, "stat", &target))?;
        if !st.is_file() {
            return Err(FileErr::Other(format!("Not a file: {rel}")));
        }
        let ext = rel.rsplit('.').next().unwrap_or("").to_lowercase();
        let is_text = TEXT_EXT.contains(&ext.as_str());
        let path = self.to_relative(&target);
        let size = JsValue::Num(st.len() as f64);
        if st.len() > self.max_read {
            return Ok(obj(vec![("projectKey", s(&self.root)), ("path", s(&path)), ("size", size),
                ("encoding", s(if is_text { "utf8" } else { "base64" })), ("binary", JsValue::Bool(!is_text))]));
        }
        let bytes = fs::read(&target).map_err(|e| node_fs_error(&e, "open", &target))?;
        if is_text {
            return Ok(obj(vec![("projectKey", s(&self.root)), ("path", s(&path)), ("size", size), ("encoding", s("utf8")),
                ("content", s(&String::from_utf8_lossy(&bytes)))]));
        }
        use base64::Engine;
        Ok(obj(vec![("projectKey", s(&self.root)), ("path", s(&path)), ("size", size), ("encoding", s("base64")),
            ("content", s(&base64::engine::general_purpose::STANDARD.encode(&bytes))), ("binary", JsValue::Bool(true))]))
    }

    /// Real-path boundary check for a write target: an existing target (including a symlink, even a dangling one)
    /// must resolve inside the root, and so must the deepest existing ancestor.
    fn check_write_real(&self, rel: &str, target: &str) -> R<()> {
        if let Ok(m) = fs::symlink_metadata(target) {
            match fs::canonicalize(target) {
                Ok(real) if self.inside(&real.to_string_lossy()) => return Ok(()),
                Ok(_) => return Err(FileErr::Boundary(rel.into())),
                Err(_) if m.file_type().is_symlink() => return Err(FileErr::Boundary(rel.into())),
                Err(_) => {}
            }
        }
        let mut cur = paths::resolve(&[target, ".."]);
        loop {
            if let Ok(real) = fs::canonicalize(&cur) {
                return if self.inside(&real.to_string_lossy()) { Ok(()) } else { Err(FileErr::Boundary(rel.into())) };
            }
            if fs::symlink_metadata(&cur).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(FileErr::Boundary(rel.into())); // dangling symlinked directory: mkdir would follow it
            }
            let parent = paths::resolve(&[&cur, ".."]);
            if parent == cur {
                return Ok(());
            }
            cur = parent;
        }
    }

    /// Intended difference from TS (SRC-DEFECT-007, ADR-0004): writes that resolve outside the root through a
    /// symlink are refused with `workspace_boundary` (TS only checks the path text and follows the link).
    /// The path check runs first. The write itself goes through `safe_write` (openat/O_NOFOLLOW from a root fd), so a
    /// symlink swapped in after the check cannot be followed and no directories are created outside the root.
    pub fn write_file(&self, rel: &str, content: &str, base64_enc: bool) -> R<()> {
        let target = self.lexical(rel)?;
        self.check_write_real(rel, &target)?;
        let data = if base64_enc { node_base64_decode(content) } else { content.as_bytes().to_vec() };
        // The fd walk is the real guard (no check-then-use race); the path check above only keeps error order.
        let rel_norm = relative(&self.root, &target);
        let comps: Vec<String> = rel_norm.split('/').filter(|c| !c.is_empty()).map(str::to_string).collect();
        match crate::safe_write::write_in_root(&self.root, &comps, &data) {
            Ok(()) => Ok(()),
            Err(crate::safe_write::WriteErr::Boundary) => Err(FileErr::Boundary(rel.into())),
            Err(crate::safe_write::WriteErr::Io(e, syscall, path)) => Err(node_fs_error(&e, syscall, &path)),
        }
    }
}

/// Node `Buffer.from(s, "base64")`: accepts base64 and base64url alphabets, skips other characters,
/// stops at the first '='.
pub fn node_base64_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.chars() {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' | '-' => 62,
            '/' | '_' => 63,
            '=' => break,
            _ => continue,
        };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

/// WHATWG `URLSearchParams.get(name)` over the request target's query.
pub fn search_param(target: &str, name: &str) -> Option<String> {
    let t = target.split('#').next().unwrap_or("");
    let q = t.split_once('?')?.1;
    for pair in q.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if form_decode(k) == name {
            return Some(form_decode(v));
        }
    }
    None
}

fn form_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit() => {
                out.push(u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap(), 16).unwrap());
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `a.localeCompare(b)` (ICU root collation) for file names: primary on base letters (accents
/// stripped via NFD, ASCII ordering from the sidecar approximation), then secondary (accents),
/// then tertiary (lowercase first). Verified against Node in tests/locale_names.rs.
pub fn name_locale_compare(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    use unicode_normalization::char::is_combining_mark;
    use unicode_normalization::UnicodeNormalization;
    let split = |s: &str| -> (String, Vec<Vec<char>>) {
        let mut base = String::new();
        let mut marks: Vec<Vec<char>> = Vec::new();
        for c in s.nfd() {
            if is_combining_mark(c) {
                // DUCET secondary order for common Latin marks (acute < grave < breve < circumflex ...).
                const ORDER: [char; 13] = ['\u{301}', '\u{300}', '\u{306}', '\u{302}', '\u{30c}', '\u{30a}', '\u{308}', '\u{30b}', '\u{303}', '\u{307}', '\u{327}', '\u{328}', '\u{304}'];
                let w = ORDER.iter().position(|&o| o == c).map_or(0x1000 + c as u32, |i| i as u32 + 1);
                if let Some(m) = marks.last_mut() {
                    m.push(char::from_u32(0xE000 + w).unwrap_or(c));
                }
            } else if c == '\u{df}' {
                // ICU: sharp s expands to "ss" at primary strength; differs at tertiary.
                base.push_str("ss");
                marks.push(Vec::new());
                marks.push(vec!['\u{df}']);
            } else {
                base.push(c);
                marks.push(Vec::new());
            }
        }
        (base, marks)
    };
    let (ba, ma) = split(a);
    let (bb, mb) = split(b);
    let lower = |s: &str| s.to_lowercase();
    match pilotdeck_agent_loop_sidecar::cacheplan::locale_compare(&lower(&ba), &lower(&bb)) {
        Ordering::Equal => {}
        o => return o,
    }
    match ma.cmp(&mb) {
        Ordering::Equal => {}
        o => return o,
    }
    pilotdeck_agent_loop_sidecar::cacheplan::locale_compare(&ba, &bb)
}
