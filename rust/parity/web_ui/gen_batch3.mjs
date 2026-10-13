// UI batch 3 vectors from the real TS: reloadOnChunkError, frameBatcher, uploadFormData (postFormDataWithProgress
// driven by a fake XMLHttpRequest), desktopUpdates. `IS_PLATFORM` is replaced by a constant in a temporary copy
// (the config module reads Vite env); both values are recorded.
// Usage: node --experimental-strip-types rust/parity/web_ui/gen_batch3.mjs [--check]
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const out = path.join(root, "rust/crates/web-ui-core/tests/fixtures/batch3.json");
const u = path.join(root, "ui/src/utils");
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "pd-batch3-"));
const copy = (name, edit = (s) => s, as = name) => { fs.writeFileSync(path.join(tmp, as), edit(fs.readFileSync(path.join(u, name), "utf8"))); };
copy("reloadOnChunkError.ts");
copy("frameBatcher.ts");
copy("desktopUpdates.ts");
for (const flag of [true, false]) copy("uploadFormData.ts", (s) => s.replace("import { IS_PLATFORM } from '../constants/config';", `const IS_PLATFORM = ${flag};`), `uploadFormData.${flag}.ts`);
const imp = (n) => import(pathToFileURL(path.join(tmp, n)).href);
const chunk = await imp("reloadOnChunkError.ts"), frame = await imp("frameBatcher.ts"), desk = await imp("desktopUpdates.ts");
const uploads = { true: await imp("uploadFormData.true.ts"), false: await imp("uploadFormData.false.ts") };

let seed = 333;
const rnd = (n) => { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; };
const pick = (a) => a[rnd(a.length)];
const v = { chunk: [], reload: [], frame: [], progress: [], response: [], desktop: [] };

// errors: {"$error":{name,message}} marks an Error instance; {"$undef":1} undefined.
const MSGS = ["Failed to fetch dynamically imported module: /a.js", "failed to FETCH dynamically imported module", "Importing a module script failed.", "error loading dynamically imported module",
  "Loading chunk 42 failed.", "loading chunk  failed", "Loading chunk abc-1.js failed", "Loading chunk x\tfailed", "Loading chunk x\u00a0failed", "loading chunk ab failedX", "ChunkLoadError: x", "chunkloaderror",
  "Unable to preload CSS for /x.css", "unable to preload css", "network error", "", "loading chunk", "Loading CSS chunk 3 failed", "ſailed to fetch dynamically imported module", "\u212Aaboom"];
