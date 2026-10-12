//! Port of the subset of `src/pilot/paths.ts` and Node `path` semantics used by
//! chat-history search.

use std::fs;

/// Node `path.posix.normalize`-style segment normalization of an absolute path.
fn normalize_absolute(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    format!("/{}", parts.join("/"))
}

pub fn cwd() -> String {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "/".to_string())
}

/// Node `path.resolve(...segments)` on POSIX.
pub fn resolve(segments: &[&str]) -> String {
    let mut acc = String::new();
    for seg in segments.iter().rev() {
        if seg.is_empty() {
            continue;
        }
        acc = if acc.is_empty() { seg.to_string() } else { format!("{seg}/{acc}") };
        if seg.starts_with('/') {
            return normalize_absolute(&acc);
        }
    }
    let base = cwd();
    if acc.is_empty() {
        normalize_absolute(&base)
    } else {
        normalize_absolute(&format!("{base}/{acc}"))
    }
}

/// Node `path.join(...segments)` on POSIX.
pub fn join(segments: &[&str]) -> String {
    let joined: Vec<&str> = segments.iter().copied().filter(|s| !s.is_empty()).collect();
    if joined.is_empty() {
        return ".".to_string();
    }
    normalize(&joined.join("/"))
}

/// Node `path.posix.normalize`.
pub fn normalize(path: &str) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    let absolute = path.starts_with('/');
    let trailing = path.ends_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|p| *p != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            s => parts.push(s),
        }
    }
    let mut out = parts.join("/");
    if out.is_empty() && !absolute {
        out = ".".to_string();
    }
    if trailing && !out.is_empty() && out != "." {
        out.push('/');
    }
    if absolute {
        format!("/{out}")
    } else {
        out
    }
}

fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

/// `resolvePilotHome(process.env)`
pub fn resolve_pilot_home_from_env() -> String {
    let raw = std::env::var("PILOT_HOME").unwrap_or_else(|_| "~/.pilotdeck".to_string());
    normalize_home_path(&raw)
}

fn normalize_home_path(path: &str) -> String {
    if path == "~" {
        return home_dir();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        let home = home_dir();
        return resolve(&[&home, rest]);
    }
    resolve(&[path])
}

fn create_legacy_project_id(project_root: &str) -> String {
    let normalized = project_root.replace('\\', "/");
    let bytes = normalized.as_bytes();
    let normalized = if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        normalized[2..].to_string()
    } else {
        normalized
    };
    let mut out = String::new();
    let mut in_run = false;
    for c in normalized.chars() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
            out.push(c);
            in_run = false;
        } else if !in_run {
            out.push('-');
            in_run = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed.to_string()
    }
}

fn find_stored_project_id(project_root: &str, pilot_home: &str) -> Option<String> {
    let projects_dir = resolve(&[pilot_home, "projects"]);
    let target = resolve(&[project_root]);
    let entries = fs::read_dir(&projects_dir).ok()?;
    let mut names: Vec<(String, bool)> = entries
        .filter_map(|e| e.ok())
        .map(|e| {
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            (e.file_name().to_string_lossy().into_owned(), is_dir)
        })
        .collect();
    names.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    for (name, is_dir) in names {
        if !is_dir {
            continue;
        }
        let marker_path = resolve(&[&projects_dir, &name, ".cwd"]);
        let Ok(bytes) = fs::read(&marker_path) else { continue };
        let raw = String::from_utf8_lossy(&bytes);
        let marker = crate::js::js_trim(&raw);
        if marker.is_empty() || resolve(&[marker]) != target {
            continue;
        }
        if fs::metadata(marker).map(|m| m.is_dir()).unwrap_or(false) {
            return Some(name);
        }
    }
    None
}

pub fn resolve_project_storage_id(project_root: &str, pilot_home: &str) -> String {
    find_stored_project_id(project_root, pilot_home)
        .unwrap_or_else(|| create_legacy_project_id(&resolve(&[project_root])))
}

/// `getPilotProjectChatDir(projectRoot, pilotHome)`
pub fn project_chat_dir(project_root: &str, pilot_home: &str) -> String {
    let id = resolve_project_storage_id(project_root, pilot_home);
    resolve(&[pilot_home, "projects", &id, "chats"])
}

/// `sanitizeSessionIdForPath` (POSIX branch).
pub fn sanitize_session_id_for_path(session_id: &str) -> String {
    let mut out = String::new();
    let mut in_run = false;
    for c in session_id.chars() {
        if c == '/' || c == '\\' {
            if !in_run {
                out.push('-');
                in_run = true;
            }
        } else {
            out.push(c);
            in_run = false;
        }
    }
    let trimmed = out.trim_start_matches('-').trim_end_matches('-');
    if trimmed.is_empty() {
        "session".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_ids() {
        assert_eq!(create_legacy_project_id("/Users/foo/my proj"), "Users-foo-my-proj");
        assert_eq!(create_legacy_project_id("C:\\Users\\foo"), "Users-foo");
        assert_eq!(create_legacy_project_id("/"), "project");
    }

    #[test]
    fn path_semantics() {
        assert_eq!(resolve(&["/a/b", "../c", "d/"]), "/a/c/d");
        assert_eq!(join(&["a", "projects", "x", "chats"]), "a/projects/x/chats");
        assert_eq!(join(&["./a/", "../b"]), "b");
        assert_eq!(sanitize_session_id_for_path("/a//b/"), "a-b");
        assert_eq!(sanitize_session_id_for_path("//"), "session");
    }
}
