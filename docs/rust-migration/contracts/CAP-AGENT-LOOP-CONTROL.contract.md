# CAP-AGENT-LOOP-CONTROL — Rust agent-loop sidecar control plane (sub-slice of CAP-AGENT-LOOP)

Source of truth: `src/agent/modules/transport/agentLoopSidecarServer.ts`, `validateModuleMessage` in `src/agent/modules/protocol.ts`.
Oracle: TS sidecar `dist/src/cli/pilotdeck-agent-loop-sidecar.js` (see SRC-DEFECT-003 for why it is the oracle, not native).

Covered: JSON-lines framing; INVALID_JSON; every validateModuleMessage code; hello/capabilities handshake
(binding ids, capabilities payload); cancel → UNKNOWN_OPERATION; status → UNKNOWN_REQUEST; resume/ack unsupported;
module_call → UNEXPECTED_MODULE_CALL; execute admission with expired deadlines → DEADLINE_EXCEEDED.
Random ids (`prefix-<base36>`) normalized in comparison.

Admitted execute is now covered by CAP-AGENT-LOOP-EXEC.
Production still runs the TS sidecar.

Evidence: `rust/parity/agent_loop_control_parity.py` → `evidence/parity-agent-loop-control.txt` (18/18, Linux).
