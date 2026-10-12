# Execution profile decision (P00)

**Selected: `light`, run mode `session_autonomous`.**

Evidence / reasons:
- One machine (agent box), one coordinator, one product writer in this session; no second concurrent writer or multi-machine dispatch exists.
- The playbook's `auto` rule picks the lowest sufficient profile for the first slice; no cross-session recovery service is available or authorized, so `persistent_supervised` is not claimed.
- Upgrade trigger to `standard`: the moment two author lanes run in parallel or the work must resume across sessions with leases (expected at P12+ given the ~180k-line `src/` + ~185k-line UI scope).

Quality invariants are unchanged by the profile (frozen oracle, identity-bound evidence, formal entry-point verification, negative controls).

Known deviation: author and verifier for slice 1 ran in the same agent session; the playbook requires an independent verifier session. The parity runner is deterministic and re-runnable, but an independent re-run by a separate reviewer is still required before the slice is counted as accepted.