const mkErr = (name, message) => { const e = new Error(message); e.name = name; return e; };
const enc = (e) => e === undefined ? { $undef: 1 } : e instanceof Error ? { $error: { name: e.name, message: e.message } } : e;
for (let i = 0; i < 1500; i++) {
  const m = pick(MSGS);
  const e = pick([() => mkErr(pick(["Error", "TypeError", "ChunkLoadError", "SyntaxError"]), m), () => m, () => ({ message: m }), () => ({ message: 5 }), () => ({}), () => null, () => undefined, () => 0, () => 42, () => true, () => [m, "x"], () => ({ name: "ChunkLoadError" }), () => mkErr("ChunkLoadError", "")])();
  v.chunk.push({ e: enc(e), out: chunk.isDynamicImportLoadError(e) });
}
for (let i = 0; i < 400; i++) {
  const e = pick([mkErr("Error", pick(MSGS)), pick(MSGS), null]);
  const st = { marker: pick([null, "1", "0", ""]), getThrows: rnd(5) === 0, setThrows: rnd(5) === 0, removeThrows: rnd(4) === 0 };
  const log = [];
  const store = new Map(st.marker === null ? [] : [["pilotdeck:chunk-load-reloaded", st.marker]]);
  const win = {
    sessionStorage: {
      getItem: (k) => { log.push(`get ${k}`); if (st.getThrows) throw new Error("x"); return store.has(k) ? store.get(k) : null; },
      setItem: (k, val) => { log.push(`set ${k} ${val}`); if (st.setThrows) throw new Error("x"); store.set(k, val); },
      removeItem: (k) => { log.push(`remove ${k}`); if (st.removeThrows) throw new Error("x"); store.delete(k); },
    },
    location: { reload: () => log.push("reload") },
  };
  const r1 = chunk.reloadOnceForDynamicImportError(e, win);
  const r2 = chunk.reloadOnceForDynamicImportError(e, win);
  chunk.clearDynamicImportReloadMarker(win);
  const r3 = chunk.reloadOnceForDynamicImportError(e, win);
  v.reload.push({ e: enc(e), st, out: [r1, r2, r3], log });
}
// frameBatcher scripts
for (let i = 0; i < 400; i++) {
  const log = [];
  let seq = 0;
  const frames = new Map();
  globalThis.requestAnimationFrame = (fn) => { const id = ++seq; frames.set(id, fn); log.push(`raf ${id}`); return id; };
  globalThis.cancelAnimationFrame = (id) => { frames.delete(id); log.push(`caf ${id}`); };
  const b = frame.createFrameBatcher((x) => log.push(`apply ${JSON.stringify(x)}`));
  const ops = [];
  for (let j = 0; j < 2 + rnd(14); j++) ops.push(pick([{ op: "schedule", v: rnd(100) }, { op: "schedule", v: null }, { op: "flush" }, { op: "cancel" }, { op: "frame", id: 1 + rnd(Math.max(1, seq + 1)) }]));
  for (const op of ops) {
    log.push(`> ${JSON.stringify(op)}`);
    if (op.op === "schedule") b.schedule(op.v);
    else if (op.op === "flush") b.flush();
    else if (op.op === "cancel") b.cancel();
    else { const fn = frames.get(op.id); if (fn) { frames.delete(op.id); fn(0); } }
  }
  v.frame.push({ ops, log });
}
// postFormDataWithProgress with a fake XHR / localStorage
class FakeXHR { constructor() { this.upload = {}; this.headers = {}; FakeXHR.last = this; } open(m, u) { this.m = m; this.u = u; } setRequestHeader(k, val) { this.headers[k] = val; } send() { } abort() { this.aborted = true; this.onabort?.(); } getResponseHeader(k) { return this.respHeaders?.[k] ?? null; } }
globalThis.XMLHttpRequest = FakeXHR;
for (let i = 0; i < 1200; i++) {
  const known = pick([0, 100, 1000, -5, 3]);
  const events = Array.from({ length: 1 + rnd(4) }, () => ({ lengthComputable: rnd(3) > 0, loaded: pick([0, 1, 33, 50, 99, 100, 150, 1000, 7]), total: pick([0, 100, 1000, 3, -1, 7, 20000, 40000]) }));
  const status = pick([200, 201, 299, 300, 404, 500, 0, 207]);
  const text = pick(["", '{"success":true}', "[1,2]", "5", "null", "not json", '"s"', '{"a":{"b":[]}}']);
  const token = pick([null, "", "tok"]);
  const isPlatform = rnd(2) === 0;
  const stored = pick([null, "", "abc"]);
  const upload = uploads[isPlatform];
  const onload = rnd(2) === 0, transferredCb = rnd(2) === 0;
  const ls = new Map(stored === null ? [] : [["auth-token", stored]]);
  globalThis.localStorage = { getItem: (k) => (ls.has(k) ? ls.get(k) : null), setItem: (k, val) => ls.set(k, String(val)) };
  const log = [];
  const ac = new AbortController();
  const p = upload.postFormDataWithProgress({ url: "/u", formData: {}, signal: ac.signal, knownTotalBytes: known, onProgress: (a, b2, c) => log.push(`progress ${a} ${b2} ${c}`), onTransferred: transferredCb ? () => log.push("transferred") : undefined });
  const x = FakeXHR.last;
  log.push(`auth ${x.headers.Authorization ?? "none"}`);
  for (const ev of events) x.upload.onprogress(ev);
  if (onload) x.upload.onload();
  x.status = status; x.responseText = text; x.respHeaders = token === null ? {} : { "X-Refreshed-Token": token };
  x.onload();
  const r = await p;
  log.push(`resolve ${JSON.stringify(r)}`, `token ${ls.get("auth-token") ?? "none"}`);
  v.progress.push({ known, events, onload, transferredCb, status, text, token, isPlatform, stored, log });
}
// desktopUpdates bridge check
for (const keys of [[], ["startUpdate"], ["startUpdate", "checkUpdates", "getUpdateStatus"], ["startUpdate", "checkUpdates", "getUpdateStatus", "cancelUpdate"], ["checkUpdates", "getUpdateStatus", "cancelUpdate"], null]) {
  globalThis.window = { pilotdeckDesktop: keys === null ? undefined : Object.fromEntries(keys.map((k) => [k, () => {}])) };
  let out;
  try { desk.desktopUpdates(); out = "ok"; } catch (e) { out = e.message; }
  v.desktop.push({ keys, out });
}
fs.rmSync(tmp, { recursive: true, force: true });
const text = JSON.stringify(v) + "\n";
if (process.argv.includes("--check")) console.log(fs.existsSync(out) && fs.readFileSync(out, "utf8") === text ? "fixture stable" : "fixture DIFFERS");
else { fs.writeFileSync(out, text); console.log(`wrote chunk ${v.chunk.length}, reload ${v.reload.length}, frame ${v.frame.length}, upload ${v.progress.length}, desktop ${v.desktop.length}`); }
