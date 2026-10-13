# SRC-DEFECT-010: commandParser substitutes `$` patterns inside argument values and included files

- Source: `ui/server/utils/commandParser.js`.
  - `replaceArguments` passes argument values to `String.prototype.replace` as the replacement string, so `$&`, `` $` ``, `$'` and `$$` in a value are expanded.
  - It replaces `$ARGUMENTS` first, then each `$n` across the whole result, so a `$2` inside an argument is substituted again.
  - `processFileIncludes` does the same with file contents: a file containing `$&` is replaced by the `@file` text.
- Example: args `["$&"]` on `run $1` gives `run $1` in TS. Expected: `run $&`.
- Fix (low risk, policy of 2026-10-13 14:08): a single pass that inserts values literally. `$10` stays `$1` followed by `0`, as in TS and bash.
- Evidence: `command_parser_oracle` checks every case against a JS reference of the fixed behaviour. TS differs in 270 of 1500 cases, and every one of them has a `$` in an argument. The `@dollar.md` include case is intended.
