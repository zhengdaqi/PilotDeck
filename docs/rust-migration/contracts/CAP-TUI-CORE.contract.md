# CAP-TUI-CORE: Rust TUI pure core (sub-slice of CAP-CLI-TUI)

Source of truth: `src/adapters/channel/tui/{tui-render.ts,TuiChannel.ts(defaultTuiSessionKey)}` and
`src/adapters/channel/tui/app/{types.ts(applyGatewayEventToTuiState),formatToolSummary.ts,groupConsecutiveTools.ts,sidebar-helpers.ts,truncate.ts}`.

Oracle: the dist build of the same modules, `string-width@8.2.1`, `slice-ansi@9.0.0`, `is-fullwidth-code-point` (slice-ansi's copy)
and `Intl.Segmenter` on Node 22.23.3 (ICU 78.3, Unicode 17.0). `rust/parity/tui/gen_tui_core_fixture.mjs` records a seeded
fixture and generates `src/tables.rs` (per-code-point stringWidth, fullwidth and Emoji_Presentation ranges) from the same
Node, so the tables are oracle output, not hand-written. `--check` regenerates and fails on drift.

Rust: crate `rust/crates/tui-core` (library; no terminal I/O). New third-party dependency: `unicode-segmentation` 1.13.3
(MIT OR Apache-2.0) for UAX#29 grapheme clusters. It is checked against `Intl.Segmenter` in the fixture.

Ported (behavior-compatible on the fixture domain):
- Gateway event reducer: all 11 event kinds, the activity cap of 8, thinking ids in UTF-16 length, JS `trim`, and
  `toolCallId` matching, including undefined === undefined.
- The non-interactive `TuiRenderState`.
- `formatToolSummary`: the regexes keep JS non-unicode `/i` semantics (ASCII case folding) and the UTF-16 80-unit slice.
- `groupConsecutiveTools`, including the JS-truthiness rule for an empty toolName.
- Sidebar:
  - grouping by project and by status (Map insertion order, stable sort)
  - smart collapse
  - row flattening
  - icons
  - titles (`||` chain, UTF-16 slice plus `…`)
  - `relativeTime`, with the clock injected
- `truncateForDisplay` and `countVisualLines`, including the TS quirks: the binary search is bounded by UTF-16 length,
  the search runs over a column index, and a wide character that straddles a wrap boundary is dropped by
  `sliceAnsi(remaining, best)`.

Domain limits. These are approximations and are recorded, not claimed as parity:
- Width and truncation are verified on ANSI-free text: ASCII, CJK, combining marks, ZWSP, tabs, halfwidth katakana and
  Hangul. Two kinds of input are not ported: ANSI SGR and hyperlink sequences (slice-ansi style tracking), and RGI emoji
  ZWJ or keycap sequence width rules in string-width.
- The gateway event fields the reducer reads are taken as strings or booleans, as the TS types declare. Non-string
  values are treated as absent.

PENDING (not dropped). The interactive Ink application is ported as a state machine in CAP-TUI-APP (slice 11). Still
not ported: the Ink components' visual layout, the pager hand-off, and the remote/in-process gateway probe. `pilotdeck tui` on a TTY is still forwarded to the TS entry.
The no-TTY path is native (CAP-CLI-MISC).
