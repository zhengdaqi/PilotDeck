# CAP-GATEWAY-SERVER: Rust Gateway server edge (sub-slice of CAP-GATEWAY / CAP-CLI-SERVER)

Source of truth:
- `src/gateway/server/{GatewayServer,GatewayWsConnection,websocket,authToken,staticAssets}.ts`
- `src/composition/public{Approval,Host}HttpTransport.ts`, ported only in their unbound configuration
- `src/adapters/web/httpRouter.ts`: auth and projects routes
- protocol version `1.2`

Oracle: the real `startGatewayServer` from the dist build. It is driven by the same deterministic fake `Gateway`
(`rust/parity/gateway/fake_gateway.mjs`) that the Rust server reaches through the JSON-lines bridge (`bridge.mjs`).

Rust: crate `rust/crates/gateway-server`, binary `pilotdeck-gateway-server`.

Ported (behavior-compatible):
- HTTP routes: `/health`, `/auth/local-token` (loopback only), 404 fallback, static assets (path normalization, root
  containment, `index.html` fallback, content types). Request-target pathnames follow WHATWG URL parsing: dot segments
  including `%2e`, backslashes, absolute and authority forms, percent-encoding.
- Public approval and host transports, unbound: 401/405/503 and 401/405/501.
- `/api/web`: Bearer auth regex semantics, `GET /projects`, `GET /projects/:key[/]` with `decodeURIComponent` inside the
  try, `not_found` 404.
- WebSocket: upgrade (`Sec-WebSocket-Accept`), client frame parsing (masking, 16- and 64-bit lengths, no fragmentation, as
  in TS), ping/pong, close frames, upgrade head bytes ignored (as in TS), 101 handshake bytes.
- GatewayWsConnection:
  - hello validation and close codes 4001/4002/4003
  - `hello_ok` with `describeServer` and `interactionBinding` (random UUID plus a process-global generation)
  - all 100+ wire methods, including optional, required, capability and skill dispatch and every fallback result
  - error classification: SkillValidationError, SkillManagerError, DialogGatewayError, or structured `code`, including
    the sop `httpStatus` detail rule and the `gateway_request_failed` fallback
  - submit_turn event streaming: seq, terminal-event selection, `result_unknown` fallback, `permission_changed` notifications
  - permission_decide SDK control-connection rule and post-call notification
  - close cleanup: `disconnectInteraction` and `abortTurn` unless `preserveTurn`
  - concurrent request handling, one thread per authed request, mirroring un-awaited `handleRequest` promises
- `ensureGatewayAuthToken`: `<pilotHome>/server-token`, base64url of 32 random bytes, dir mode 0700, file mode 0600.

Bounded fixes, each a documented divergence pending user decision and never counted as parity:
SRC-DEFECT-004 (malformed frame crash), SRC-DEFECT-005 (unhandled rejection crash), SRC-DEFECT-006 (half-open close cleanup).

- `/api/web/projects/:key/files/{tree,read,write}` and `ProjectFileService`:
  - lexical and realpath boundary checks
  - tree typing and sorting (`localeCompare` on names verified against Node, 20000/20000 cases, Latin and accent domain)
  - text/base64 reads with the 1 MiB cap
  - recursive mkdir and writes; lenient Node base64 decoding; WHATWG `searchParams`; Content-Length and chunked bodies
  - Node `fs` error messages
  - The root is `projectKey`, because the TS server entry binds no `resolveProject`.
  - Covered by 71 file-route parity cases on real temp trees: symlinks in and out, a dangling link, a fifo, bad UTF-8, a
    BOM, unicode names, big files.
  - SRC-DEFECT-007 (write follows symlinks out of the root) is replicated as-is, pending user decision.

- Public module-host transports in their bound configuration (`publicApprovalHttpTransport.ts`,
  `publicHostHttpTransport.ts`).
  - Rust owns the HTTP side:
    - auth and method checks
    - size limits (64 KiB / 32 MiB)
    - JSON and shape validation
    - the allowed-key and scope-override rules
    - binding subject checks
    - the wire-operation filter
    - status validation
    - the header whitelist
    - NDJSON/SSE chunked streaming, with mid-stream destroy
    - error mapping (409 conflict, 502 defaults)
  - The bound `PublicApprovalBridge` and provider resolver stay in the Gateway host. They are reached through
    `GatewayBackend` methods `__approvals.*` and `__host.*` (resolve, call, and release, with handles so the resolver
    runs once per request).
  - Parity: 58 public cases against the real TS transports with the same fake bridge and provider (`fake_public.mjs`),
    including the effects log.
  - Approximations:
    - Client-disconnect abort is not propagated to `provider.call` (the host passes a never-aborted signal).
    - A stream whose head the server rejects is still drained by the host.
  - SRC-DEFECT-008 (no response for oversized approval input) is replicated, pending decision.

PENDING. Not dropped; each fails loudly, never fakes success:
- `/feishu/webhook` option.
- The `Gateway` implementation itself (InProcessGateway: sessions, agent loop host, tools, storage, cron, MCP...). The
  Rust edge reaches a Gateway only through `GatewayBackend`. Today that is `ProcessGateway` (JSON-lines host process).
  Production wiring of a TS bridge host (`pilotdeck server` -> Rust edge) is not done.

Known race-only differences (not observable with a client that waits for responses):
- TS re-enters hello handling for frames that arrive before `describeServer` resolves. Rust handles hello inline and
  in order.
- HTTP transport headers: Rust always answers with `Content-Length` and `Connection: close`. Node uses keep-alive and,
  for static files, chunked encoding. Status, content-type and body are identical.
- Lone UTF-16 surrogates in JSON strings: the same known gap as the rest of the Rust JSON layer.

Production: NOT switched. `pilotdeck server` still starts the TS GatewayServer.

Evidence: `evidence/gateway-server-parity.txt`. 710/710 parity cases pass:
- `full` + static, `full`, and `minimal` gateway variants
- HTTP and WS cases
- runs repeated 3x

Documented divergences are listed at the end of that file.
