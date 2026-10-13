# CAP-WEB-UI contract (Leptos Wasm UI, ADR-0003) — slice 1

Source: `ui/src/utils/workspaceFileMention.ts` (all exports). Target: `rust/crates/web-ui-core`
(pure, host-agnostic), rendered by `rust/web-ui` (Leptos 0.7 CSR) and hosted natively by `rust/desktop-tauri`.

Behaviour: identical outputs for every exported function, with JS semantics kept: UTF-16 indices
for cursor and slicing, JS `trim`/`\s` whitespace set, `toLowerCase` identity keys.
Oracle: `rust/parity/web_ui/gen_vectors.mjs` runs the real TS (Node 22 `--experimental-strip-types`)
-> 9011 vectors (BMP only: astral splits cannot be represented in JSON/Rust strings; recorded limitation).
The same Rust runner (`vectors::run_workspace_file_mention`) executes them in three hosts:
- native `cargo test` — 9011/9011
- wasm32 in headless Chrome 154 (`rust/web-ui/verify_browser.mjs`) — 9011/9011, plus a DOM check (click, then click again -> "already present")
- wasm32 in Tauri v2 / WebKitGTK 2.54 under xvfb — 9011/9011
Negative control: changing one expected value in the fixture makes the test fail (1 mismatch).
Remaining UI: the rest of ui/src (~150k LOC), React components -> Leptos components, slice by slice.
