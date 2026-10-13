# SRC-DEFECT-001 — `pilotdeck cron …` never exits while a server is reachable (TS baseline)

- Observed on baseline `b243fa00`: after a successful WS hello, `handleCronCommand` prints its result (or the error via
  `main().catch`) but never closes the `GatewayWsClient` WebSocket, so the Node process stays alive indefinitely
  (`timeout 30` → exit 124 in every connected case; 21/23 parity steps).
- The observable product output (stdout/stderr, and `process.exitCode` that would be reported) is unaffected.
- Rust behaviour: identical output, then closes the socket and exits with the exit code TS sets (0 on success, 1 on
  error/usage). This is a **fix of a hang, not a change of a product promise**; it is recorded as an explicit,
  independently reviewable deviation. If the user wants strict hang-compatibility (unlikely), revert to waiting forever.
- Parity contract: oracle is killed after 6 s idle; its exit code is derived as `1 iff stderr non-empty`, matching how
  `main()`/`handleCronCommand` set `process.exitCode`.
