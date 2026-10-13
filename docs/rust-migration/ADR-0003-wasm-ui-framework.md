# ADR-0003: Wasm UI framework = Leptos (FINAL, 2026-10-13)

Status: **FINAL** (decided by the user Da on 2026-10-13, after `probes/ADR-0003-wasm-ui-framework-probe.md`).

Decision: the Rust → Wasm Web UI (ADR-0002 D1) and the Tauri desktop UI (D2) use **Leptos** (0.7.x on the repo
toolchain Rust 1.85; client-side rendering). Dioxus and Yew are not used.

Consequences:
- CAP-WEB-UI is migrated in slices; each slice states its parity oracle (shared vectors against the TS/React
  implementation and a real-browser host check).
- First-party view logic lives in plain Rust crates shared with the TUI/CLI where possible; Leptos crates hold
  only components and wasm-bindgen glue.
