# CAP-AGENT-LOOP-EXEC: Rust agent-loop sidecar execute path (sub-slice of CAP-AGENT-LOOP)

Source of truth: the `AgentLoop.run`/`runOrdered` methods in `src/agent/loop/AgentLoop.ts`, plus `AgentLoopSidecarServer.handleExecute`,
`createAgentLoopSidecarFactory`, `hostToolPort`/`hostModelPort` (in `src/agent/modules/transport/*`).
Oracle: TS sidecar running behind the same shim (`PILOTDECK_AGENT_LOOP_SIDECAR_EXEC` unset). See SRC-DEFECT-003.

Rust: `rust/crates/agent-loop-sidecar/src/{rt,exec,factory,agent,looprun,status,timeline,util}.rs`.
The binary now runs the native runtime (`Rt::serve`), and execute is fully handled.

Ported:
- Turn loop with plan/todo, steer drain, budget preview, auto-compact and persist_compaction.
- Lifecycle hooks (validated and detached), the model prepare/stream/close_stream path, and the assistant assembler.
- Batch and per-call tool execution: concurrency-safe calls first, then sequential. Includes cancel detection and transport failure results.
- Tool-result projection, repeated-failure annotation, reactive prompt-too-long recovery, and output-token retry and caps.
- Token calibration, usage cost, and the max_turns, error and aborted results.
- Terminal events: completed, failed, cancelled, result_unknown, and DEADLINE_EXCEEDED (deadline-unknown).
- seedState and modelState projections.
- Pure-Rust sha256 for `requestFingerprint`.

PENDING. These are not dropped: they are required for the full migration and production cutover. Each one currently fails the
operation loudly with code `RUST_SIDECAR_PENDING`, and never fakes success:
- ask runMode
- cachePlan
- media_reference content
- materialize_prepared_request
- LargeFileRepair / repaired truncated tool calls
- truncateHeadPreservingCheckpoint
- countTokens fallback
- subagent status pump (2000 ms heartbeat) and TOOL_EVENT_PUMP (500 ms)
- concurrent executes (the Rust runtime queues them; TS interleaves them)
- permission decide port
- parsePlanTodoSnapshot validation

Production: NOT switched. The shim only delegates when `PILOTDECK_AGENT_LOOP_SIDECAR_EXEC` is set, and stays on the TS sidecar
until every PENDING item is ported and parity is re-run.

Evidence:
- `evidence/agent-loop-parity-rust-vs-ts.txt`
- `evidence/agent-loop-parity-rust-sidecar-summary.json`: 53 scenarios. Same failure set as the TS sidecar baseline, apart from timing-flaky lines that were triaged by re-runs.
- Control parity 18/18.
- `cargo test --workspace`.
