//! Port of `FileSessionStore` (packages/sdk/src/session-store.ts). The on-disk layout and bytes are the same as TS:
//! `<root>/project-<b64url>/session-<b64url>/{session.json | subpaths/<b64url>.json}`, holding `JSON.stringify(snapshot) + "\n"`.
//! Writes go to a temp file first and are then renamed into place. Ops are sequential (`&mut self`), so the TS
//! per-path promise chain is implicit.
use crate::session_store::{fold_session_summary, SessionKey, SessionSummary};
use crate::snapshot::*;
use base64::Engine;
use serde_json::{json, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub struct FileSessionStore {
    root: PathBuf,
    tmp_counter: u64,
}

fn enc(s: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s.as_bytes())
}

fn disp(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Node `readdir` order on unix: libuv `scandir` sorts entries by name (strcmp).
fn sorted_entries(dir: &Path, want_dir: bool) -> Result<Vec<String>, StoreError> {
    let rd = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(_) => return Err(StoreError::invalid(format!("Unable to list SessionStore directory {}.", disp(dir)))),
    };
    let mut out = vec![];
    for d in rd.flatten() {
        let ft = d.file_type().ok();
        let ok = if want_dir { ft.is_some_and(|t| t.is_dir()) } else { ft.is_some_and(|t| t.is_file()) };
        if ok {
            out.push(d.file_name().to_string_lossy().into_owned());
        }
    }
    out.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    Ok(out)
}

impl FileSessionStore {
    pub fn new(root_dir: &str) -> Result<Self, StoreError> {
        if crate::js::trim(root_dir).is_empty() {
            return Err(StoreError::invalid("FileSessionStore requires a non-empty rootDir."));
        }
        Ok(Self { root: PathBuf::from(root_dir), tmp_counter: 0 })
    }

    fn project_dir(&self, project_key: &Value) -> Result<PathBuf, StoreError> {
        let k = normalize_session_key(&json!({"projectKey": project_key, "sessionId": "placeholder"}))?;
        Ok(self.root.join(format!("project-{}", enc(k["projectKey"].as_str().unwrap()))))
    }
    fn session_dir(&self, key: &Value) -> Result<PathBuf, StoreError> {
        let k = normalize_session_key(key)?;
        Ok(self.project_dir(&k["projectKey"])?.join(format!("session-{}", enc(k["sessionId"].as_str().unwrap()))))
    }
    fn path_for(&self, key: &Value) -> Result<PathBuf, StoreError> {
        let k = normalize_session_key(key)?;
        let dir = self.session_dir(&k)?;
        Ok(match k.get("subpath").and_then(|s| s.as_str()) {
            Some(sp) => dir.join("subpaths").join(format!("{}.json", enc(sp))),
            None => dir.join("session.json"),
        })
    }

