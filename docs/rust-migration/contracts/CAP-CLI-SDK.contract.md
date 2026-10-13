# Contract CAP-CLI-SDK v1 (slice 5: `sessions`, `settings`; `help` in slice 4)

| Field | Value |
|---|---|
| Entry | `pilotdeck sessions [list\|messages <id>] [--project p]`, `pilotdeck settings [get\|set '<json>'\|set --json '<json>']` + `--gateway-url`, `--auth-token` |
| source_scope | `src/cli/sdkCli.ts` (readConnection, positional, readValue, runSessionsCommand, runSettingsCommand), `packages/sdk/src/transport.ts` (connect/hello/request, error mapping), `packages/sdk/src/client.ts` (`sessions.list/messages`, `resolveSettings`, `updateSettings`, `connectionOptions`) |
| Rust | `rust/crates/gateway-client/src/transport.rs`, `rust/crates/cli/src/sdk_cli.rs`, `rust/crates/skills/src/v8json.rs` (JSON.parse with V8 error text) |
| Connection | URL: `--gateway-url` ?? `PILOTDECK_SDK_GATEWAY_URL` ?? `PILOTDECK_GATEWAY_URL` ?? `ws://127.0.0.1:${PILOTDECK_GATEWAY_PORT ?? 18789}/ws`; token: `--auth-token` ?? `PILOTDECK_SDK_AUTH_TOKEN` ?? `PILOTDECK_TOKEN` ?? trimmed `$PILOT_HOME/server-token` (JS `??`: an empty env var is used, not skipped). Hello `clientName:"sdk"`, `clientVersion:"pilotdeck-cli"` (sessions) / `"0.1.0"` (settings). Timeouts: connect+hello 10 s, request 30 s. |
| Errors (stderr, exit 1) | `Gateway auth token is required. …`; `gatewayUrl and authToken are required; …`; `Gateway connection failed.`; `Gateway connection timed out.`; `Gateway hello timed out.`; close reason (e.g. `auth_failed`) or `Gateway WebSocket closed.`; server `error.message`; `TypeError: Invalid URL`; `Expected a ws: or wss: protocol, got <s>:`; `Got fragment`; usage strings; `Invalid settings JSON: <V8 SyntaxError message>`; `updateSettings settings must be an object.` |
| Output | `JSON.stringify(x, null, 2)` of `result.sessions ?? []` / `result.messages ?? []` / resolve_settings result; `{"updated":true}` |
| Normalizer | uuids, current-day ISO timestamps, epoch-ms, `fileSize`, `contentHash`; 2 s settle after each `settings set` (server config-watcher version counter is timing-sensitive) |
| Evidence | `evidence/parity-sdk-cli-sessions-settings.json` 43/43 + negative controls; `evidence/v8json-fuzz.txt` (15k differential cases vs Node `JSON.parse`) |
| Known gaps | `wss://`/`https://` not supported yet (no TLS linked; fails as `Gateway connection failed.`); lone UTF-16 surrogate escapes in settings JSON are replaced with U+FFFD; WHATWG URL parsing approximated for common forms; `run`/`resume` still forwarded to TS (needs SDK run/event-stream port) |
