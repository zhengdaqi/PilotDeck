//! Pure, terminal-independent TUI core ported from `src/adapters/channel/tui`.
//! Interactive Ink rendering is not ported here (see CAP-TUI contract).
pub mod reducer;
pub mod render;
pub mod sidebar;
pub mod summary;
pub mod tables;
pub mod truncate;
pub mod width;

/// `defaultTuiSessionKey` (TuiChannel.ts).
pub fn default_tui_session_key(project_key: &str) -> String {
    format!("tui:project={project_key}:default")
}
