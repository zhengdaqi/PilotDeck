# CAP-UI-SERVER-SERVICES contract: ui/server service layer, slice 1 (modelReferences)

Source: `ui/server/services/modelReferences.js`: `REFERENCE_ACTIONS`, `findModelReferences`, `planModelRemoval`,
`rewriteModelReferences`, `configuredModelIds`. Target: `rust/crates/ui-server-core` (pure).
JS behaviour kept:
- object key order: index-like keys first, then insertion order; `delete` keeps the order of the remaining keys;
- `delete arr[i]` leaves a hole, which JSON writes as null;
- strict-mode TypeErrors with V8's exact messages ("Cannot create property 'model' on string 'str'",
  "Cannot read properties of null (reading 'providerId')");
- `Map` last-wins;
- truthiness rules;
- provider lookup uses own keys only (SRC-DEFECT-009 fixed; intended difference, ADR-0004).
Oracle: `rust/parity/ui_server/gen_model_references.mjs` runs the real JS. It writes 8045 cases (mulberry32-seeded,
plus hand-written router/tokenSaver and prototype-key cases), compared byte for byte. Result: 8045/8045.
Mutation checks: 6 and 93 mismatches.
Not yet ported: the rest of ui/server, about 48k LOC, including index.js, pilotdeck-bridge.js, the routes, and the
other services. Next: commandParser, pilotdeckConfig, modelConfigurationState, spreadsheetPreview, then the HTTP
routes (the axum host is to be decided in the routes slice).
- commandParser (replaceArguments, isPathSafe, validateCommand via shell-quote port, sanitizeOutput, processFileIncludes) ported; SRC-DEFECT-010/011/012 fixed (intended, ADR-0004). parseCommand (gray-matter) and processBashCommands pending.
- frontmatter.js (gray-matter 4.0.3 + js-yaml 3.14.2 loader ported in js_yaml.rs), parseCommand, processBashCommands ported; SRC-DEFECT-013/014 fixed; YAML-GAP-001 closed (option a, 0 gaps).
- permissionSettings.js and mcpConfig.js ported (settings_files.rs, with Node fs messages and modes); pilotPaths.isVirtualProjectPath.
- slice 27: uploadLimits.readUploadLimits, gatewayConnectionCache (sans-IO `GatewayConnectionCache<G>` driven by effects: connect/timer/onConnected/onDisconnected/onInvalidated; same backoff, stale-attempt and promise-sharing rules), regenerateLastMessageTransaction (rollback, buffered writer, finalize). Oracle gen_ui_services.mjs + tests/ui_services_oracle.rs (300/404/400 cases); evidence ui-server-services-cache-regenerate.txt. Not ported yet: real timers/async host glue (done in the routes slice), connectionTestTasks.
- slice 28: connectionTestTasks.js → `connection_test_tasks.rs`, sans-IO (`ConnectionTestHost` supplies clock, ids, prepare/run/persist tokens, getRecord, applyImage, isCurrent; `run_settled` / `persist_settled` / `tick` feed async completions). Keeps JS task key order, the V8 TypeError text for `result.status` on null/undefined, and the 10-min/60-min prune rules. Oracle gen_connection_test_tasks.mjs, 612/612; evidence ui-server-connection-test-tasks.txt. The real AbortController/timer wiring is done in the routes slice.
- slice 30: terminalSessionRegistry.js + runtimeCoordination.js → `terminal_runtime.rs` (PTY, timers and supervisor IPC behind traits; `WsSlot` keeps the JS undefined/null distinction). Typed boundary: keys are `&str` and the constructor takes numbers, so the JS TypeErrors for non-string keys / non-number options cannot arise (callers cannot express them); every other path matches. Oracle gen_terminal_runtime.mjs 709 + 300 scripts; evidence ui-server-terminal-runtime.txt.
- slice 34: src/model/providerEndpoint.ts (chat/models endpoint candidates over WHATWG URL path segments, Google model aliases, response-shape checks), src/network/fetch.ts `normalizeNetworkError` (code/cause + message substring rules) and ui/server/services/modelConnectionProbe.js → `model_probe.rs`. The probe is sans-IO and async: the host passes `fetch`. Kept JS details: `(x || []).some` / `for…of` V8 TypeErrors become ENDPOINT_UNREACHABLE results, `String(v)` coercions, ASCII-only `/i` folding, `.` excludes line terminators, `.{0,60}` counts UTF-16 code units, and the `<think>` backreference matches case-insensitively. Intended difference: the PNG's IDAT deflate stream comes from miniz_oxide rather than Node zlib; header, IEND, CRCs and the decoded pixels are identical. Pending (host slice): the networkFetch retry/backoff loop (retryPolicy; the Rust probe makes one attempt per URL, which equals maxRetries 0), the 60 s timeout, and caller abort forwarding (the host must surface a timeout as a `network_timeout` failure). Oracle gen_model_probe.mjs; evidence ui-server-model-probe.txt.
- slice 36: src/network/fetch.ts `networkFetch` / `networkFetchJson` / `jitteredBackoff` / `isRetryableNetworkCode` → `network_fetch.rs` (sans-IO `FetchHost`: fetch, sleep, `Math.random`, `Date.now`). Kept: fractional or NaN maxRetries semantics (including the final `normalizeNetworkError(undefined)` → "Network request failed."), method gating on the uppercased method while the request carries the caller's method unchanged, Retry-After seconds (JS `Number`, round half up) before dates, jitter only without Retry-After, configured retryStatuses, and the 429/5xx/other codes and UTF-16 500-unit message slices. Host requirements: Retry-After is passed as Fetch `Headers.get` returns it (HTTP whitespace stripped). Gaps (recorded): only IMF-fixdate Retry-After dates (V8's lenient `Date.parse` forms fall back to backoff); a surrogate pair split by the 500-unit slice becomes U+FFFD (lone-surrogate gap); abort/timeout signals and wiring the probe through this loop are done in the host slice. Oracle gen_network_fetch.mjs (TZ=UTC); evidence ui-server-network-fetch.txt.
- slice 37: webUpdateCommand.js → `web_update_command.rs` (sans-IO `UpdateHost`: fetch with a streaming `BodyStream`, clock, sleep, uuid, realpath, `localHeaders`, offline service). Kept: SERVER_PORT via JS `Number` (hex, exponent, whitespace), ECONNREFUSED-only offline fallback, request error reason/message fallbacks (including the TypeError for a `null` detail), the NDJSON progress stream through a WHATWG streaming UTF-8 decoder (split sequences, U+FFFD, a trailing partial line is ignored), the early `return` from for-await rejecting when the body had already failed (`BodyStream::cancel`), recovery polling against the clock deadline, restart confirmation, and `describeUpdateReason` including Object.prototype member names. Host requirements: `localHeaders` (SQLite + JWT signing) and `createWebUpdateService` stay host-side. Typed boundary: a JSON SyntaxError's V8 text is not reproduced (`message: None`; only the error kind is part of the contract). Oracle gen_web_update_command.mjs; evidence ui-server-web-update-command.txt.
- slice 38: releaseService.js → `release_service.rs` (normalizeRepository, releaseVersion including `Date.UTC`'s 0–99 → 1900+ year mapping for leap days, compareVersions with the safe-integer bound, getLatestRelease manifest and asset validation, sha256 lowercasing, spread-order asset output, Set identity on raw name values, the TypeError for a `null` asset). The host performs the GET (headers and 15 s timeout are the request contract). Typed boundary: a JSON SyntaxError's V8 text is not reproduced. Also fixes the slice-37 clippy warning (same condition). Oracle gen_release_service.mjs; evidence ui-server-release-service.txt.
- slice 39 (pilotdeckConfig dependencies): `config_parsers.rs` ports PROVIDER_CATALOG + lookupCatalogProvider, providerCredentialScope (default URL, catalog API-key env var gated on protocol and canonical WHATWG URL), parseThinkingSettings, parseGatewayConfig, parseAdaptersConfig and parseToolsConfig (diagnostics in TS order). The catalog is a generated snapshot (`src/provider_catalog.json`) that gen_config_parsers.mjs --check proves identical to the TS module. Object.prototype provider ids map to "no catalog entry" (all their catalog fields are undefined in TS). Not yet ported: lookupCatalogModel (needed by the model runtime, not by pilotdeckConfig). Oracle gen_config_parsers.mjs; evidence ui-server-config-parsers.txt.

## pilotdeckConfig (pure part) — slice 40 (`ui-server-core/src/pilotdeck_config.rs`)

- Ported: buildDefaultPilotDeckConfig, normalizePilotDeckConfig (deepMerge, implicit `enabled: true` for configured
  memory/router/tools.webSearch, officePreview migration, spreadsheetMode removal), sanitizeProviderCredentials,
  resolveModel, validatePilotDeckConfig (providers, agent/memory/subagent refs, router scenarios/fallback/baseline/
  pricing/tokenSaver, gateway+tools diagnostics, contextWindow deprecation, officePreview), maskSecrets,
  preserveMaskedSecrets, hasUnresolvedMaskedSecrets, resolveConfiguredProviderUrl/ApiKey, buildRuntimeEnv,
  buildMemoryLlmOptions, buildMemoryDefaults, configRevision (sha256), expandTilde (POSIX path.join), syncAgentModelWithRouter.
- `process.env` / `os.homedir()` / `PILOT_HOME` are explicit (`PilotDeckEnv`); the JS module captures PILOT_HOME at load.
- JS exceptions keep V8 text: `Cannot read properties of null (reading 'officePreview'|'model')`,
  `Cannot set properties of undefined (setting 'service')`, `Cannot create property 'service' on number '5'`.
- Typed boundaries (documented, not generated): `__proto__` keys; array-valued `webui.officePreview` (JS sets a
  property on the array; outside the JSON domain, `unreachable!`); providers given as arrays/strings resolve by index
  in JS and are treated as "not a record" here (identical outcome: index values are never records in YAML input).
- Pending: YAML read/write half (readPilotDeckConfigFile, serializePilotDeckConfigResponse, updatePilotDeckConfig,
  writePilotDeckConfig, atomic writes, finalizeBootstrapPlaceholder, purgeLegacyProviderDrafts, configToYaml,
  rawYamlToMaskedString, parseConfigYaml).

## pilotdeckConfig write-side pipeline — slice 41 (`ui-server-core/src/pilotdeck_config_write.rs`)

- Ported: purgeLegacyProviderDrafts, finalizeBootstrapPlaceholder, the in-memory part of writePilotDeckConfig
  (`prepare_pilotdeck_config_write`: shallow copy → sanitize → purge → syncAgentModelWithRouter → finalize → blank
  memory.model removal → validate → YAML text), serializePilotDeckConfigResponse, configToYaml,
  rawYamlToMaskedString, detectYamlIndent, valueAtPath.
- YAML text: `pilotdeck_setup::yamlfmt::stringify` (the existing fuzz-verified yaml@2 `stringify` emulation;
  ui-server-core now depends on pilotdeck-setup). Byte equality with the file the real writePilotDeckConfig wrote
  is part of the oracle.
- Host contract: when `previousConfig` is not a record, the host passes the disk config (defaults when the file is
  missing; nothing when it failed to parse, JS then skips purging). Atomic replace, mode preservation, symlink
  resolution, revision check and fsync stay host-side (pending slice).
- Typed boundary: valueAtPath with key "length" on arrays (JS returns the length) is not reproduced.
- Pending: readPilotDeckConfigFile (exact yaml@2 parse + error text), atomicWritePilotDeckYaml,
  resolveConfigWritePath, readStablePilotDeckConfigFile, updatePilotDeckConfig (comment-preserving
  parseDocument/setIn/deleteIn editing), withPilotDeckConfigWrite queue.
