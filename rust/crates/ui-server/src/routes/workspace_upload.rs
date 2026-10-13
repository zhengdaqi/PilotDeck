//! `createWorkspaceUploadCheckHandler` from `ui/server/services/workspaceFileUpload.js`.
//! (The multipart upload handler itself is pending: it needs the multer-compatible parser.)
use crate::node_fs::NodeError;
use crate::{express_json_body, json, AppState};
use axum::body::Bytes;
use axum::extract::{Path as RoutePath, State};
use axum::http::HeaderMap;
use axum::response::Response;
use pilotdeck_ui_server_core::services::read_upload_limits;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn upload_error(code: &str, message: &str, status: u16) -> NodeError {
    NodeError::new(code, message, Some(status))
}

/// `relativePath(value, allowEmpty)`.
pub fn relative_path(value: &Value, allow_empty: bool) -> Result<String, NodeError> {
    let Value::String(s) = value else { return Err(upload_error("UPLOAD_INVALID_PATH", "Invalid destination path.", 400)) };
    let normalized = s.replace('\\', "/");
    if allow_empty && (normalized.is_empty() || normalized == ".") {
        return Ok(String::new());
    }
    let b = normalized.as_bytes();
    let drive = b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':';
    if normalized.is_empty()
        || normalized.starts_with('/')
        || drive
        || normalized.contains('\0')
        || normalized.split('/').any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(upload_error("UPLOAD_INVALID_PATH", "Destination must be a relative path inside the workspace.", 400));
    }
    Ok(normalized)
}

/// `destinationExists(root, relative)`: lstat each component without following directory symlinks.
fn destination_exists(root: &Path, relative: &str) -> Result<bool, NodeError> {
    let parts: Vec<&str> = relative.split('/').collect();
    let mut current = root.to_path_buf();
    for (i, part) in parts.iter().enumerate() {
        current = node_join(&current, part);
        let info = match std::fs::symlink_metadata(&current) {
            Ok(m) => m,
            Err(e) => {
                let err = NodeError::from_io(&e, "lstat", &current);
                if err.code == "ENOENT" {
                    return Ok(false);
                }
                return Err(err);
            }
        };
        if i == parts.len() - 1 {
            return Ok(true);
        }
        if info.file_type().is_symlink() || !info.is_dir() {
            return Err(upload_error("UPLOAD_INVALID_PATH", "Destination contains a symlink or is not a directory.", 400));
        }
    }
    Ok(false)
}

/// `path.join(a, b)` for a validated single segment (or a validated relative path).
fn node_join(a: &Path, b: &str) -> PathBuf {
    let mut p = a.to_path_buf();
    for seg in b.split('/').filter(|s| !s.is_empty()) {
        p.push(seg);
    }
    p
}

fn check(state: &AppState, project_name: &str, body: &Value) -> Result<Value, (u16, Value)> {
    let inner = || -> Result<Result<(), Vec<String>>, NodeError> {
        let dir = (state.resolve_project)(project_name)?;
        let root = std::fs::canonicalize(&dir).map_err(|e| NodeError::from_io(&e, "realpath", &dir))?;
        let get = |k: &str| body.as_object().and_then(|o| o.get(k));
        let target_raw = get("targetPath").filter(|v| truthy(v)).cloned().unwrap_or_else(|| Value::String(String::new()));
        let target = relative_path(&target_raw, true)?;
        let limits = read_upload_limits(&state.env);
        let max_files = limits["maxFiles"].as_f64().unwrap_or(0.0);
        let input = match get("relativePaths") {
            Some(Value::Array(a)) if !a.is_empty() && (a.len() as f64) <= max_files => a,
            _ => return Err(upload_error("UPLOAD_MANIFEST_INVALID", "Invalid file list.", 400)),
        };
        let names = input.iter().map(|n| relative_path(n, false)).collect::<Result<Vec<_>, _>>()?;
        if names.iter().enumerate().any(|(i, n)| names[..i].contains(n)) {
            return Err(upload_error("UPLOAD_MANIFEST_INVALID", "Duplicate destination paths.", 400));
        }
        let mut conflicts = vec![];
        for name in &names {
            let rel = if target.is_empty() { name.clone() } else { format!("{target}/{name}") };
            if destination_exists(&root, &rel)? {
                conflicts.push(name.clone());
            }
        }
        Ok(if conflicts.is_empty() { Ok(()) } else { Err(conflicts) })
    };
    match inner() {
        Ok(Ok(())) => Ok(json!({ "success": true })),
        Ok(Err(conflicts)) => Err((
            409,
            json!({ "error": { "code": "UPLOAD_FILE_EXISTS", "message": "Destination already exists. Rename the file before uploading." }, "conflicts": conflicts }),
        )),
        Err(e) => Err((e.status.unwrap_or(500), json!({ "error": { "code": e.code, "message": e.message } }))),
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

pub async fn upload_check(State(state): State<AppState>, RoutePath(project_name): RoutePath<String>, headers: HeaderMap, bytes: Bytes) -> Response {
    let body = match express_json_body(headers.get("content-type").and_then(|v| v.to_str().ok()), &bytes) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let result = tokio::task::spawn_blocking(move || check(&state, &project_name, &body)).await.expect("upload check task");
    match result {
        Ok(v) => json(200, &v),
        Err((status, v)) => json(status, &v),
    }
}
