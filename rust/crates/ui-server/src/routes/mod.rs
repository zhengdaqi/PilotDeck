pub mod workspace_upload;

use crate::AppState;
use axum::routing::post;

pub fn router() -> axum::Router<AppState> {
    axum::Router::new().route("/api/projects/{project_name}/files/upload/check", post(workspace_upload::upload_check))
}
