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

    So no files or directories are created outside the root. The check-then-write race is **closed** (slice 17).
    `safe_write` walks from an fd for the root, one component at a time, using `openat(O_DIRECTORY|O_NOFOLLOW)` and
    `mkdirat`, and opens the file with `openat(O_NOFOLLOW)`. A symlink inside the root is followed only after its
    target is checked; the walk then restarts from the root fd. Evidence:
    - A stress test (`symlink_swap_race_never_escapes`) swaps a directory and a symlink 3000 times. 0 escapes.
    - Negative control: with the old check-then-`fs::write`, the same test leaked 80-130 files outside the root in
      each of 3 runs.
    - Unix only, like the rest of the crate. Windows needs its own handling when the Windows port is done.
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

## Source-defect policy (user decision 2026-10-13 14:08)
Low-risk source defects are **fixed by default** and recorded here as intended differences, each with tests that
include a negative control. The parity harness must show TS still has the defect and Rust has the fix. Only defects
with product or behaviour impact are escalated to the user.

## SRC-DEFECT-009 (FINAL 2026-10-13 14:08: fixed, intended difference)
`planModelRemoval` for a provider id that is not configured, including `__proto__`, returns `NOT_FOUND`
("Provider \"<id>\" is not configured."). Rust looks up own keys only; `serde_json::Map` has no prototype chain.
TS returns success or blames the model instead. Evidence: the `model_references_oracle` test checks 167 intended
cases (TS shows the defect, Rust the fix) and 7878 byte-exact cases.

## SRC-DEFECT-010/011/012 (2026-10-13: fixed under the source-defect policy, intended differences)
These defects are in `ui/server/utils/commandParser.js`; see `feedback/SRC-DEFECT-01{0,1,2}.md`.
- 010: argument values and included contents are inserted literally, with no `$&`/`$'`/`` $` ``/`$$` expansion and no second substitution.
- 011: `@file` includes are refused when the real path leaves the real base, which closes the symlink escape. The error text is TS's traversal error.
- 012: the whitespace in front of `@file` is kept.

Evidence: the `command_parser_oracle` test. TS must still show each defect, and the Rust port must match the fixed reference.
