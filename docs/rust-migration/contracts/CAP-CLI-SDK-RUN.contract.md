# Contract CAP-CLI-SDK-RUN v1 (slice 6: `pilotdeck run` / `pilotdeck resume`)

| Field | Value |
|---|---|
| Entry | `pilotdeck run <prompt…>`, `pilotdeck resume <session-id> <prompt…>`; prompt from stdin when no positional text and stdin is not a TTY; flags `--gateway-url --auth-token --project --channel --model --permission-mode --json --stream-json` |
| source_scope | `src/cli/sdkCli.ts#runTurn/consumeRun/eventText/readPrompt`; `packages/sdk/src/client.ts` `sessions.create/resume`, `runs.start`, `createRunHandle`, `PilotDeckQueryImpl` (initialize, next, result, abort), `mapPublicMessage`, `resolveModelOverride`, `gatewayTurnPermissionMode/BasePermissionMode`, `sdkSessionConfig` (permissionMode subset); `transport.ts#stream` (seq validation, final frame) |
| Rust | `rust/crates/cli/src/sdk_cli.rs` (`run_turn`), `rust/crates/gateway-client/src/transport.rs` (`stream_start/stream_next`, backlog demux) |
| Wire sequence | control WS (`clientName sdk`, `clientVersion pilotdeck-cli`): `new_session {projectKey?, channelKey}` (run) or `resume_session {sessionKey}` (resume) → prompt read → run WS: `resume_session` → [`model_catalog_list`] → `submit_turn` stream with `channelKey:"api_server"`, `canPrompt:false`, `canElicit:false`, mode mapping, `sdkSessionConfig` (`{}` when server advertises `sdk_session_defaults`, `{permissionMode}` for acceptEdits/dontAsk), `runId` |
| Output | text mode: assistant-typed event text streamed (+`\n` when completed), otherwise output / `JSON.stringify(result)`; `--stream-json`: one `JSON.stringify(mappedEvent)` per line (partial `assistant.message/thinking` filtered); `--json`: `{"sessionId","result"}`; exit 1 unless `completed` |
| Results | completed `{status,output,usage,finishReason}`; failed `{status,error:{code,details,retryable:false,name:"PilotDeckError"}}`; result_unknown `{status,recovery:{sessionId,runId}}`; SIGINT/SIGTERM → `abort_turn {sessionKey,runId,reason:"CLI interrupted"}` → `{"status":"aborted","reason":"CLI interrupted"}` exit 1 |
| Deviation | SRC-DEFECT-002 (TS hangs after init errors; Rust exits) |
| Evidence | `evidence/parity-run-resume.json` 25/25 (real TS gateway + agent loop, deterministic OpenAI mock `rust/parity/mock_openai.py`, incl. SIGINT abort, stdin prompt, model override, permission modes, auth/URL errors) |
| Known gaps | SDK features not reachable from the CLI (canUseTool, elicitation/user dialogs, hosted hooks/MCP, session-store mirror) are not ported here — they belong to CAP-SDK-NPM; wss/TLS as in CAP-CLI-SDK |
