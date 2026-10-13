//! Transport-agnostic core of the PilotDeck SDK (port of `packages/sdk/src`).
//! Hosts: native Rust, and wasm via `rust/sdk-wasm` (npm wrapper).
#![allow(clippy::result_large_err, clippy::should_implement_trait)] // SdkError carries the full JS error shape; `next()` mirrors TS method names and is not an iterator.
pub mod session_store;
#[cfg(feature = "fs")]
pub mod file_store;
pub mod js;
pub mod options;
pub mod snapshot;
pub mod transport_core;
#[cfg(feature = "net")]
pub mod net;
#[cfg(feature = "net")]
pub mod hook_server;
