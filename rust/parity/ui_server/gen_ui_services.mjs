// Oracle for ui/server/services/{uploadLimits,gatewayConnectionCache,regenerateLastMessage}.js.
// - uploadLimits: random env maps.
// - gatewayConnectionCache: scripted ops with deferred connect() and fake timers; logs every callback.
// - regenerateLastMessage: scripted host callbacks; logs every call, frame and the result.
// Usage: node --experimental-transform-types rust/parity/ui_server/gen_ui_services.mjs [--check]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
// ui/server imports src/*.js paths that exist only as .ts sources: resolve them to .ts (Node 22 strips types).
const { register } = await import("node:module");
register("data:text/javascript," + encodeURIComponent(`
import fs from "node:fs"; import { fileURLToPath } from "node:url";
export async function resolve(spec, ctx, next) {
  try { return await next(spec, ctx); } catch (e) {
    if (!spec.endsWith(".js") || !ctx.parentURL) throw e;
    const u = new URL(spec.slice(0, -3) + ".ts", ctx.parentURL);
    if (!fs.existsSync(fileURLToPath(u))) throw e;
    return next(u.href, ctx);
  }
}`));
const { readUploadLimits } = await import(path.join(root, "ui/server/services/uploadLimits.js"));
const { createGatewayConnectionCache } = await import(path.join(root, "ui/server/services/gatewayConnectionCache.js"));
const { regenerateLastMessageTransaction } = await import(path.join(root, "ui/server/services/regenerateLastMessage.js"));

let seed = 2626;
const rnd = (n) => { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; };
const pick = (a) => a[rnd(a.length)];
const flush = () => new Promise((r) => setImmediate(r));

// uploadLimits
const names = ["PILOTDECK_UPLOAD_MAX_FILE_BYTES", "PILOTDECK_UPLOAD_MAX_TASK_BYTES", "PILOTDECK_UPLOAD_MAX_FILES", "PILOTDECK_UPLOAD_MAX_CONCURRENT", "PILOTDECK_UPLOAD_RETENTION_MS"];
const envVals = ["1", "10", "0", "-5", "1.5", "  42 ", "0x10", "1e3", "abc", "", "9007199254740991", "9007199254740992", "Infinity", "700", "+7", "0b11", "1_000", ".5e1"];
const upload = [];
for (let i = 0; i < 300; i++) {
  const env = {};
  for (const n of names) if (rnd(2)) env[n] = pick(envVals);
  upload.push({ env, out: readUploadLimits(env) });
}

// gatewayConnectionCache
async function cacheScript(ops, cfg) {
  const log = [];
  const connects = [];
  const handlers = new Map();
  let timer = null;
  let should = cfg.should;
  const cache = createGatewayConnectionCache({
    connect: () => { const d = {}; d.p = new Promise((res, rej) => { d.res = res; d.rej = rej; }); connects.push(d); log.push(`connect ${connects.length}`); return d.p; },
    onConnected: (g) => log.push(`connected ${g.id}`),
    onDisconnected: (_e, g) => log.push(`disconnected ${g.id}`),
    onInvalidated: (g) => log.push(`invalidated ${g.id}`),
    shouldReconnect: () => should,
    reconnectBaseDelayMs: cfg.base, reconnectMaxDelayMs: cfg.max,
    setTimeoutFn: (fn, delay) => { log.push(`setTimer ${delay}`); timer = fn; return { unref() {} }; },
    clearTimeoutFn: () => { log.push("clearTimer"); timer = null; },
  });
  const promises = [];
  const gateways = new Map();
  for (const op of ops) {
    log.push(`> ${JSON.stringify(op)}`);
    if (op.op === "get") { const p = cache.get(); p.catch(() => {}); let k = promises.indexOf(p); if (k < 0) { promises.push(p); k = promises.length - 1; } log.push(`get #${k}`); }
    else if (op.op === "resolve") { const d = connects[(op.n === 0 ? connects.length : op.n) - 1]; if (d && !d.done) { d.done = true; const g = { id: op.gid, onDisconnect(cb) { handlers.set(op.gid, cb); } }; gateways.set(op.gid, g); d.res(g); } }
    else if (op.op === "reject") { const d = connects[(op.n === 0 ? connects.length : op.n) - 1]; if (d && !d.done) { d.done = true; d.rej(new Error("no")); } }
    else if (op.op === "disconnect") { const h = handlers.get(op.gid); if (h) h(new Error("gone")); }
    else if (op.op === "invalidate") log.push(`invalidate ${cache.invalidate(op.gid ? gateways.get(op.gid) ?? { id: op.gid } : undefined)}`);
    else if (op.op === "fire") { if (timer) { const t = timer; timer = null; t(); } }
    else if (op.op === "should") should = op.value;
    for (let i = 0; i < 4; i++) await flush();
    log.push(`current ${cache.current()?.id ?? null}`);
  }
  return log;
}
const cache = [];
// Fixed scenarios: backoff growth, reset after a success, and the max cap.
const FIXED_CACHE = [
  [{ op: "get" }, { op: "reject", n: 0 }, { op: "fire" }, { op: "reject", n: 0 }, { op: "fire" }, { op: "resolve", n: 0, gid: "g1" }, { op: "disconnect", gid: "g1" }, { op: "fire" }, { op: "reject", n: 0 }],
  [{ op: "get" }, { op: "reject", n: 0 }, { op: "fire" }, { op: "reject", n: 0 }, { op: "fire" }, { op: "reject", n: 0 }, { op: "fire" }, { op: "reject", n: 0 }, { op: "fire" }, { op: "reject", n: 0 }, { op: "fire" }, { op: "reject", n: 0 }],
];
for (const ops of FIXED_CACHE) for (const cfg of [{ should: true, base: 10, max: 1000 }, { should: true, base: 250, max: 1000 }]) cache.push({ cfg, ops, log: await cacheScript(ops, cfg) });
for (let i = 0; i < 400; i++) {
  const cfg = { should: rnd(3) > 0, base: pick([1000, 10, 250]), max: pick([30000, 50, 1000]) };
  const ops = [];
  let gid = 0;
  for (let j = 0; j < 4 + rnd(24); j++) {
    const r = rnd(10);
    if (r < 3) ops.push({ op: "get" });
    else if (r < 5) ops.push({ op: "resolve", n: rnd(2) ? 0 : 1 + rnd(4), gid: `g${++gid}` }); // n 0: the latest connect
    else if (r < 6) ops.push({ op: "reject", n: rnd(2) ? 0 : 1 + rnd(4) });
    else if (r < 7) ops.push({ op: "disconnect", gid: `g${1 + rnd(Math.max(1, gid))}` });
    else if (r < 8) ops.push({ op: "invalidate", gid: rnd(2) ? `g${1 + rnd(Math.max(1, gid))}` : null });
    else if (r < 9) ops.push({ op: "fire" });
    else ops.push({ op: "should", value: rnd(2) === 0 });
  }
  cache.push({ cfg, ops, log: await cacheScript(ops, cfg) });
}

