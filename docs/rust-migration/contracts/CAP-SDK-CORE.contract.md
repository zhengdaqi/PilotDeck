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

## Slice 2 (slice 18): FileSessionStore
`rust/crates/sdk/src/{file_store,snapshot}.rs` (feature `fs`, on by default; the wasm build turns it off).
The on-disk format is byte-identical to TS:
- same paths: base64url `project-`/`session-`/`subpaths` layout;
- same bytes: `JSON.stringify(snapshot)+"\n"`;
- same temp-file-then-rename write.

This was checked by comparing the whole tree after each script. The full key/snapshot validation and error
messages are ported, as are `mergeEntries` (dedupe on string uuids from existing entries) and `nextMtime`.
`listSubkeys` uses the ICU-verified `locale_compare`; vector subpaths are ASCII.
Oracle: `rust/parity/sdk/gen_file_store.mjs`, 60 scripts, 2460 checks, 0 mismatches. Mutation checks: 60 and 82.
TS nondeterminism: when several mirror files are bad, `Promise.all` rejects with whichever read fails first.
The oracle records every error TS could give (`__errorOneOf`, 289 ops); Rust returns the first in sorted readdir
order, which is always one of them.
Still to do: `createSessionStoreFromAdapter` (host adapter), the `importSessionToStore` path through adapter stores,
client.ts, transport.ts, hook-server.
- transport.ts: sans-IO `TransportCore` (handleMessage, close mapping, client close, send-when-disconnected, mapError, normalizeGatewayCode, retry delay, AsyncEventQueue). Oracle `gen_transport.mjs` covers 600 random scripts through a mock WebSocket. SRC-DEFECT-015 is fixed. Pending: async IO adapters (native tokio/tungstenite, browser WebSocket via sdk-wasm), connect/hello timers, retry loop.
