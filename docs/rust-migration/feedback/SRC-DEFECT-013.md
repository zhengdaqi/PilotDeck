# SRC-DEFECT-013: date-like YAML keys turn into time-zone dependent strings

- Source: `ui/server/utils/frontmatter.js` (gray-matter + js-yaml 3.14 `safeLoad`). js-yaml resolves a plain key such as `2020-01-01` as a timestamp, then converts it to a string with `String(date)`. The result, `"Wed Jan 01 2020 08:00:00 GMT+0800 (China Standard Time)"`, depends on the server's time zone and locale. The same command file therefore gives different keys on different machines.
- Fix (low risk, defect policy): a timestamp key keeps its source text (`2020-01-01`). Timestamp values are unchanged; they still serialise as ISO strings, as in TS.
- Evidence: `frontmatter_oracle` turns every `String(Date)` key in the TS output back into its source text and requires an exact match with Rust. 311 cases are intended differences.
