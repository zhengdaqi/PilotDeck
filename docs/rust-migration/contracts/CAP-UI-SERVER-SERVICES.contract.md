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
