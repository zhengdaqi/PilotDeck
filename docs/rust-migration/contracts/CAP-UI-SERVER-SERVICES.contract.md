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
