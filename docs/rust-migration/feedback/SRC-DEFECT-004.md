# SRC-DEFECT-004: one malformed WebSocket frame kills the whole TS gateway server

- Source: `src/gateway/server/websocket.ts` `readClientFrame` throws on an unmasked client frame ("Client WebSocket frames
  must be masked.") or on a frame longer than 2^53-1 bytes ("WebSocket frame is too large."). The throw happens inside the
  socket `'data'` listener (`handleData`), which nothing catches, so Node terminates the whole gateway process.
- Reachability: any client that can open `/ws`. The upgrade happens before hello/auth, so authentication is not required.
  Binding is loopback-only by default.
- Reproduced by `rust/parity/gateway/gateway_server_parity.py`. Divergence "SRC-DEFECT-004 unmasked client frame":
  `ts_server_alive=false`.
- Rust treatment (bounded fix, **pending user decision**): `ws::serve` destroys only the offending connection. The server
  stays up (`rust_server_alive=true`, `/health` 200 afterwards). This case is reported as a documented divergence and never
  counted as a parity pass. If the user wants byte-for-byte compatibility instead, the Rust server would have to exit the
  process. We do not recommend that.
