# CAP-PLUGIN-HOST contract (ADR-0004: D4 amended — Node child-process host)

Source: plugin MCP spawn/install logic (project-scoped MCP spec patching, install command, stdio spawn).
Target: `rust/crates/plugin-host`. JS plugins are still run by Node (`PILOTDECK_NODE_EXECUTABLE` or `node` on PATH).
Oracle: `rust/parity/plugin_host/gen_fixture.mjs` (TS) -> 231/231. The fixture depends on the host (/tmp paths), so regenerate it on each machine.
TS TypeError on a non-string command/arg -> Rust `Err` (oracle `{__error}`).
Live: tests/spawn.rs starts the funasr MCP server as a Node subprocess; initialize + tools/list -> ["transcribe_audio"].
