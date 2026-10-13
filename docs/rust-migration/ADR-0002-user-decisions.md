# ADR-0002: Frozen user decisions on product-wide routes (2026-10-13)

Status: **FROZEN** (decided by the user Da on 2026-10-13). Changing these requires a new ADR.

| # | Topic | Decision | Consequence for scope axes |
|---|---|---|---|
| D1 | Web UI (`ui/src`, `ui/shared`) | **Replace React with a Rust → Wasm UI.** | `js_runtime` for the UI moves to `forbidden` for first-party logic (only wasm-bindgen glue allowed). Strategy for CAP-WEB-UI: S02 (Rust Wasm) with shared vectors + real browser host verification. Requires a Rust UI framework choice (ADR-0003, pending probe: Leptos / Dioxus / Yew). |
| D2 | Desktop (`apps/desktop`, Electron) | **Tauri**: Rust backend reusing the migrated crates, Wasm UI from D1. | CAP-DESKTOP: S03 native host + D1 UI. Packaging/signing per platform needs real macOS/Windows machines and signing credentials (future blocker). |
| D3 | SDK (`packages/sdk`, `@pilotdeck/sdk`) | **FINAL**: publish primarily as a Rust crate; additionally publish a wasm-bindgen wrapper to npm as `@pilotdeck/sdk` for JS compatibility. | CAP-SDK: S03 for the crate + S02 for the npm wrapper; the public JS API surface of `@pilotdeck/sdk` (alpha) is the compatibility contract for the wrapper. Actual npm/crates.io publication is NOT authorized by this ADR. |
| D4 | JS plugins (`src/extension/plugins`, builtin JS plugins) | **Keep an embedded JS runtime host for plugins.** | `js_runtime=declared_hosts` (plugin host only). Plugin host engine choice (e.g. `rquickjs`/`boa`/`deno_core`) to be probed against the existing builtin plugins (browser-use, funasr, windows-skills). First-party PilotDeck logic must not live in the host. |

resolved-config.json `scope_axes` is updated accordingly.
