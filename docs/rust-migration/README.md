# PilotDeck TS → Rust migration (playbook ts-to-rust-agent-playbook v2.1)

Status: slices 1–2 verified on linux (`pilotdeck chat search`, `pilotdeck skills migrate`). The full migration is NOT done. Product routes frozen in ADR-0002.

| Artifact | File |
|---|---|
| Frozen baseline (commit/tree/lock/runtime) | `baseline.json` |
| Resolved config / scope axes / permissions | `resolved-config.json` |
| Profile decision (light, session_autonomous) | `profile-decision.md` |
| Full-scope inventory + risk table + unknowns | `inventory.md`, `inventory-loc.json` |
| ADRs | `ADR-0001-rust-architecture.md`, `ADR-0002-user-decisions.md` (frozen user decisions) |
| Strategy facts → decision (S03 SELECTED) | `strategy-facts.json`, `strategy-decision.json` |
| Slice contracts | `contracts/*.contract.md` |
| Feedback / regressions | `feedback/FB-*.md` |
| Capability coverage | `coverage.csv` |
| Evidence | `evidence/parity-chat-search.json`, `evidence/ts-baseline-tests.txt` |

## Reproduce / resume
```sh
# Node 22 (engines: >=22.13 <23) + pnpm 10.32.1
pnpm install --frozen-lockfile && npm run build      # TS oracle -> dist/
NODE=node rust/parity/run.sh                          # cargo build --release + 63-case chat-search parity
python3 rust/parity/skills_migrate_parity.py --ts-cli dist/src/cli/pilotdeck.js --rust-bin rust/target/release/pilotdeck  # 28 cases
(cd rust && cargo test)                               # Rust unit tests
```
Run the Rust CLI: `rust/target/release/pilotdeck chat search docker --json`. Other commands: set `PILOTDECK_TS_CLI=<abs path to dist/src/cli/pilotdeck.js>` to forward them to the TS CLI (transitional shim).

## Honest deviations from the playbook
- Author and verifier were the same agent session (light profile); an independent re-run/review is still required before acceptance.
- The strategy selector was run after the slice code was written (the slice served as the native-entry probe); the decision is SELECTED with no probes outstanding, but the ordering differs from P08→P09.
- FB-001 (transient mismatches under load) closed: TS oracle broken by a concurrent SDK rebuild in the test suite; runner now guards/blocks instead of comparing.
- No CI wiring yet; Windows/macOS not verified.