    fn read_at(&self, path: &Path, expected: Option<&Value>) -> Result<Option<Value>, StoreError> {
        let raw = match fs::read(path) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(StoreError::invalid(format!("Unable to read SessionStore mirror at {}.", disp(path)))),
        };
        let parsed: Value = serde_json::from_str(&raw)
            .map_err(|_| StoreError::invalid(format!("SessionStore mirror at {} is not valid JSON.", disp(path))))?;
        let snap = validate_snapshot(&parsed)?;
        if let Some(exp) = expected {
            if key_of(&snap["key"]) != key_of(&normalize_session_key(exp)?) {
                return Err(StoreError::invalid(format!("SessionStore mirror at {} does not match its requested key.", disp(path))));
            }
        }
        Ok(Some(snap))
    }

    fn write_at(&mut self, path: &Path, snapshot: &Value) -> Result<(), StoreError> {
        let fail = || StoreError::invalid(format!("Unable to write SessionStore mirror at {}.", disp(path)));
        fs::create_dir_all(path.parent().unwrap()).map_err(|_| fail())?;
        self.tmp_counter += 1;
        let tmp = PathBuf::from(format!("{}.{}.{}.tmp", disp(path), std::process::id(), self.tmp_counter));
        let text = format!("{}\n", serde_json::to_string(snapshot).expect("json"));
        let r = fs::write(&tmp, text).and_then(|_| fs::rename(&tmp, path));
        if r.is_err() {
            let _ = fs::remove_file(&tmp);
            return Err(fail());
        }
        Ok(())
    }

    pub fn load(&self, key: &Value) -> Result<Option<Vec<Value>>, StoreError> {
        let p = self.path_for(key)?;
        Ok(self.read_at(&p, Some(key))?.map(|s| s["entries"].as_array().cloned().unwrap_or_default()))
    }

    pub fn append(&mut self, key: &Value, entries: &[Value], now: f64) -> Result<(), StoreError> {
        if entries.is_empty() {
            return Ok(());
        }
        let p = self.path_for(key)?;
        let current = self.read_at(&p, Some(key))?;
        let existing = current.as_ref().and_then(|c| c["entries"].as_array().cloned()).unwrap_or_default();
        let (merged, _, _) = merge_entries(&existing, entries)?;
        let mtime = next_mtime(current.as_ref().and_then(|c| c["mtime"].as_f64()), now);
        let snap = json!({"schemaVersion": 1, "key": normalize_session_key(key)?, "mtime": mtime_json(mtime), "entries": merged});
        self.write_at(&p, &snap)
    }

    pub fn list_sessions(&self, project_key: &Value) -> Result<Vec<(String, Value)>, StoreError> {
        let dir = self.project_dir(project_key)?;
        let mut out = vec![];
        for c in sorted_entries(&dir, true)? {
            if let Some(s) = self.read_at(&dir.join(c).join("session.json"), None)? {
                if &s["key"]["projectKey"] == project_key && s["key"].get("subpath").is_none() {
                    out.push((s["key"]["sessionId"].as_str().unwrap().to_string(), s["mtime"].clone()));
                }
            }
        }
        out.sort_by(|a, b| b.1.as_f64().unwrap_or(0.0).partial_cmp(&a.1.as_f64().unwrap_or(0.0)).unwrap());
        Ok(out)
    }

    pub fn list_session_summaries(&self, project_key: &Value) -> Result<Vec<SessionSummary>, StoreError> {
        let mut out = vec![];
        for (sid, _) in self.list_sessions(project_key)? {
            let key = json!({"projectKey": project_key, "sessionId": sid});
            if let Some(s) = self.read_at(&self.path_for(&key)?, Some(&key))? {
                let k = SessionKey::from_json(&s["key"]);
                let entries = s["entries"].as_array().cloned().unwrap_or_default();
                out.push(fold_session_summary(None, &k, &entries, s["mtime"].as_i64()));
            }
        }
        out.sort_by(|a, b| b.mtime.cmp(&a.mtime));
        Ok(out)
    }

    pub fn list_subkeys(&self, key: &Value) -> Result<Vec<String>, StoreError> {
        let dir = self.session_dir(key)?.join("subpaths");
        let mut out = vec![];
        for f in sorted_entries(&dir, false)?.into_iter().filter(|f| f.ends_with(".json")) {
            if let Some(s) = self.read_at(&dir.join(f), None)? {
                if s["key"]["projectKey"] == key["projectKey"] && s["key"]["sessionId"] == key["sessionId"] {
                    if let Some(sp) = s["key"]["subpath"].as_str() {
                        out.push(sp.to_string());
                    }
                }
            }
        }
        out.sort_by(|a, b| pilotdeck_agent_loop_sidecar::cacheplan::locale_compare(a, b));
        Ok(out)
    }

    pub fn delete(&mut self, key: &Value) -> Result<(), StoreError> {
        let p = self.path_for(key)?;
        match fs::remove_file(&p) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StoreError::Raw(e.to_string())),
        }
    }

    pub fn export_session(&self, key: &Value) -> Result<Option<Value>, StoreError> {
        let p = self.path_for(key)?;
        self.read_at(&p, Some(key))
    }

    /// `importSession(snapshot, { mode })` -> `{ mode, imported, skipped, mtime }`.
    pub fn import_session(&mut self, snapshot: &Value, mode: Option<&str>, now: f64) -> Result<Value, StoreError> {
        let incoming = validate_snapshot(snapshot)?;
        let mode = mode.unwrap_or("reject");
        let p = self.path_for(&incoming["key"])?;
        let current = self.read_at(&p, Some(&incoming["key"]))?;
        let cur_entries = current.as_ref().and_then(|c| c["entries"].as_array().cloned()).unwrap_or_default();
        if mode == "reject" && !cur_entries.is_empty() {
            return Err(StoreError::Store {
                code: "SESSION_STORE_CONFLICT",
                message: format!(
                    "A local SessionStore mirror already exists for {}; choose append or replace explicitly.",
                    incoming["key"]["sessionId"].as_str().unwrap()
                ),
            });
        }
        let base = if mode == "replace" { vec![] } else { cur_entries };
        let (entries, imported, skipped) = merge_entries(&base, incoming["entries"].as_array().unwrap())?;
        let mtime = incoming["mtime"].as_f64().unwrap().max(next_mtime(current.as_ref().and_then(|c| c["mtime"].as_f64()), now));
        let snap = json!({"schemaVersion": 1, "key": incoming["key"], "mtime": mtime_json(mtime), "entries": entries});
        self.write_at(&p, &snap)?;
        Ok(json!({"mode": mode, "imported": imported, "skipped": skipped, "mtime": mtime_json(mtime)}))
    }
}
