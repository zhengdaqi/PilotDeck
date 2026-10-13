//! Port of `migrateSkillsToPilotDeck` (src/extension/skills/migrateSkills.ts)
//! for the user scope used by `pilotdeck skills migrate`.

use crate::jsjson::{s, JsObject, JsValue};
use crate::validate::{validate_from_disk, Validation};
use pilotdeck_session_search::paths;
use std::cmp::Ordering;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    ClaudeCode,
    OpenClaw,
    Hermes,
    Custom,
}

impl SourceKind {
    fn as_str(self) -> &'static str {
        match self {
            SourceKind::ClaudeCode => "claude-code",
            SourceKind::OpenClaw => "openclaw",
            SourceKind::Hermes => "hermes",
            SourceKind::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictMode {
    Skip,
    Overwrite,
    Rename,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Migrated,
    WouldMigrate,
    Skipped,
    Conflict,
    Error,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Migrated => "migrated",
            Status::WouldMigrate => "would_migrate",
            Status::Skipped => "skipped",
            Status::Conflict => "conflict",
            Status::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Source {
    pub kind: SourceKind,
    pub label: String,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub source_kind: SourceKind,
    pub source_label: String,
    pub source_path: String,
    pub destination_path: Option<String>,
    pub slug: String,
    pub status: Status,
    pub reason: String,
    /// (validation, include sourcePath echo)
    pub validation: Option<(Validation, Option<String>)>,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub execute: bool,
    pub target_root: String,
    pub sources: Vec<Source>,
    pub items: Vec<Item>,
}

impl Report {
    pub fn count(&self, st: Status) -> usize {
        self.items.iter().filter(|i| i.status == st).count()
    }

    pub fn to_js(&self) -> JsValue {
        let n = |st| JsValue::Num(self.count(st) as f64);
        let summary = JsObject::new()
            .with("migrated", n(Status::Migrated))
            .with("would_migrate", n(Status::WouldMigrate))
            .with("skipped", n(Status::Skipped))
            .with("conflict", n(Status::Conflict))
            .with("error", n(Status::Error));
        let sources = self
            .sources
            .iter()
            .map(|x| {
                JsValue::Obj(JsObject::new().with("kind", s(x.kind.as_str())).with("label", s(&x.label)).with("path", s(&x.path)))
            })
            .collect();
        let items = self
            .items
            .iter()
            .map(|i| {
                let mut o = JsObject::new()
                    .with("sourceKind", s(i.source_kind.as_str()))
                    .with("sourceLabel", s(&i.source_label))
                    .with("sourcePath", s(&i.source_path))
                    .with("destinationPath", i.destination_path.clone().map(JsValue::Str).unwrap_or(JsValue::Null))
                    .with("slug", s(&i.slug))
                    .with("status", s(i.status.as_str()))
                    .with("reason", s(&i.reason));
                if let Some((v, echo)) = &i.validation {
                    o.set("validation", v.to_js(echo.as_deref()));
                }
                JsValue::Obj(o)
            })
            .collect();
        JsValue::Obj(
            JsObject::new()
                .with("mode", s(if self.execute { "execute" } else { "dry-run" }))
                .with("targetRoot", s(&self.target_root))
                .with("sources", JsValue::Arr(sources))
                .with("summary", JsValue::Obj(summary))
                .with("items", JsValue::Arr(items)),
        )
    }
}

pub struct Options {
    pub pilot_home: String,
    pub project_root: Option<String>,
    pub include: Option<Vec<SourceKind>>,
    pub custom_sources: Vec<String>,
    pub execute: bool,
    pub conflict_mode: ConflictMode,
}

/// A thrown JS error that escapes `migrateSkillsToPilotDeck`.
#[derive(Debug)]
pub struct Thrown(pub String);

fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".into())
}

fn expand_home(p: &str) -> String {
    if p == "~" {
        return home();
    }
    if let Some(rest) = p.strip_prefix("~/") {
        return paths::join(&[&home(), rest]);
    }
    p.to_string()
}

fn build_sources(o: &Options) -> Vec<Source> {
    let home = home();
    let project_root = paths::resolve(&[o.project_root.as_deref().unwrap_or("")]);
    let include = o.include.clone().unwrap_or(vec![SourceKind::ClaudeCode, SourceKind::OpenClaw, SourceKind::Hermes]);
    let mut out = Vec::new();
    let mut add = |kind, label: &str, path: String| out.push(Source { kind, label: label.into(), path });
    for kind in include {
        match kind {
            SourceKind::ClaudeCode => {
                add(kind, "Claude Code user skills", paths::join(&[&home, ".claude", "skills"]));
                add(kind, "Claude Code project skills", paths::join(&[&project_root, ".claude", "skills"]));
            }
            SourceKind::OpenClaw => {
                let oc = paths::join(&[&home, ".openclaw"]);
                add(kind, "OpenClaw workspace skills", paths::join(&[&oc, "workspace", "skills"]));
                add(kind, "OpenClaw workspace-main skills", paths::join(&[&oc, "workspace-main", "skills"]));
                add(kind, "OpenClaw workspace-assistant skills", paths::join(&[&oc, "workspace-assistant", "skills"]));
                add(kind, "OpenClaw managed skills", paths::join(&[&oc, "skills"]));
                add(kind, "OpenClaw shared skills", paths::join(&[&home, ".agents", "skills"]));
                add(kind, "OpenClaw project shared skills", paths::join(&[&oc, "workspace", ".agents", "skills"]));
                add(kind, "OpenClaw default shared skills", paths::join(&[&oc, "workspace.default", ".agents", "skills"]));
            }
            SourceKind::Hermes => {
                let h = paths::join(&[&home, ".hermes"]);
                add(kind, "Hermes user skills", paths::join(&[&h, "skills"]));
                add(kind, "Hermes Claude-style skills", paths::join(&[&h, ".claude", "skills"]));
                add(kind, "Hermes agent skills", paths::join(&[&h, ".agents", "skills"]));
            }
            SourceKind::Custom => {}
        }
    }
    for p in &o.custom_sources {
        add(SourceKind::Custom, "Custom skills", paths::resolve(&[&expand_home(p)]));
    }
    for src in &mut out {
        src.path = paths::resolve(&[&src.path]);
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|x| seen.insert(x.path.clone()));
    out
}

/// `fs.access` (F_OK, follows symlinks).
fn exists(p: &str) -> bool {
    fs::metadata(p).is_ok()
}

/// ICU root-collation approximation for ASCII names (`localeCompare`):
/// punctuation < digits < letters, case-insensitive first, lowercase first on ties.
fn collate_key(c: char) -> (u32, u32) {
    const PUNCT: &str = "_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";
    if c.is_whitespace() {
        return (0, c as u32);
    }
    if let Some(i) = PUNCT.find(c) {
        return (1, i as u32);
    }
    if c.is_ascii_digit() {
        return (2, c as u32);
    }
    if c.is_ascii_alphabetic() {
        return (3, c.to_ascii_lowercase() as u32);
    }
    (4, c as u32)
}

pub fn locale_compare(a: &str, b: &str) -> Ordering {
    let ka: Vec<_> = a.chars().map(collate_key).collect();
    let kb: Vec<_> = b.chars().map(collate_key).collect();
    ka.cmp(&kb).then_with(|| {
        // tertiary: lowercase before uppercase
        let ta: Vec<bool> = a.chars().map(|c| c.is_ascii_uppercase()).collect();
        let tb: Vec<bool> = b.chars().map(|c| c.is_ascii_uppercase()).collect();
        ta.cmp(&tb)
    })
}

fn node_err(e: &std::io::Error, syscall: &str, path: &str) -> String {
    let code = match e.raw_os_error() {
        Some(20) => "ENOTDIR: not a directory",
        Some(13) => "EACCES: permission denied",
        Some(2) => "ENOENT: no such file or directory",
        Some(40) => "ELOOP: too many symbolic links encountered",
        _ => return format!("{e}, {syscall} '{path}'"),
    };
    format!("{code}, {syscall} '{path}'")
}

fn discover(root: &str) -> Result<Option<Vec<String>>, Thrown> {
    if exists(&paths::join(&[root, "SKILL.md"])) {
        return Ok(Some(vec![root.to_string()]));
    }
    let entries = match fs::read_dir(root) {
        Ok(_) => crate::validate::read_dir_sorted(root).map_err(|e| Thrown(node_err(&e, "scandir", root)))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Thrown(node_err(&e, "scandir", root))),
    };
    let mut dirs: Vec<String> = entries
        .into_iter()
        .filter(|(_, ft)| ft.is_dir() || ft.is_symlink())
        .map(|(name, _)| paths::join(&[root, &name]))
        .filter(|d| exists(&paths::join(&[d, "SKILL.md"])))
        .collect();
    dirs.sort_by(|a, b| locale_compare(basename(a), basename(b)));
    Ok(Some(dirs))
}

fn basename(p: &str) -> &str {
    let t = p.trim_end_matches('/');
    t.rsplit('/').next().unwrap_or(t)
}

pub fn is_valid_slug(slug: &str) -> bool {
    let b = slug.as_bytes();
    !b.is_empty()
        && b.len() <= 100
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
        && !slug.contains("..")
}

fn resolve_destination_slug(target_root: &str, slug: &str, mode: ConflictMode) -> Option<String> {
    if mode == ConflictMode::Overwrite {
        return Some(slug.into());
    }
    if !exists(&paths::join(&[target_root, slug])) {
        return Some(slug.into());
    }
    if mode == ConflictMode::Skip {
        return None;
    }
    let mut counter = 2;
    let mut candidate = format!("{slug}-imported");
    while exists(&paths::join(&[target_root, &candidate])) {
        candidate = format!("{slug}-imported-{counter}");
        counter += 1;
    }
    Some(candidate)
}

/// `fs.cp(src, dst, { recursive, force, dereference: false })`
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let md = fs::symlink_metadata(src)?;
    let ft = md.file_type();
    if ft.is_symlink() {
        // Node fs.cp (verbatimSymlinks=false) rewrites relative link targets to absolute.
        let raw = fs::read_link(src)?;
        let target = if raw.is_absolute() {
            raw
        } else {
            let parent = src.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            std::path::PathBuf::from(paths::resolve(&[&parent, &raw.to_string_lossy()]))
        };
        if fs::symlink_metadata(dst).is_ok() {
            fs::remove_file(dst)?;
        }
        std::os::unix::fs::symlink(target, dst)?;
    } else if ft.is_dir() {
        if !dst.exists() {
            fs::create_dir(dst)?;
            fs::set_permissions(dst, md.permissions())?;
        }
        let mut names: Vec<_> = fs::read_dir(src)?.filter_map(|e| e.ok()).map(|e| e.file_name()).collect();
        names.sort();
        for n in names {
            copy_tree(&src.join(&n), &dst.join(&n))?;
        }
    } else {
        fs::copy(src, dst)?;
    }
    Ok(())
}

