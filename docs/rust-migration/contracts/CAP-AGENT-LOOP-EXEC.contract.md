# CAP-AGENT-LOOP-EXEC: Rust agent-loop sidecar execute path (sub-slice of CAP-AGENT-LOOP)

Source of truth: the `AgentLoop.run`/`runOrdered` methods in `src/agent/loop/AgentLoop.ts`, plus `AgentLoopSidecarServer.handleExecute`,
`createAgentLoopSidecarFactory`, `hostToolPort`/`hostModelPort` (in `src/agent/modules/transport/*`).
Oracle: TS sidecar running behind the same shim (`PILOTDECK_AGENT_LOOP_SIDECAR_EXEC` unset). See SRC-DEFECT-003.

Rust: `rust/crates/agent-loop-sidecar/src/{rt,exec,factory,agent,looprun,status,timeline,util,cacheplan,lfr,tokenizer,truncate,plantodo}.rs`.
The binary runs the native runtime: `rt::Host` owns stdio and the control plane, and every admitted execute runs on its own
thread with its own `rt::Rt` handle.

Ported:
- Turn loop with plan/todo, steer drain, budget preview, auto-compact and persist_compaction.
- Lifecycle hooks (validated and detached), the model prepare/stream/close_stream path, and the assistant assembler.
- Batch and per-call tool execution: concurrency-safe calls first, then sequential. Includes cancel detection and transport failure results.
- Tool-result projection, repeated-failure annotation, reactive prompt-too-long recovery, and output-token retry and caps.
- Token calibration, usage cost, and the max_turns, error and aborted results.
- Terminal events: completed, failed, cancelled, result_unknown, and DEADLINE_EXCEEDED (deadline-unknown).
- seedState and modelState projections.
- Pure-Rust sha256 for `requestFingerprint`.
- (slice 8d, previously PENDING)
  - ask runMode tool filter (`ask_mode.json`, generated from TS by `rust/parity/gen_ask_mode.mjs`).
  - cachePlan (`buildCachePlan`: stableSerialize + localeCompare + sha256).
  - media_reference materialization, with diagnostics.
  - `materialize_prepared_request` with `budgetPreparationId`. On the routed compaction path, persist_compaction runs after re-materialize, as in TS.
  - LargeFileRepair (all 4 wiring points) and repaired truncated tool calls.
  - `truncateHeadPreservingCheckpoint`.
  - countTokens fallback (js-tiktoken o200k_base port: `tokenizer.rs` + `o200k_base.bin` from `gen_o200k.mjs`).
  - `parsePlanTodoSnapshot` validation and history replay (`plantodo.rs`).
  - Concurrent executes: executes for different operations interleave; the same operation is still rejected with
    OPERATION_ALREADY_ACTIVE / OPERATION_IDENTITY_MISMATCH.
- Proven inert inside the sidecar process, so ported as no-ops. Both need host-side wiring that the sidecar factory never provides:
  - subagent status pump / TOOL_EVENT_PUMP. `AgentLoop.executeToolsWithEventPump` only yields events from
    `capabilities.events.drain`. The sidecar factory (`pilotdeck-agent-loop-default-factory.ts`) passes only `eventEmitter`
    and never `drainEvents`, so the drain is undefined. `activeSubagents` stays empty, which means no subagent_status or
    heartbeat event can be produced. The 500 ms sleep only affects timing, not output. Host-side draining is done by
    `agentLoopSidecarClient` (TS host, out of scope for the sidecar binary).
  - permission decide port. The factory builds `permission` from the host `decide` method, but `AgentLoop` never reads
    `capabilities.permission`. `createSidecarPorts` is called without `permission`/`authorization`, so tool authorization
    happens host-side in `execute`/`execute_batch`.
  If a future TS change wires either of these into the sidecar, the Rust port must follow.

Known approximations (guarded by differential fixtures):
- `locale_compare` is a CLDR-root approximation for the ASCII and Latin-1 keys that cache-plan sorting sees. It matched
  20000/20000 generated pairs (`tests/locale_compare.rs`), but is not a full ICU collator.
- The tokenizer regex uses the `regex` crate's Unicode tables plus lookahead emulation. Node's ICU Unicode version may
  differ for newly assigned code points. It matched 4001/4001 generated cases (`tests/count_tokens.rs`).
- Lone UTF-16 surrogates still cannot be represented (known gap, shared with the rest of the Rust JSON layer).

There is no remaining `RUST_SIDECAR_PENDING` call site. The `pending()` helper is kept only for future gaps.

Production: NOT switched. The shim only delegates when `PILOTDECK_AGENT_LOOP_SIDECAR_EXEC` is set, and stays on the TS sidecar
until the user approves the cutover. All PENDING items are now ported, and parity has been re-run (see Evidence).

Evidence:
- `evidence/agent-loop-parity-rust-vs-ts.txt`
- `evidence/agent-loop-parity-rust-sidecar-summary.json`: 53 scenarios. Same failure set as the TS sidecar baseline, apart from timing-flaky lines that were triaged by re-runs.
- Control parity 18/18.
- `evidence/agent-loop-replay-parity-slice8d.txt`: replay differential of 70 TS recordings x 13 variants, plus the concurrent-execute differential.
- `cargo test --workspace`.
