# PilotDeck TS → Rust migration (playbook ts-to-rust-agent-playbook v2.1)

Status: **slice 1 (P09) done on linux; the full migration has NOT been done.** Scope is the whole `standalone_alpha1` product; only `pilotdeck chat search` runs in Rust so far.

| Artifact | File |
|---|---|
| Frozen baseline (commit/tree/lock/runtime) | `baseline.json` |
| Resolved config / scope axes / permissions | `resolved-config.json` |
| Profile decision (light, session_autonomous) | `profile-decision.md` |
| Full-scope inventory + risk table + unknowns | `inventory.md`, `inventory-loc.json` |
| ADR | `ADR-0001-rust-architecture.md` |
| Strategy facts → decision (S03 SELECTED) | `strategy-facts.json`, `strategy-decision.json` |
| Slice-1 contract | `contracts/CAP-CLI-CHAT-SEARCH.contract.md` |
| Capability coverage | `coverage.csv` |
| Evidence | `evidence/parity-chat-search.json`, `evidence/ts-baseline-tests.txt` |

## Reproduce / resume
```sh
# Node 22 (engines: >=22.13 <23) + pnpm 10.32.1
pnpm install --frozen-lockfile && npm run build      # TS oracle -> dist/
NODE=node rust/parity/run.sh                          # cargo build --release + 63-case parity
(cd rust && cargo test)                               # Rust unit tests
```
Run the Rust CLI: `rust/target/release/pilotdeck chat search docker --json`. Other commands: set `PILOTDECK_TS_CLI=<abs path to dist/src/cli/pilotdeck.js>` to forward them to the TS CLI (transitional shim).

## Honest deviations from the playbook
- Author and verifier were the same agent session (light profile); an independent re-run/review is still required before acceptance.
- The strategy selector was run after the slice code was written (the slice served as the native-entry probe); the decision is SELECTED with no probes outstanding, but the ordering differs from P08→P09.
- One parity run executed concurrently with the full TS test suite showed 19 transient mismatches; 3 idle runs passed 63/63. Root cause not isolated yet (FB-001).
- No CI wiring yet; Windows/macOS not verified.
