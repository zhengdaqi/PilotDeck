# CAP-SDK-CORE contract — SDK Rust crate + wasm npm wrapper, slice 1 (session store)

Source: `packages/sdk/src/session-store.ts`: `InMemorySessionStore`, `foldSessionSummary`, `summaryToSessionInfo`, `importSessionToStore`.
Target: `rust/crates/sdk` (pure; the host passes in the time as `now`). `rust/sdk-wasm` builds it with wasm-bindgen, and
`rust/sdk-wasm/npm` (`@pilotdeck/sdk-wasm`, private, never published) gives it the same async JS API as the TS SDK.
JS behaviour kept:
- `Map` insertion order: set keeps a key's position; delete then set moves it to the end.
- uuid dedupe only for truthy uuids, by SameValueZero (7 and "7" are different keys; object uuids never match).
- `Math.max(Date.now(), last+1)` mtime.
- stable sort by mtime, descending.
- `""` text does not set a prompt, but still stops the fallback lookup.
- metadata `null` values are copied.
Oracle: `rust/parity/sdk/gen_session_store.mjs` runs the real TS with a scripted `Date.now` and writes 2440 ops (40 stores).
`rust/parity/sdk/drive.mjs` replays the same script against the TS source and the npm wrapper; the Rust test replays it natively.
Results: native 2440/2440, wasm npm wrapper (Node 22) 2440/2440. Negative control fails both (1 mismatch each).
Not yet ported (pending): FileSessionStore and createSessionStoreFromAdapter (fs/adapters), client.ts (query/run/dialogs/MCP),
transport.ts (WebSocket), hook-server, the dialogs. The transport plan is described in probes D3 (web-sys WebSocket shown working in the sdk-wasm probe).
