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

## Batch 2 (slice 16)
Sources: `ui/src/utils/officePreviewStatus.ts` (normalize, signature, signatureFromConfig), `ime.ts` (both),
`externalFileDrop.ts` (isExternalFileDrag, resolveExternalFileDropTargetPath), `dateUtils.ts` (formatTimeAgo).
Targets: `web-ui-core::{office_preview, ime, external_file_drop, date_utils, js_value}`.
Host boundary: `formatTimeAgo` gets `Date.parse` and `toLocaleDateString` from the host. In the browser, `js_sys::Date`
is checked against the value TS recorded for all 400 inputs, in both Chrome and WebKitGTK.
Oracle: `rust/parity/web_ui/gen_batch2.mjs`, 8119 checks. Native, Chrome and Tauri/WebKitGTK all show 0 mismatches.
Mutation check: 42 mismatches.
Generator note: two imports are swapped in temporary copies, because strip-types cannot run them: the i18next
`TFunction` type, and React `useEffect`, which is only used by the hook that is not under test. No logic is changed.
The Leptos app now has an IME-aware Enter-to-send composer, a time-ago label and Office service normalization.
Chrome DOM checks: Enter sends; Enter during composition does not; "3 hours ago"; " LibreOffice " -> libreoffice.
Not yet ported: `readExternalFileDropTarget` and `useRejectExternalFileDropOutsideTargets` (DOM; they go with the
file-tree component), `readOfficePreviewStatus` (fetch; with the api client), and the remaining UI.

## Batch 3 (slice 32)
Sources: `ui/src/utils/reloadOnChunkError.ts` (isDynamicImportLoadError, reloadOnceForDynamicImportError,
clearDynamicImportReloadMarker), `frameBatcher.ts`, `uploadFormData.ts` (postFormDataWithProgress callback logic:
progress math, onload, Authorization, response/token), `desktopUpdates.ts`.
Targets: `web-ui-core::{chunk_reload, frame_batcher, upload_form_data, desktop_updates}`. Host boundary: sessionStorage +
location.reload (`ReloadWindow`), requestAnimationFrame (`FrameHost`), the XMLHttpRequest itself.
Oracle: `rust/parity/web_ui/gen_batch3.mjs`, 3506 checks (fake storage/rAF/XHR logs). Native, Chrome 154 and
Tauri/WebKitGTK all show 0 mismatches (self-test total 21036). web-ui-core serde_json now enables preserve_order
explicitly so response bodies keep JS key order in the standalone web-ui build too.
Not yet ported: registerDynamicImportReloadHandler (window listeners; goes with the app shell), clipboard.ts
(DOM/Clipboard API), restartUi.ts, api.js, and the React components.
