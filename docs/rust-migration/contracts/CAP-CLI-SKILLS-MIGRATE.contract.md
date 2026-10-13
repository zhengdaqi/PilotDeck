# Contract CAP-CLI-SKILLS-MIGRATE v1 (slice 2, P10 — file I/O with side effects)

| Field | Value |
|---|---|
| Capability / entry | `pilotdeck skills migrate [--execute] [--from cc,openclaw,hermes,all] [--source <dir>]… [--overwrite|--rename] [--project <p>] [--pilot-home <p>] [--json]` |
| Strategy | S03 native (same decision scope family as slice 1; re-run selector with slice-2 evidence: `strategy-decision-slice2.json`) |
| source_scope | `src/cli/pilotdeck.ts#handleSkillsCommand/parseSkillMigrationSources/printSkillMigrationReport`, `src/extension/skills/migrateSkills.ts`, `SkillManager#validate/#import` (user scope, copy mode), `validateFromDisk`, `walkDir`, frontmatter parsing (YAML + compat fallback) |
| Rust target | `rust/crates/skills` (`jsjson`, `validate`, `migrate`), `rust/crates/cli/src/skills.rs` |
| Observables | exit code; stdout (text report or `JSON.stringify(report,null,2)`); stderr; **resulting filesystem tree** (paths, types, file modes, content hashes, symlink targets) |
| Side effects | `--execute` creates `<pilotHome>/skills/<slug>` copies (recursive, symlinks preserved but relative targets rewritten to absolute like Node `fs.cp`), removes the target first with `--overwrite`. Never writes outside the target root. Dry-run writes nothing. |
| Ordering | sources in fixed order then deduped by resolved path; skill dirs sorted by ICU `localeCompare` of basename (ASCII approximation implemented; see divergences); bundle walk in bytewise readdir order |
| Errors | unknown `--from` value → `Unknown skills source "<raw>". …`, exit 1; non-directory source root → `ENOTDIR: not a directory, scandir '<path>'`, exit 1; text mode exits 1 when `summary.error > 0`, JSON mode exits 0 (bug-compatible) |
| Numbers/JSON | JS key ordering (integer-like keys first), ECMAScript number formatting (`ryu-js`), `toFixed(1)` exact half-up |
| Normalizer | Node runtime ExperimentalWarning banners removed from oracle stderr. Nothing else. |
| Cases / evidence | 28 cases (dry-run, execute skip/rename/overwrite, self-import, custom sources incl. `~`, ENOTDIR crash, unknown source, flags) → `evidence/parity-skills-migrate.json`; negative controls: stdout mutation, tree mutation, wrong binary |
| Known divergences (open) | YAML parser is `serde_yaml` (YAML 1.2-ish) vs npm `yaml` v2 — exotic YAML (tags, anchors-as-keys, non-string keys, `014`-style ints) may differ in `--json` `frontmatter`; `localeCompare` only exact for ASCII names; Windows path handling not ported; mode/ownership copied only as file permission bits |
| Rollback | Pure additive copy into `~/.pilotdeck/skills`; TS CLI remains available via `PILOTDECK_TS_CLI` forwarding |
