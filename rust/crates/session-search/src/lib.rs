//! Rust port of PilotDeck's read-only chat-history search capability
//! (`pilotdeck chat search`). Behaviour reference: TypeScript sources under
//! `src/session/search/` at the frozen baseline recorded in
//! `docs/rust-migration/`.

pub mod format;
pub mod js;
pub mod paths;
pub mod search;
pub mod session_info;
