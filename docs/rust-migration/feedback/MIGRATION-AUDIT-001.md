# MIGRATION-AUDIT-001: weak PRNG in two earlier fixture generators (pending)

`rust/parity/gen_locale_compare_fixture.mjs` and `rust/parity/gen_count_tokens_fixture.mjs` use a plain LCG and take `seed % n`.
The low bits of that LCG repeat in short cycles, so some choices come up much less often than intended. In slice 14 the
same pattern left whole SDK ops nearly untested (listSessions was hit once), and switching to mulberry32 fixed it.
Those fixtures still pass, but they may cover less than their counts suggest.
Follow-up (pending, not dropped): add a second, mulberry32-seeded vector set to both generators (keep the existing vectors) and re-verify.
