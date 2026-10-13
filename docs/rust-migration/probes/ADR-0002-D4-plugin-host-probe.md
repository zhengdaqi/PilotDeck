# ADR-0002 D4 probe: JS plugin host engine (static analysis of the builtin plugins)

ADR-0002 D4: "Keep an embedded JS runtime host for plugins … engine choice (rquickjs / boa / deno_core) to be
probed against the builtin plugins (browser-use, funasr, windows-skills)."

Findings (source: `src/extension/plugins/builtin/*`, `src/mcp/runtime/projectMcpSpec.ts`):
- **windows-skills** has no JS. It contributes skills (Markdown) only.
- **funasr** contributes an MCP server whose `command` is the marker `__PILOTDECK_NODE_EXECUTABLE__`, which
  `projectMcpSpec.ts` replaces with `process.execPath`. The entry `funasr-local-mcp.mjs` imports
  `node:child_process`, `node:fs/promises`, `node:readline` and `node:path`. It is a stdio MCP server process.
- **browser-use** contributes an MCP server that runs `@playwright/mcp` (an npm package) with the same Node
  marker. Playwright needs Node's `net`, `child_process`, streams and fs, and it launches Chromium.
- No builtin plugin executes JS **in-process**. Plugin hooks are configuration (shell commands), not JS
  callbacks.

Consequence: an embedded engine without Node APIs (`rquickjs`, `boa`, plain `deno_core`) cannot run either
JS-bearing builtin plugin. The options that keep these plugins working are:
1. Keep spawning a real Node executable as a **subprocess** host. The Rust side resolves
   `__PILOTDECK_NODE_EXECUTABLE__` to a bundled or system Node. This changes nothing for plugins, but it is a
   process host rather than an embedded runtime.
2. Embed a Node-compatible runtime (the Deno runtime with its Node compatibility layer). This makes the binary
   much larger, and `@playwright/mcp` compatibility is unverified.
3. Embed a small engine for *future* in-process plugins, and still use option 1 for MCP-server plugins.

Needed from the user: D4 says "embedded", but the evidence shows the current plugins need a full Node as a
subprocess. Please choose 1, 2 or 3, or amend D4 through a new ADR. Until then, plugin MCP servers keep using
`process.execPath` of the TS runtime, which is unchanged because production stays on TS.
