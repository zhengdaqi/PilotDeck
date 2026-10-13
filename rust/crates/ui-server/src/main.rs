//! `pilotdeck-ui-server`: the axum host. Until the full ui/server bootstrap (auth, config, projects) is
//! ported, the binary runs the ported routes with a directory-based project resolver:
//! `pilotdeck-ui-server --listen 127.0.0.1:0 --projects-base DIR` (prints `listening <addr>`).
use pilotdeck_ui_server::{node_fs::NodeError, router, AppState};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let listen = arg("--listen").unwrap_or_else(|| "127.0.0.1:3001".into());
    let base = std::path::PathBuf::from(arg("--projects-base").unwrap_or_else(|| ".".into()));
    let state = AppState {
        resolve_project: Arc::new(move |name: &str| -> Result<std::path::PathBuf, NodeError> { Ok(base.join(name)) }),
        env: Arc::new(std::env::vars().collect()),
    };
    let listener = tokio::net::TcpListener::bind(&listen).await.expect("bind");
    println!("listening {}", listener.local_addr().expect("addr"));
    axum::serve(listener, router(state)).await.expect("serve");
}
