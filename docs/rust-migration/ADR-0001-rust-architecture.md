# ADR-0001 (provisional, P03): Rust target architecture and strategy branch

Status: accepted for slice 1; provisional for the rest of the scope.

## Decision
- **Integration branch: S03 (native Rust capability migration)** for CLI/server capabilities. The final `pilotdeck` entry is a native Rust binary (`rust/crates/cli`, bin `pilotdeck`).
- **Overlays: none for slice 1.** `boundary_separable=true` (chat search is a read-only port with a single provider), `atomic_cutover_required=false`, `deterministic_core=false`, `is_compiler=false` → no S04/V01/V02. S04 is expected to be re-evaluated for the gateway/agent-loop group at P12.
- **Transition shim:** the Rust binary forwards not-yet-migrated commands verbatim to the TS CLI when `PILOTDECK_TS_CLI` is set (process-level, logic-free; otherwise exits 2). Logged in `coverage.csv` as a transitional bridge with deletion condition "all CLI commands migrated".
- Web UI / desktop / SDK routes are **not decided** (see inventory unknowns); S01/S02 may apply there.

## Crate layout and dependency direction
```
pilotdeck-cli (bin "pilotdeck")  ──>  pilotdeck-session-search (lib)
```
- `pilotdeck-session-search`: owns nothing mutable; read-only JSONL scan. Modules: `paths` (pilot home / project id / Node path semantics), `session_info` (title resolution), `search` (matcher, snippet, limits), `format` (text output), `js` (JS string/number semantics).
- State owner: transcripts remain owned by the (still TS) session runtime; search never writes.
- Errors: a TS promise rejection maps to `SearchError { message }` → stderr + exit 1, matching `main().catch`.
- Cancellation: none needed (synchronous CLI, no long-running I/O beyond file reads).
- Dependencies (licence-checked, all MIT/Apache-2.0): `serde`, `serde_json`, `regex`, `chrono` (clock only). `Cargo.lock` committed.

## Rejected alternatives
- S01 Node-API binding for the search port: adds a bridge with no long-term value since the target entry is native.
- S02 Wasm: no browser requirement for this capability.

## Consequences / known divergences (tracked in the contract)
- JS `RegExp` vs Rust `regex` syntax (lookaround/backrefs unsupported in Rust; error text differs).
- `serde_json` rejects lone-surrogate escapes and numbers like `1e400` that `JSON.parse` accepts → such lines are skipped instead of parsed.
- Text-mode timestamps reproduce Node 22.23 ICU `en-US` output; other ICU versions may differ.