/// SkillManager.import for scope=user, mode=copy. Err = reason string.
fn import(source_path: &str, slug: &str, target_root: &str, force: bool) -> Result<(String, Validation), String> {
    let resolved = paths::resolve(&[&expand_home(crate::js_trim(source_path))]);
    let md = fs::metadata(&resolved).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("source_missing: Source path does not exist: {resolved}")
        } else {
            node_err(&e, "stat", &resolved)
        }
    })?;
    if !md.is_dir() {
        return Err(format!("source_not_directory: Source path is not a directory: {resolved}"));
    }
    if !exists(&paths::join(&[&resolved, "SKILL.md"])) {
        return Err(format!("no_skill_md: Source folder does not contain a SKILL.md at the root: {resolved}"));
    }
    let slug = if crate::js_trim(slug).is_empty() { basename(&resolved).to_string() } else { slug.to_string() };
    if !is_valid_slug(&slug) {
        return Err(format!(
            "invalid_slug: Invalid slug \"{slug}\". Allowed: [a-zA-Z0-9][a-zA-Z0-9._-]{{0,99}}, no \"..\"."
        ));
    }
    let target = paths::join(&[target_root, &slug]);
    if paths::resolve(&[&target]) == resolved {
        return Err("self_import: Source and target resolve to the same path; pick a different slug or scope.".into());
    }
    let present = exists(&target);
    if present && !force {
        return Err(format!("conflict: Skill already exists at {target}. Re-run with force=true to overwrite."));
    }
    let validation = validate_from_disk(&resolved);
    if !validation.ok() {
        return Err(format!("validation_failed: {}", validation.hard_fail_messages()));
    }
    if present {
        let _ = if fs::symlink_metadata(&target).map(|m| m.is_dir()).unwrap_or(false) {
            fs::remove_dir_all(&target)
        } else {
            fs::remove_file(&target)
        };
    }
    fs::create_dir_all(target_root).map_err(|e| node_err(&e, "mkdir", target_root))?;
    copy_tree(Path::new(&resolved), Path::new(&target)).map_err(|e| e.to_string())?;
    Ok((target, validation))
}

