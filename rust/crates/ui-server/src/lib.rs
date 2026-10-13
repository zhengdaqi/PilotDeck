//! axum host for the ui/server HTTP surface (port of `ui/server/index.js` and `ui/server/routes/*`).
//!
//! Routes are added slice by slice; each one is checked against the Express handler it replaces by a
//! parity harness under `rust/parity/ui_server/`. Middleware that is not ported yet (authentication,
//! project filesystem guards) is the host's responsibility and is listed as pending in the contract.
pub mod node_fs;
pub mod routes;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

/// `extractProjectDirectory(projectName)`: maps a route project name to its directory.
pub type ProjectResolver = Arc<dyn Fn(&str) -> Result<PathBuf, node_fs::NodeError> + Send + Sync>;

#[derive(Clone)]
pub struct AppState {
    pub resolve_project: ProjectResolver,
    /// Environment used for `readUploadLimits` and other env-driven settings.
    pub env: Arc<std::collections::HashMap<String, String>>,
}

/// `res.status(status).json(body)`: Express writes compact JSON with this content type.
pub fn json(status: u16, body: &Value) -> Response {
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        [("content-type", "application/json; charset=utf-8")],
        body.to_string(),
    )
        .into_response()
}

/// Express 4 `express.json({ type: contentType.includes('json') && !multipart })`: a non-JSON request
/// leaves `req.body` as `{}`; JSON bodies parse strictly (objects and arrays only).
pub fn express_json_body(content_type: Option<&str>, bytes: &[u8]) -> Result<Value, Response> {
    let ct = content_type.unwrap_or("");
    if ct.contains("multipart/form-data") || !ct.contains("json") || bytes.is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    match serde_json::from_slice::<Value>(bytes) {
        Ok(v @ (Value::Object(_) | Value::Array(_))) => Ok(v),
        // body-parser rejects other JSON and malformed JSON with a 400 handled by Express' error handler.
        _ => Err((StatusCode::BAD_REQUEST, "Bad Request").into_response()),
    }
}

pub fn router(state: AppState) -> axum::Router {
    routes::router().with_state(state)
}
