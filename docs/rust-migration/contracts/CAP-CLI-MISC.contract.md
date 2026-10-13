# Contract CAP-CLI-MISC v1 (slice 4)

Covers the small CLI entries that need no gateway/agent runtime:

| Entry | TS source | Rust | Behaviour |
|---|---|---|---|
| `pilotdeck update [args]` | `pilotdeck.ts#handleUpdateCommand`, `runtime/updateCommand.ts` | `cli/src/update.rs` | Walk up from the install dir until `scripts/update-web.mjs` exists (TS: from `dist/src/runtime`; Rust: from the executable dir, overridable by `PILOTDECK_INSTALL_ROOT`), spawn `node <root>/scripts/update-web.mjs args…` with cwd=root, both child streams → stdout; exit 0 if child exits 0 or 2, else 1. No root → stderr `Update failed: This installation does not include the Git Web updater. …`, exit 1. The updater script stays a JS asset (UI-server owned, migrates with CAP-UI-SERVER). Node binary: `PILOTDECK_NODE` or `node` (TS: `process.execPath`). |
| `pilotdeck gateway` / `gateway <not setup>` | `pilotdeck.ts` | `main.rs` | stderr `Usage: pilotdeck gateway setup [feishu\|weixin\|wecom]`, exit 1 (`gateway setup` still forwarded; interactive + third-party QR APIs). |
| `pilotdeck tui` without TTY stdin | `pilotdeck.ts` | `main.rs` | stderr `pilotdeck tui requires an interactive terminal.`, exit 1 (TTY path still forwarded). |
| `pilotdeck help\|--help\|-h` | `sdkCli.ts#printHelp` | `main.rs` | verbatim help text, exit 0 |

Evidence: `evidence/parity-misc-cli.json` (10/10, negative control ok).
