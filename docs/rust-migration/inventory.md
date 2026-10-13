# P01 inventory (full scope, names only; detailed contracts per slice)

Baseline: `standalone_alpha1` @ `b243fa00`. Line counts in `inventory-loc.json`.

## Size
| Area | Files | Lines | Kind |
|---|---|---|---|
| `src/` (31 modules) | ~1,000 .ts | ~180.6k | first-party runtime (CLI, gateway, agent loop, tools, sessions…) |
| `ui/src` + `ui/shared` | ~750 .ts/.tsx | ~135.8k | first-party browser UI (React/Vite) |
| `ui/server` | JS/MJS | ~49.5k | first-party runtime (Express server, bridge) — **JS, not TS, still in scope** |
| `packages/sdk` | 16 | ~15.2k | public npm SDK (`@pilotdeck/sdk`) |
| `apps/desktop` | 21 | ~3.2k | Electron desktop host |
| `src/context/memory/edgeclaw-memory-core` | — | (in src/context) | embedded package |
| `tests/` | 418 | ~97.6k | test oracle (read-only) |
| `scripts/`, `tools/` | — | ~5k | dev/build tooling (retain_development_only) |

## User capabilities / entry points
| ID | Entry | Notes |
|---|---|---|
| CAP-CLI-CHAT-SEARCH | `pilotdeck chat search` | **slice 1 (migrated)** |
| CAP-CLI-SDK | `pilotdeck run/resume/sessions/settings/help` | via `runSdkCli` → SDK |
| CAP-CLI-SERVER | `pilotdeck server` | gateway + HTTP/WS + automation + channels |
| CAP-CLI-GATEWAY-SETUP | `pilotdeck gateway setup feishu|weixin|wecom` | interactive, external services |
| CAP-CLI-CRON | `pilotdeck cron list/create/delete/stop` | persistent cron state |
| CAP-CLI-UPDATE | `pilotdeck update` | self-update (installer side effects) |
| CAP-CLI-SKILLS | `pilotdeck skills migrate` | filesystem migration, dry-run/execute |
| CAP-CLI-TUI | `pilotdeck tui` | Ink/React TTY UI |
| CAP-SIDECAR | `pilotdeck-agent-loop-sidecar` | process protocol |
| CAP-GATEWAY | `src/gateway` HTTP/WS protocol | external protocol, streaming |
| CAP-AGENT-LOOP | `src/agent`, `src/model`, `src/router` | LLM streaming, retries, compaction |
| CAP-TOOLS | `src/tool` (builtin tools, web search, shell, files) | side effects, sandbox |
| CAP-SESSION | `src/session` (JSONL transcripts, catalog, search, projections) | persisted data format |
| CAP-CONTEXT-MEMORY | `src/context` + edgeclaw-memory-core | persisted data, sqlite |
| CAP-MCP | `src/mcp` (@modelcontextprotocol/sdk clients) | external SDK |
| CAP-EXTENSIONS | `src/extension` (plugins, skills, builtin plugins) | **dynamic JS plugin loading** |
| CAP-CHANNELS | `src/adapters` (Feishu/Lark SDK, Weixin, WeCom, CLI/TUI) | external SDKs |
| CAP-ALWAYS-ON / CRON / SOP / WORKFLOW / TASK / GOAL / PLAN-TODO | respective `src/*` | state machines |
| CAP-SANDBOX / PERMISSION / NETWORK(proxy) | respective `src/*` | security-relevant |
| CAP-TELEMETRY / STATUS / LSP / WEB | respective `src/*` | |
| CAP-UI-SERVER | `ui/server` Express routes, sqlite DB, bridge | JS runtime logic |
| CAP-WEB-UI | `ui/src` | browser; route decision required (Wasm UI vs declared JS host) |
| CAP-DESKTOP | `apps/desktop` Electron | route decision required (Tauri/other vs declared host) |
| CAP-SDK | `packages/sdk` npm API | public JS API → crate + Node-API/Wasm or keep TS facade |

## Compatibility-risk table (must be resolved per capability)
- Dynamic `await import()` in 16 `src` files; builtin JS plugins (`src/extension/plugins/builtin`: browser-use, funasr, windows-skills) → JS plugin host question.
- Native/binary deps: `better-sqlite3`, `node:sqlite`, `node-pty`, `sharp`, `mupdf` (wasm), `@vscode/ripgrep`, `@playwright/mcp`.
- External SDKs: `@google/genai`, `@larksuiteoapi/node-sdk`, `@modelcontextprotocol/sdk`, `weixin-ilink`, `undici` proxy agent.
- Browser/Electron APIs in `ui/` and `apps/desktop`.
- `js-tiktoken`, `turndown`, `parse5`, `jsonrepair`, `ajv` semantics need Rust equivalents verified.
- ICU-dependent formatting (`toLocaleString`) — output depends on the Node ICU version (see slice-1 contract).

## Unknowns — RESOLVED by ADR-0002 (2026-10-13); kept for history
1. `js_runtime` final axis for the web UI and desktop (Rust Wasm UI vs keep React as a declared browser host).
2. Whether the public `@pilotdeck/sdk` npm API must remain a JS package (Node-API/Wasm binding) or may become a Rust crate + CLI.
3. JS plugin compatibility (keep a declared JS host vs a versioned new plugin protocol).
