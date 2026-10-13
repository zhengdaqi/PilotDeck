//! Pure UI logic ported from `ui/src` (ADR-0003: shared by the Leptos UI). No I/O, no wasm-only deps.
pub mod js;
pub mod workspace_file_mention;
pub mod vectors;
pub mod date_utils;
pub mod external_file_drop;
pub mod ime;
pub mod js_value;
pub mod office_preview;
pub mod chunk_reload;
pub mod desktop_updates;
pub mod frame_batcher;
pub mod upload_form_data;
