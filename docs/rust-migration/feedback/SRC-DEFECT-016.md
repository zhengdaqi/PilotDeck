# SRC-DEFECT-016: a malformed request target crashes the SDK host through the hook server

- **Source:** `packages/sdk/src/hook-server.ts` `handleRequest`.
  - It calls `new URL(request.url, "http://localhost")` without a try.
  - Targets the WHATWG parser rejects, such as `//` or `//?event=Stop`, make it throw. Node's HTTP parser accepts these targets.
  - `createServer` calls `void this.handleRequest(...)`, so the rejection is unhandled. On Node ≥15 that terminates the whole SDK host process.
  - Any peer that can reach the hook port can kill the host with one request, before authentication.
- **Fix** (low risk, defect policy): Rust treats a target it cannot resolve as a non-matching path. It answers `404 {"error":"PilotDeck SDK hook endpoint not found."}`. Valid targets behave as before.
- **Evidence:** `rust/parity/sdk/hook_server_parity.mjs` sends `//` and `//<path>` targets.
  - TS raises `ERR_INVALID_URL` as an unhandled rejection; the harness catches it and records it.
  - Rust answers 404.
  - Counted as intended: 18–27 per 150-script run.
