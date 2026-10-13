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
- SRC-DEFECT-007, -008: **FINAL (user decision 2026-10-13 14:02, replaces the earlier "keep reproducing TS"): fix both;
  these are intended behaviour differences.**
  - 007: the Rust `/api/web/projects/:key/files/write` refuses any write that resolves outside the workspace. It returns
    403 `workspace_boundary`. Real paths are checked both before mkdir and just before the write:
    - an existing target, including a dangling symlink, must resolve inside the root;
    - so must the deepest existing ancestor.

    So no files or directories are created outside the root. Remaining risk: a race window between the check and the
    write, if another process swaps a symlink in between (no O_NOFOLLOW/openat yet). This is recorded as a follow-up.
    Evidence:
    - `rust/crates/gateway-server/tests/write_boundary.rs`, including a negative control. The control shows the
      fixture really escapes when written without the check.
    - A mutation check: removing the check makes the test fail.
    - `gateway_server_parity.py` INTENDED cases: `w_via_link_out`, `w_via_secret_link`, `outside_after`, and the
      knock-on `tree_after`.
  - 008: an oversized (>64 KiB) public approval input gets HTTP 413 `{"code":"APPROVAL_INPUT_TOO_LARGE"}`. TS sends no
    response. Evidence: `gateway_server_parity.py` INTENDED case `public/ap_too_large` (TS: no response; Rust: 413).
  - The parity harness checks each intended case. TS must still show the defect and Rust the fix. These cases are not
    counted as parity passes: 832/832 parity + 5/5 intended.

## 3. Production gate
Production stays on the TS implementation until **all** of the following hold: everything is migrated; the
migration is independently reviewed; and it is verified on Linux, macOS and Windows. Until then Rust binaries
remain opt-in/experimental and `pilotdeck` entry points keep forwarding to TS where not yet native.
