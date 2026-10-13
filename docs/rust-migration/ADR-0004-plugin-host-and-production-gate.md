# ADR-0004: Plugin host amendment (D4), source-defect decisions, production gate (FINAL, 2026-10-13)

Status: **FINAL** (decided by the user Da on 2026-10-13). Amends ADR-0002 D4.

## 1. Plugin host (amends ADR-0002 D4)
JS plugins are hosted by **spawning Node child processes** (option 1 of `probes/ADR-0002-D4-plugin-host-probe.md`).
The Rust runtime resolves the `__PILOTDECK_NODE_EXECUTABLE__` marker to a Node executable and starts MCP-server
plugins as subprocesses, exactly like the TS runtime does with `process.execPath`. No embedded JS engine is
introduced. First-party PilotDeck logic must not live in plugins.

## 2. Source-defect decisions
- SRC-DEFECT-004, -005, -006: the Rust fixes are **intended behaviour differences** (final). Parity reports keep
  listing them as documented divergences, never as passes.
- SRC-DEFECT-007, -008: Rust **keeps reproducing the TS behaviour** for now (decision may be revisited).

## 3. Production gate
Production stays on the TS implementation until **all** of the following hold: everything is migrated; the
migration is independently reviewed; and it is verified on Linux, macOS and Windows. Until then Rust binaries
remain opt-in/experimental and `pilotdeck` entry points keep forwarding to TS where not yet native.
