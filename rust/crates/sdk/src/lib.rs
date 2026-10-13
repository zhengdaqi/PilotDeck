//! Transport-agnostic core of the PilotDeck SDK (port of `packages/sdk/src`).
//! Hosts: native Rust, and wasm via `rust/sdk-wasm` (npm wrapper).
pub mod session_store;
#[cfg(feature = "fs")]
pub mod file_store;
pub mod js;
pub mod snapshot;