pub fn migrate(o: &Options) -> Result<Report, Thrown> {
    let pilot_home = paths::resolve(&[&o.pilot_home]);
    let target_root = paths::resolve(&[&pilot_home, "skills"]);
    let sources = build_sources(o);
    let mut items = Vec::new();
    for src in &sources {
        let base = |path: &str, dest: Option<String>, slug: &str, status, reason: String| Item {
            source_kind: src.kind,
            source_label: src.label.clone(),
            source_path: path.to_string(),
            destination_path: dest,
            slug: slug.to_string(),
            status,
            reason,
            validation: None,
        };
        let Some(dirs) = discover(&src.path)? else {
            items.push(base(&src.path, None, "", Status::Skipped, "Source directory does not exist.".into()));
            continue;
        };
        if dirs.is_empty() {
            items.push(base(
                &src.path,
                None,
                "",
                Status::Skipped,
                "No immediate child skill directories with SKILL.md found.".into(),
            ));
            continue;
        }
        for sp in dirs {
            let source_slug = basename(&sp).to_string();
            if !is_valid_slug(&source_slug) {
                items.push(base(&sp, None, &source_slug, Status::Error, format!("Invalid slug \"{source_slug}\".")));
                continue;
            }
            let Some(slug) = resolve_destination_slug(&target_root, &source_slug, o.conflict_mode) else {
                items.push(base(
                    &sp,
                    Some(paths::join(&[&target_root, &source_slug])),
                    &source_slug,
                    Status::Conflict,
                    "Destination skill already exists.".into(),
                ));
                continue;
            };
            let dest = paths::join(&[&target_root, &slug]);
            if !o.execute {
                let resolved = paths::resolve(&[&expand_home(crate::js_trim(&sp))]);
                let v = validate_from_disk(&resolved);
                let mut it = if !v.ok() {
                    base(&sp, Some(dest), &slug, Status::Error, format!("validation_failed: {}", v.hard_fail_messages()))
                } else {
                    let reason =
                        if slug == source_slug { "Would copy skill.".to_string() } else { format!("Would copy skill as {slug}.") };
                    base(&sp, Some(dest), &slug, Status::WouldMigrate, reason)
                };
                it.validation = Some((v, Some(resolved)));
                items.push(it);
                continue;
            }
            match import(&sp, &slug, &target_root, o.conflict_mode == ConflictMode::Overwrite) {
                Ok((skill_path, v)) => {
                    let resolved = paths::resolve(&[&expand_home(crate::js_trim(&sp))]);
                    let mut it = base(&resolved, Some(skill_path), &slug, Status::Migrated, "Copied skill.".into());
                    it.validation = Some((v, None));
                    items.push(it);
                }
                Err(reason) => items.push(base(&sp, Some(dest), &slug, Status::Error, reason)),
            }
        }
    }
    Ok(Report { execute: o.execute, target_root, sources, items })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collation() {
        let mut v = vec!["b", "A", "a", "_x", "1", "a-b", "a_b", "a.b", "B"];
        v.sort_by(|a, b| locale_compare(a, b));
        assert_eq!(v, vec!["_x", "1", "a", "A", "a_b", "a-b", "a.b", "b", "B"]);
    }
}
