// JS facade over the wasm build: same names and async shape as packages/sdk/src/session-store.ts.
import { readFileSync } from "node:fs";
import { PilotDeckError as PilotDeckErrorClass } from "./transport.js";
import { initSync, validateOptionsJson, WasmSessionStore, foldSessionSummary as foldRaw, summaryToSessionInfo as infoRaw, importBatchSize } from "./pkg/pilotdeck_sdk_wasm.js";

initSync({ module: readFileSync(new URL("./pkg/pilotdeck_sdk_wasm_bg.wasm", import.meta.url)) });
const j = (v) => JSON.stringify(v ?? null);

export class InMemorySessionStore {
  #s = new WasmSessionStore();
  async load(key) { return JSON.parse(this.#s.load(j(key))); }
  async append(key, entries) { if (entries.length === 0) return; this.#s.append(j(key), j(entries), Date.now()); }
  async listSessions(projectKey) { return JSON.parse(this.#s.listSessions(projectKey)); }
  async listSessionSummaries(projectKey) { return JSON.parse(this.#s.listSessionSummaries(projectKey)); }
  async listSubkeys(key) { return JSON.parse(this.#s.listSubkeys(key.projectKey, key.sessionId)); }
  async delete(key) { this.#s.delete(j(key)); }
}

export function foldSessionSummary(previous, key, entries, options = {}) {
  return JSON.parse(foldRaw(j(previous), j(key), j(entries), options.mtime));
}

export function summaryToSessionInfo(summary) { return JSON.parse(infoRaw(j(summary))); }

export async function importSessionToStore(sessionId, store, entries, options) {
  const size = importBatchSize(options.batchSize);
  const key = { projectKey: options.projectKey, sessionId, subpath: options.subpath };
  for (let offset = 0; offset < entries.length; offset += size) {
    await store.append(key, entries.slice(offset, offset + size));
  }
}

export { GatewayTransport, AsyncEventQueue, PilotDeckError, mapError } from "./transport.js";

// validateOptions: functions, undefined members and non-finite numbers do not survive JSON, so they are
// marked ({"$fn":1}, {"$undef":1}, {"$num":"NaN"|"Infinity"|"-Infinity"}) before crossing into wasm.
export function encodeOptions(v) {
  if (v === undefined) return { $undef: 1 };
  if (typeof v === "function") return { $fn: 1 };
  if (typeof v === "number" && !Number.isFinite(v)) return { $num: String(v) };
  if (Array.isArray(v)) return Array.from(v, encodeOptions);
  if (v && typeof v === "object") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, encodeOptions(x)]));
  return v;
}

export function validateOptions(options) {
  const r = validateOptionsJson(JSON.stringify(encodeOptions(options)));
  if (!r) return;
  const { code, message } = JSON.parse(r);
  if (code === "TypeError") throw new TypeError(message);
  throw new PilotDeckErrorClass({ code, message });
}
