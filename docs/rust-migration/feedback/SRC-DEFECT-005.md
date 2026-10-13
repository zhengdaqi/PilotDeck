# SRC-DEFECT-005: unhandled rejections in the TS gateway HTTP handler terminate the server

- Source: `GatewayServer.ts` calls `void handleHttpRequest(...)` with no catch. Any rejection escaping the handler is an
  unhandled rejection, and Node 22's default mode then terminates the process. Observed triggers:
  - `/api/web/projects/<key>/<sub>`: `decodeURIComponent(match[1])` runs before the `try` block in
    `src/adapters/web/httpRouter.ts`. Example: `/api/web/projects/%zz/files/tree` raises URIError and the process exits.
  - `GET /api/web/projects` when `gateway.listProjects` rejects or is missing.
- Reproduced by `gateway_server_parity.py`, divergence "SRC-DEFECT-005 ...": `ts_server_alive=false`. The `minimal`
  fake-gateway variant excludes the authorized `/api/web` project cases for the same reason.
- Rust treatment (bounded fix, **FINAL: intended behaviour difference, ADR-0004**): the request is dropped without a response. The process and
  every other connection keep running.
- Not affected: `decodeURIComponent` inside the `try` (describe project) turns into
  `404 {"error":{"code":"project_not_found","message":"URI malformed"}}` on both sides. That case is in the parity set
  (`web_project_bad_pct`).
