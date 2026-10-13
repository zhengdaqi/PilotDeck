# CAP-TUI-APP: interactive TUI state machine (slice 11, sub-slice of CAP-CLI-TUI)

Source of truth: `src/adapters/channel/tui/app/TuiApp.tsx` (state, `useInput` handler, `handleSubmit`,
`handleCommand`, `switchToSession`, `openSidebar`, `openToolOutput`, `buildPermissionEntry`), `ToolOutputViewer.tsx`
(offset keys), `PromptInput.tsx` + `ink-text-input@6.0.0` (editing/cursor), `src/permission/settings.ts`
(PILOT_HOME/permissions.json read/normalize/write), and Ink 7.0.4 input semantics (`useInput` key derivation,
`exitOnCtrlC`, listener order).

Oracle: `rust/parity/tui/gen_tui_app_fixture.mjs` renders the **real dist TuiApp** with Ink 7 (`render` with fake
stdin/stdout, 100x30, `exitOnCtrlC: true` as in production `TuiChannel`) and a data-driven fake Gateway
(`tui_app_spec.json`). Bytes are written to stdin one step at a time (150 ms apart, so the 100 ms Ctrl+E/Ctrl+S
suppress window has always expired by the next step). After every step it records:
- the TuiApp `useState` value, the TextInput `{cursorOffset, cursorWidth}` and the viewer offset, read from the
  React fiber tree of the Ink instance;
- the `(input, key)` pairs Ink delivered (a probe `useInput` component), which are the Rust inputs;
- the Gateway call log (method + params), text written straight to stdout (short tool outputs), the
  permissions.json content (minus `lastUpdated`) and whether the app exited.
`--check` regenerates and diffs (stable: re-run gave an identical fixture).

Rust: crate `rust/crates/tui-app` (library, no terminal I/O): `App::handle(gateway, input, key)` runs one Ink input
event to quiescence; `TuiGateway` is the subset of the Gateway interface TuiApp uses; `settings` ports
permission settings with an explicit PILOT_HOME path. Test `tests/oracle.rs` replays the fixture against a Rust
twin of the fake Gateway: **30 scenarios, 492 steps, 3579 comparisons, 0 mismatches** (20 hand-written scenarios
covering every command, help/settings, sidebar navigation/grouping/switching, permission queue y/a/n/Esc,
viewer keys, scrolling, tool focus/expand, turn errors, initial bypass from a messy permissions.json, a broken
permissions.json; plus 8 seeded random key sequences).

Behaviour reproduced on purpose (TS quirks, not fixed):
- ink-text-input clamps using the *old* cursor, so the cursor reaches -1 / len+1 and a later insert uses
  `slice` negative-index rules (e.g. "hello" ← Z at -1 → "hellZo").
- A typed line pasted with its newline in one chunk does not submit (Ink treats it as one paste).
- Ctrl+E/Ctrl+S still move the TextInput cursor although the inserted character is suppressed.
- Opening/closing the sidebar or the viewer remounts PromptInput (cursor reset to end, listener re-registered
  after TuiApp's; a remounted input still handles the in-flight event on its discarded instance).
- Ctrl+C exits through Ink's `exitOnCtrlC` before TuiApp's own Ctrl+C branch (abort-while-running) can run; that
  branch is dead code in production and is ported but unreachable the same way.
- Synchronous updates inside one listener commit together (e.g. `/help` submitted while typing leaves the cursor
  unclamped because focus is lost in the same commit).

Modelled, not observed: async continuations are run to completion in program order within each event (the
oracle waits for quiescence). Interleavings that need a turn and a keypress to race are not covered.

Experimental front-end: crate `rust/crates/tui-term`, binary `pilotdeck-tui-rs` (crossterm 0.28; MIT). It drives the
state machine against a running Gateway server over WS (`Transport::stream_poll`, additive in gateway-client, so
keys stay live while a turn streams). Smoke-tested under a pty against the TS gateway server with the parity fake
gateway (type + submit, `/mode plan`, Ctrl+C exit). **Visual parity with the Ink UI is not claimed** (plain
layout; no logo, colours, markdown or welcome card). It is not wired into `pilotdeck tui`: production stays on the
TS Ink TUI until the user decides.

PENDING (not dropped):
- Ink visual layout parity (components, wrapping, colours); key decoding of raw terminal bytes in the Rust binary
  uses crossterm and is not differentially verified against Ink's `parseKeypress`.
- In-process gateway mode (`pilotdeck tui` without a server) and the remote/in-process probe.
- `onViewOutput` pager hand-off.
- Switching production `pilotdeck tui` to Rust (user decision).
