# YAML-GAP-001 (pending): js-yaml leniency for single-line explicit keys

js-yaml 3.14 accepts some invalid or lenient forms of an explicit key written on one line, for example:
- `? {x: 1}: a: b`;
- `? k: 'multi` followed by a less-indented continuation line.

yaml-rust2 0.13, which the Rust port uses as its YAML syntax layer, rejects these, so Rust throws where TS returns data. They are not realistic command front matter. The oracle pins the count at 5 cases so the gap cannot grow unnoticed.

Options:
- (a) Patch or fork the YAML scanner to accept js-yaml's leniency.
- (b) Accept the gap and record it as an intended difference.

Neither has been chosen yet. Kept pending.
