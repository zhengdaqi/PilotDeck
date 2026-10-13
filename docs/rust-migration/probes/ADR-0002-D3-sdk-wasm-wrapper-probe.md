# ADR-0002 D3 probe: npm wasm-bindgen wrapper for `@pilotdeck/sdk` (no publishing)

Question: can the JS-compatible `@pilotdeck/sdk` be a wasm-bindgen wrapper over Rust code, given that the SDK's
core is a Gateway WebSocket client?

Probe: `rust/probes/sdk-wasm` (cdylib, own workspace). `wasm-bindgen --target nodejs` output is loaded by
`check.mjs` in Node 22.23.3 and compared with the TS SDK build (`packages/sdk/dist`). It exports:
- `hookEvents()` and `filterEscalatingDefaultMode()`: pure exports, compared with TS `HOOK_EVENTS` and
  `filterEscalatingDefaultMode`. Results: equal, and the result is a deep clone.
- `gatewayHello(url, token)`: the Gateway WS `hello` handshake through the **host's** `WebSocket` (`web-sys`),
  run against the TS gateway server (`rust/parity/gateway/ts_server.mjs`). Result: `hello_ok` received; a wrong
  token is rejected.
Result line: `{"hookEvents":true,"filterEscalatingDefaultMode":true,"gatewayHello":true,"badTokenRejected":true}`.
The wasm is 75,834 bytes.

Findings:
- The existing `pilotdeck-gateway-client` transport (blocking `TcpStream` + tungstenite) cannot compile for
  wasm32. The wrapper needs a host-WebSocket transport. The probe shows that path works in Node 22 (global
  `WebSocket`), and it works in browsers the same way.
- The Rust crate (D3 primary) and the wrapper should therefore share a transport-agnostic core: protocol
  framing, stream sequencing, and message mapping. Each would add its own I/O: native tungstenite for the crate,
  `web-sys` for the wrapper.
- The TS SDK's public surface is large: `client.ts` is about 4k LOC, plus query/warm query, session APIs, hooks
  over a local HTTP server, an in-process MCP server via `@modelcontextprotocol/sdk`, and terminal, browser and
  DOM permission dialogs. Several of these take JS callbacks (tools, hooks, `canUseTool`). In a wrapper they
  become JS-side shims around Rust state, which is S02 work with shared vectors per ADR-0002.

PENDING (not dropped): the full SDK crate and wrapper port (a multi-slice effort; the compatibility contract is
the alpha JS API). Publication stays unauthorized per ADR-0002.
