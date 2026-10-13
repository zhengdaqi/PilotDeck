# Contract CAP-CLI-CRON v1 (slice 3, P11 — network protocol + server-side state)

| Field | Value |
|---|---|
| Entry | `pilotdeck cron list [--history] [--limit N] | create --session --message (--once|--cron) [--channel] [--project] [--timezone] | delete <id>|--task <id> [--stop-running] | stop <id>|--run <id>` |
| Strategy | S03 native client; counterpart server stays TS (real `pilotdeck server`) until CAP-GATEWAY is migrated |
| source_scope | `src/cli/pilotdeck.ts#handleCronCommand/readNumberFlag(parseInt)/inferChannelKey`, `src/gateway/client/probeServer.ts`, `GatewayWsClient.connect/request` (hello + request/response only), `server/authToken.ts#readGatewayAuthToken` |
| Rust target | `rust/crates/gateway-client`, `rust/crates/cli/src/cron.rs` |
| Protocol | `GET http://127.0.0.1:18789/health` (200 ms budget) → token from `$PILOT_HOME/server-token` → WS `ws://127.0.0.1:18789/ws`, hello `{type, protocolVersion:"1.2", clientName:"cli", clientVersion:"0.1.0", token}` → wait `hello_ok` (10 s) → `{type:"request", id:<uuid v4>, method:"cron_list|cron_create|cron_delete|cron_stop", params}`; params omit undefined fields exactly like `JSON.stringify` |
| Output | `JSON.stringify(result, null, 2)` (server key order preserved, JS integer-key ordering and number formatting) |
| Errors | no server / no token / hello failure → `pilotdeck cron requires a running pilotdeck server.` exit 1; server error → `error.message`, exit 1; usage messages unchanged (note: usage is checked only after a server connection, as in TS) |
| State | Tasks live in the TS server's cron store; verified through subsequent `list --history` in the same scripted sequence |
| Normalizer | UUIDs → `<uuid#n>` by first appearance; timestamps of the current UTC date → `<now>`; Node ExperimentalWarning banners removed |
| Deviation | SRC-DEFECT-001 (TS hangs after output; Rust exits) |
| Evidence | `evidence/parity-cron.json`: 23/23 steps, negative controls ok |
| Known gaps | gateway URL/port is hard-coded like TS (`gateway.port` config ignored by TS CLI too); `cron_update`/`cron_run_now` have no CLI entry; proxy env (`installGlobalProxy`) not applied — loopback is excluded from proxying in TS as well |
