// JS facade over the wasm build: same names and async shape as packages/sdk/src/session-store.ts.
import { readFileSync } from "node:fs";
import { initSync, WasmSessionStore, foldSessionSummary as foldRaw, summaryToSessionInfo as infoRaw, importBatchSize } from "./pkg/pilotdeck_sdk_wasm.js";

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
