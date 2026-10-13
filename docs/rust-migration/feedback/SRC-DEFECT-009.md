# SRC-DEFECT-009: removing provider "__proto__" reports success for a provider that does not exist

- Source: `ui/server/services/modelReferences.js` `planModelRemoval`: `config?.model?.providers?.[providerId]` with
  `providerId = "__proto__"` returns `Object.prototype` when there is no own key. `isRecord` accepts that, so the plan
  goes ahead:
  - it returns `blocked: null` and an unchanged config;
  - it would also repair any references whose provider is literally `__proto__`;
  - `delete providers.__proto__` does nothing.

  Expected: `NOT_FOUND`. Other `Object.prototype` names (`constructor`, `toString`, ...) are functions, so they get
  `NOT_FOUND` correctly.
- Reproduced by the hand-written cases in `rust/parity/ui_server/gen_model_references.mjs`.
- Rust treatment: **behaviour-compatible** (the `Provider::ObjectPrototype` branch). **Pending user decision**: change to `NOT_FOUND`.
- Low impact: the route that calls this needs an authenticated settings user, and nothing is written.
