# SRC-DEFECT-015: malformed Gateway frames throw inside the SDK's WebSocket listener

- Source: `packages/sdk/src/transport.ts` `handleMessage`. Three kinds of frame make the listener throw:
  - a `null` frame (`frame.type` on null);
  - `{"type":"response","ok":false}` with no `error`, or with `error: null`;
  - `error.code` that is not a string (`code?.toLowerCase` is not a function).

  With Node's WebSocket the exception is uncaught in the host process. For the response cases the pending request has already been deleted from the map, so it never settles: the caller hangs until the request timeout or forever.
- Fix (low risk, defect policy): Rust ignores a `null` frame. It treats a missing or null `error` as `{}` and a non-string code as absent, which gives `gateway_request_failed` / "Gateway request failed.".
- Evidence: `transport_oracle` `malformed_frames_do_not_crash_src_defect_015`. The TS mock run logs `crash` for all 4 fixed scripts; Rust settles or ignores as described.
