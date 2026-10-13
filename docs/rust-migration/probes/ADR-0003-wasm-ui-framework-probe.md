# Probe for ADR-0003 (Wasm UI framework) — data only, NO decision

ADR-0002 D1 froze "replace React with a Rust → Wasm UI" and left the framework to ADR-0003. This probe produces
comparable evidence. **The framework choice is the user's decision; nothing here selects one.**

Setup: `rust/probes/wasm-ui` (own Cargo workspace, not part of the product build). Each app renders the same
view: a session list grouped by project, with a button that regroups it by status. The view model in `probe-shared`
calls the migrated `pilotdeck-tui-core::sidebar::group_sessions`, so all three apps run first-party Rust logic
that the TUI also uses. Toolchain: rustup 1.85.1 + `wasm32-unknown-unknown` (installed under
`/workspace/.tools`, outside the product), `wasm-bindgen-cli 0.2.129`, release profile `opt-level=z`, LTO.
Host check: `verify_browser.mjs` loads each app in real headless Chrome over CDP, reads the DOM, clicks the
button and reads the DOM again.

| Framework (version) | wasm bytes | gzip -9 | cold release build (s) | Chrome mount | click → regroup |
|---|---|---|---|---|---|
| Leptos 0.7.8 (csr) | 169,045 | 60,963 | 45 | yes (~56 ms) | yes |
| Dioxus 0.6.3 (web) | 456,610 | 165,016 | 27 | yes (~66 ms) | yes |
| Yew 0.21.0 (csr) | 229,370 | 85,680 | 31 | yes (~56 ms) | yes |

Evidence: `evidence/wasm-ui-probe-browser.json` (DOM before/after per app) and
`evidence/wasm-ui-probe-sizes.txt`. Cold build times are wall-clock values from a separate target directory per
app on a shared, loaded box, so treat them as rough.

Observations (facts, not a recommendation):
- All three compile on Rust 1.85.1 and run in a real browser host with shared first-party crates.
- Newer majors (Leptos 0.8, Dioxus 0.7, Yew 0.23) need a newer rustc than the repo's 1.85 and were not probed.
- Dioxus 0.6 web needs `#main` in the host page and the `launch` feature. Its bundle here is about 2.7× Leptos's.
- Leptos uses fine-grained signals. Yew uses a React-like component, hooks and virtual-DOM model, which is the
  closest to the current React code's structure. Dioxus also offers a desktop renderer (relevant to D2 vs. Tauri).
- Not measured: SSR/hydration, router, i18n, the size of a real screen (the current `ui/src`), accessibility, or
  the equivalents of `ui/shared` test tooling.

Needed from the user: choose the framework (ADR-0003). D2 (Tauri) hosts the D1 UI, so it waits on the same choice.
