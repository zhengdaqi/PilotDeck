# SRC-DEFECT-003 — agent-loop parity gate is red on the baseline (native vs TS sidecar)

- Baseline `b243fa00` (Node 22.23.3), `python3 tools/agent-loop-parity/run.py --comparison same-version --surface gateway --scenario all`:
  53 scenarios executed, `blocked=0`, `oracleFailures=0`, **`failed=26`** (repeatable, not a flake: 3/3 reruns of `single_tool`).
  `docs/pilotdeck-agent-loop-parity-results.zh.md` records 53/53 green at 2026-09-18; the regression came later
  (candidates: 4a45b845 / 89123b32, agent status relay changes, 2026-10-03..05).
- Typical diff (`single_tool`): the native path publishes `agent.status turn_continued{reason:next_turn}` *after* the next
  model response, the TS sidecar publishes it right after the tool result (the order the AgentLoop actually yields).
- Migration treatment: the oracle for the Rust agent-loop sidecar is the **TS sidecar** (the component it replaces):
  Rust-sidecar traces must equal TS-sidecar traces under the tool's own comparator, and the full gate run with the Rust
  sidecar must reproduce exactly the baseline's failure set (no new failures, no "fixed" ones hidden).
  Not fixed by the migration; reported for the product owners.
- Evidence: `evidence/agent-loop-parity-ts-baseline-summary.json`.

## Addendum: flakiness of the TS-sidecar baseline
A second full baseline run had 25 failures, not 26, and the failing set was different. permission_ask_approve,
multi_tool_mixed_permission and sidecar_parent_close_after_admission flip from run to run because of timing. The Rust sidecar
gate therefore compares failure sets modulo these three scenarios.