// regenerateLastMessage
async function regen(script) {
  const log = [];
  const thrown = (t) => (t.kind === "err" ? new Error(t.msg) : t.value);
  const result = await regenerateLastMessageTransaction({
    data: script.data, sessionId: script.sessionId, requestId: script.requestId, expectedTurnId: script.expectedTurnId, provider: script.provider,
    writer: { send: (f) => log.push(["writer", f]) },
    streamWriter: { send: (f) => log.push(["stream", f]) },
    replaceLastTurn: async (sid, tid, opts) => { log.push(["replace", sid, tid, opts]); if (script.replace.throw) throw thrown(script.replace.throw); return script.replace.value; },
    finalizeLastTurnReplacement: async (sid, tx, action, opts) => { log.push(["finalize", sid, tx ?? null, action, opts]); if (script.finalizeThrows) throw new Error("fin"); },
    runChat: async (command, options, sw, provider, { onInputAccepted }) => {
      log.push(["run", command ?? null, options ?? null, provider]);
      for (const a of script.run.actions) {
        if (a.send !== undefined) sw.send({ frame: a.send });
        else if (a.accept) onInputAccepted();
        else if (a.throw) throw thrown(a.throw);
      }
      return script.run.ret;
    },
  });
  console.error = () => {};
  log.push(["result", result]);
  return JSON.parse(JSON.stringify(log));
}
const regenCases = [];
for (let i = 0; i < 400; i++) {
  const options = rnd(6) === 0 ? pick([undefined, null]) : {};
  if (options) {
    if (rnd(5)) options.runId = pick(["r1", " r2 ", "", "  ", 5]);
    if (rnd(2)) options.userVisibleInput = pick(["shown", null, ""]);
    if (rnd(2)) options.images = pick([["i1"], [], null]);
    if (rnd(2)) options.displayAttachments = pick([["d"], null, []]);
    if (rnd(2)) options.attachments = pick([["a"], []]);
  }
  const data = { ...(rnd(4) ? { command: pick(["cmd", "", null]) } : {}), ...(options === undefined ? {} : { options }) };
  const script = {
    data, sessionId: pick(["s1", "s1", "", null]), requestId: pick(["q1", null]), expectedTurnId: pick(["t1", "t1", "", null]), provider: pick(["pilotdeck", "x"]),
    replace: rnd(6) === 0 ? { throw: pick([{ kind: "err", msg: "rep fail" }, { kind: "val", value: "str thrown" }, { kind: "val", value: 7 }]) } : { value: pick([{ replacedTurnId: "rt", transactionId: "tx" }, { transactionId: "tx2" }]) },
    finalizeThrows: rnd(4) === 0,
    run: { actions: Array.from({ length: rnd(5) }, () => pick([{ send: rnd(100) }, { send: rnd(100) }, { accept: true }, { throw: { kind: "err", msg: "run fail" } }, { throw: { kind: "val", value: null } }])), ret: pick([{ inputAccepted: true }, { inputAccepted: false }, {}, null, { inputAccepted: 1 }]) },
  };
  regenCases.push({ script, log: await regen(script) });
}

const target = path.join(root, "rust/crates/ui-server-core/tests/fixtures/ui_services.json");
const text = JSON.stringify({ upload, cache, regen: regenCases }) + "\n";
if (process.argv.includes("--check")) { const same = fs.readFileSync(target, "utf8") === text; console.log(same ? "fixture stable" : "fixture DIFFERS"); process.exit(same ? 0 : 1); }
fs.writeFileSync(target, text);
console.log(`wrote ${upload.length} upload, ${cache.length} cache, ${regenCases.length} regenerate cases`);
