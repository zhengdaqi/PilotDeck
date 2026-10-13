# YAML-GAP-001 (closed, option a): js-yaml leniency for single-line explicit keys

js-yaml 3.14 accepts some lenient forms of an explicit key written on one line, for example:
- `? {x: 1}: a: b`;
- `? k: 'multi` followed by a less-indented continuation line.

yaml-rust2 0.13 rejected these, so Rust used to throw where TS returned data (5 oracle cases).

Decision (user, 2026-10-13): option (a), behavior compatibility first.

Resolution (slice 24): yaml-rust2 was removed. `ui-server-core/src/js_yaml.rs` is a line-by-line port of js-yaml 3.14.2
`loader.js` with DEFAULT_SAFE_SCHEMA. It works on UTF-16 code units, so the leniency is the same by construction.
- `frontmatter_oracle`: 4035/4035 match with 0 known gaps. YAML errors are now compared by js-yaml's reason text, not just "threw".
- New `js_yaml_oracle`: 6000 random syntax-heavy documents (`gen_js_yaml.mjs`) match exactly, as values or as error reasons.

Remaining representation limits:
- The line/column snippet is not reproduced.
- Circular aliases and lone surrogates are not reproduced; JSON can't carry either.
- Neither reaches the HTTP surface.
