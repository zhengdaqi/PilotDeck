# Contract CAP-CLI-CHAT-SEARCH v1 (frozen for slice 1)

| Field | Value |
|---|---|
| contract_id / version | CAP-CLI-CHAT-SEARCH / v1 |
| Capability, entry, consumers | `pilotdeck chat search <keyword> [flags]` — end users and scripts (`--json`) |
| Strategy | S03 native; decision `strategy-decision.json` (SELECTED) |
| Exit checks → cases | external_contract_parity → 63 parity cases; formal_entrypoint → cases run the real `pilotdeck` binaries (TS `dist/src/cli/pilotdeck.js` vs Rust `rust/target/release/pilotdeck`); architecture_dependency_constraints → ADR-0001 crate graph (cli → session-search only); feedback_window / full_scope_native_ownership → open (later phases) |
| Risk / review | low-medium (read-only file I/O, no writes, no network) |
| Four axes | functional: yes · external_protocol: CLI argv/stdout/stderr/exit code · lifecycle: n/a (one-shot) · interaction: n/a |
| Baseline | commit `b243fa00`, tree `343671dd`, lock `664356f6…` |
| source_scope | `src/cli/commands/chatSearch.ts#runChatSearchCli`, `src/session/search/{NodeSessionSearchPort,formatChatHistorySearch,SessionSearchPort}.ts`, `src/session/storage/{SessionList#readSessionInfo,SessionLiteReader}.ts`, `src/pilot/paths.ts` (pilot home, project id, `.cwd` markers), `ProjectSessionStorage#sanitizeSessionIdForPath` |
| Rust target | `rust/crates/session-search`, `rust/crates/cli/src/chat_search.rs` |
| Input domain | argv tokens; `PILOT_HOME`, `HOME`, cwd; `<pilotHome>/projects/<id>/chats/*.jsonl`, `.cwd` markers |
| Output | `--json`: `JSON.stringify(result, null, 2)` + `\n`, key order `query, matches[sessionId, role, text, snippet, createdAt, lineNumber, sessionTitle, projectKey], truncated, sessionsScanned`. Text: `formatChatHistorySearchResults(locale=en, includeProject=--all-projects)` + `\n` |
| Errors | non-`search` subcommand → usage on stderr, exit 1; empty keyword → `Error: search keyword is required.`, exit 1; missing `--session` file → `ENOENT: no such file or directory, open '<path>'`, exit 1; invalid regex / `null` JSONL line / undecodable metadata title → message on stderr, exit 1 (message text compared only for presence) |
| Numbers/strings | `--limit` uses JS `Number()` (hex, exponent, fractions; non-finite ignored), clamped to [1,100], fractional limits compare as floats; snippets use UTF-16 indices and JS `\s`/`trim` sets; case-insensitive plain search uses full Unicode lowercase |
| Ordering | directory listings sorted bytewise (libuv scandir order); files in listing order, lines in file order; stop at limit |
| Bug-compatibility kept | `--pilot-home <v>` value is also appended to the query (TS filter only skips values of --project/--limit/--role/--session); `createdAt` non-string → `""` |
| State / side effects | none (read-only). No data format change, no rollback needed |
| Normalizer | Node.js runtime `ExperimentalWarning` banners stripped from oracle stderr (emitted by the node host, not PilotDeck code). Nothing else normalized. Fixed env: `TZ=Asia/Shanghai`, `LANG=C.UTF-8`, `HOME=<fixture>` |
| Negative controls | mutated stdout detected; wrong binary (`/bin/echo`) detected; empty-oracle guard |
| Platforms | linux x86_64 verified; macOS/Windows pending (Windows path rules for `sanitizeSessionIdForPath`/project ids are **not** ported yet) |
| Known divergences (accepted as open issues, not hidden) | JS RegExp features absent in Rust `regex` (lookaround, backreferences) → error instead of results; lone-surrogate `\uD800` escapes / out-of-range numbers make serde_json skip a line that JSON.parse accepts; non-ISO `createdAt` strings that `new Date()` can parse are printed raw; ICU-version-dependent `toLocaleString` |
| Performance | no threshold frozen in TS; Rust must not be slower than TS on the parity fixture (measured informally: Rust ~ms vs Node ~0.8 s per call incl. startup) |
| Feedback window | to be opened after merge into a Rust candidate build |
