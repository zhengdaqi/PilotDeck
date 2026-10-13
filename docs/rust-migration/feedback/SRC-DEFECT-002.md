# SRC-DEFECT-002 — `pilotdeck run|resume` never exits after a run-initialization error (TS baseline)

- Baseline `b243fa00`: when `PilotDeckQueryImpl.initialize` throws (e.g. `--model` without `--project` →
  `projectKey is required when options.model is set.`, or `Model is not uniquely resolvable: <m>`), the CLI prints the
  message via `main().catch` and sets exit code 1, but the run's own query transport (a second WebSocket opened by
  `runs.start`) is never closed, so Node keeps running forever.
- Rust: same stderr and exit code 1, then exits. Same treatment as SRC-DEFECT-001 (fix of a hang, not of a product
  promise). Parity: oracle killed after 25 s idle, exit code derived as `1 iff stderr non-empty`
  (`run_parity.py`, cases `run-model-no-project`, `run-model-missing`).
