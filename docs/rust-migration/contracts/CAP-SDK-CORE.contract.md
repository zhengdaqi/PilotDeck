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

## Slice 25: transport network layer (native and wasm)
- Native (`crates/sdk/src/net.rs`, feature `net`, on by default):
  - A blocking `GatewayTransport` over `TransportCore`, with one tungstenite IO thread per socket.
  - API: `connect` (open → hello timer → retry backoff with jitter), `request` (timer), `stream` (blocking iterator), `on_notification`/`off_notification`, `close`.
- wasm (`sdk-wasm`):
  - `WasmTransportCore` plus `serverInfo`, `helloFrame`, `closeError`, `retryDelayMs` and `mapError` exports.
  - `npm/transport.js` is the WebSocket host (browser, or the Node 22 global) with the same API, timers and Error shape as TS.
- Live parity (`rust/parity/sdk/net_transport_parity.mjs`):
  - Real ws@8 servers drive 27 scenarios: hello variants, 4003/auth_failed/protocol_mismatch, drop before hello, refused port, retries with connection counts, request errors/timeouts, invalid frames, server close mid-stream, stream seq rules, notifications, client close, invalid URLs.
  - The TS dist, the native Rust probe and the wasm wrapper are identical: 27/27 three ways, stable over 3 runs.
  - Mutations caught: retry-on-auth, close-error mapping, close code on IO error, notification delivery.
  - Two mutations survive by design: the default connectTimeout (scenarios always pass it) and the close check in `open()` (unreachable sequentially).
- Intended difference: a socket that TS drops after a failure is left open (leaked); Rust/wasm closes it.
- `wss://` still needs TLS (unchanged gap).

## Slice 26: hook-server.ts → `crates/sdk/src/hook_server.rs` (feature `net`)
- **Ported:** `HostedHookServer` (constructor checks, start/close, config shape) and `isLoopbackHttpUrl`.
  - HTTP/1.1 server, one thread per connection.
  - Request handling: routing via WHATWG URL resolution (url crate), Bearer token check, `Number()` matcher selector, 1MB body limit (connection dropped like TS `request.destroy()`).
  - Hook logic: camelCase→snake_case input, the input validations, callback timeout and abort, async/sync merging rules, `defer`→`passthrough`, and error JSON with `code` for PilotDeckError.
- **Callbacks** are Rust closures `(input, toolUseId, {signal, asyncHookId}) -> Result<Option<Value>, HookError>`.
- **Live parity:** `rust/parity/sdk/hook_server_parity.mjs`.
  - Inputs: a shared callback catalogue, random configs and start options, raw HTTP requests (methods, odd targets, auth variants, chunked bodies, broken requests, oversized bodies), plus 19 fixed merge scripts.
  - The TS dist and the Rust probe are compared on status, content-type and body.
  - Results: 1266/1266, 1374/1374 and 1374/1374 over 3 seeds × 150 scripts. Mutations: 10 run, 10 killed (after adding the fixed scripts).
- **SRC-DEFECT-016 fixed:** a request target that `new URL` rejects crashes the TS host process; Rust answers 404.
- **Pending:**
  - Abort on client disconnect mid-callback: TS aborts the callback signal at once. Rust notices the disconnect only on write; `close()` still aborts.
  - JSON bodies containing lone-surrogate escapes: serde rejects them, so Rust answers 500 "not valid JSON" where TS accepts the body.

## Slice 29: client.ts validateOptions
`crates/sdk/src/options.rs` ports `validateOptions` with normalizeSandbox (validation part), validateDeferredTools,
normalizeSystemPrompt, isPilotDeckJsonSchema, validateAgentMcpServers/Transport, supportsUserDialogKind and
isAbsolutePath. Input is a JS-shaped `Js` value (functions, undefined members and non-finite numbers included), so
the wasm/JS host and loosely typed native config are validated the same way. Check order, messages, codes and the
V8 TypeErrors for wrong member types are kept. Oracle: rust/parity/sdk/gen_validate_options.mjs → 9860/9860;
evidence sdk-validate-options.txt. Pending in client.ts: sdkSessionConfig/query init sequence, Query/stream
control, MCP preparation, resource-client methods (validateOptions is exported to wasm since slice 33).

## Slice 33: validateOptions in the wasm npm wrapper
`npm/index.js` exports `validateOptions(options)` and `encodeOptions`. The encoder marks functions, undefined
members and non-finite numbers, and the wasm `validateOptionsJson` runs `options.rs`. Errors are rethrown as
`PilotDeckError` (code/message) or `TypeError`. Check: `node rust/sdk-wasm/check_validate_options.mjs` → 9860/9860.
Limitation (recorded): the encoder walks own enumerable properties, while TS also reads inherited ones through
`options.x` (prototype-carried options are not supported by the wrapper).
