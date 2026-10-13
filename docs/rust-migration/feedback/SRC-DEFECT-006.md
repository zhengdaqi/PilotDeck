# SRC-DEFECT-006: TS gateway never cleans up after a client half-closes a WebSocket without a close frame

- Source: `TextWebSocketConnection` only listens for `'close'` and `'error'`. The upgraded socket inherits
  `allowHalfOpen` from the HTTP server. When a client sends TCP FIN without a WebSocket close frame (process exit,
  `socket.shutdown(SHUT_WR)`), Node emits `'end'` but never `'close'`. As a result `GatewayWsConnection.handleClose` does not
  run:
  - in-flight turns are not aborted
  - `disconnectInteraction` is not called
  - the socket stays half-open until a later write fails.
- Reproduced by `gateway_server_parity.py`, divergence "SRC-DEFECT-006 ...": `ts_cleanup_calls=[]`,
  `rust_cleanup_calls=[disconnectInteraction, abortTurn]`.
- When the close handshake is initiated by a client close frame, the server-initiated close (4001/4002/4003) or a socket
  error, both implementations run identical cleanup. That case is in the parity set (`log_after_close`).
- Rust treatment (bounded fix, **FINAL: intended behaviour difference, ADR-0004**): read EOF is treated as connection close, so cleanup runs.
