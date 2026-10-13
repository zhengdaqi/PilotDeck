# CAP-CLI-GATEWAY-SETUP/v1 — `pilotdeck gateway setup [feishu|weixin|wecom]`

Oracle: `src/cli/commands/gatewaySetup.ts` (+ the weixin-ilink `loginWithQR` helper and the top-level `installGlobalProxy()` banner from `src/cli/proxy.ts`).
Rust: `rust/crates/setup` (`gateway_setup.rs`, `yamlfmt.rs`, `http.rs`), wired in `rust/crates/cli/src/main.rs`.

## Observable contract
- stdout/stderr text byte-for-byte (prompts, banners, errors), exit code, and resulting files under `$HOME/.pilotdeck/` (`pilotdeck.yaml`, `weixin-credentials.json`).
- EOF at any prompt → exit 0, no trailing newline (Node readline behaviour). Thrown errors → message on stderr, exit 1.
- YAML read/write matches `yaml@2` (`parse`, `stringify({lineWidth:0, singleQuote:false})`), including non-object roots (JS semantics: array roots/`adapters` arrays absorb the property invisibly; scalar roots raise the strict-mode TypeError text).
- Network: fetch semantics with `PILOTDECK_PROXY`/`https_proxy`/…, `NO_PROXY` + loopback excluded; any transport failure → `fetch failed`.
- When an env proxy is set, every natively handled command prints `[proxy] Global fetch proxy → <url> (noProxy: <list>)` first, as the TS entry does.

## Normalization (parity harness)
ISO timestamps; Node `ExperimentalWarning` banner on stderr (runtime noise, same rule as CAP-CLI-CHAT-SEARCH).

## Known deviations
1. Piped stdin: Node readline drops lines that arrive while no question is pending; Rust keeps them for the next prompt. Parity feeds input paced line by line. Interactive (TTY) use is unaffected.
2. Feishu QR path: the bundled Lark SDK lacks `AppTicketManager`, so in TS the QR branch always falls back; Rust skips it directly (same output).

## Evidence
`evidence/parity-gateway-setup.json` (18/18), `evidence/yaml-parse-fuzz.txt` (0 mismatches, 3 seeds × 857 docs). Success paths of WeCom QR / Weixin login / Feishu token are not covered against live endpoints (dead-proxy failure paths only).
