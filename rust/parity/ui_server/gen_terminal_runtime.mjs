// Oracle for ui/server/services/terminalSessionRegistry.js and runtimeCoordination.js.
// Scripted ops with fake PTYs/sockets/timers/process; every side effect and return value is logged.
// Usage: node --experimental-transform-types rust/parity/ui_server/gen_terminal_runtime.mjs [--check]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const out = path.join(root, "rust/crates/ui-server-core/tests/fixtures/terminal_runtime.json");
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
const { TerminalSessionRegistry } = await import(path.join(root, "ui/server/services/terminalSessionRegistry.js"));
const { createRuntimeCoordination } = await import(path.join(root, "ui/server/services/runtimeCoordination.js"));

let seed = 3030;
function rnd(n) { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; }
const pick = (a) => a[rnd(a.length)];

// ---- terminal registry
// PTY kinds: full | nowrite | noresize | nokill | throwkill
const PTY_KINDS = ["full", "full", "full", "nowrite", "noresize", "nokill", "throwkill"];
function terminalScript(cfg, ptyKinds, ops) {
  const log = [];
  const ptys = ptyKinds.map((kind, i) => {
    const id = `p${i + 1}`;
    const p = { id };
    if (kind !== "nowrite") p.write = (d) => log.push(`write ${id} ${JSON.stringify(d)}`);
    if (kind !== "noresize") p.resize = (c, r) => log.push(`resize ${id} ${c} ${r}`);
    if (kind === "throwkill") p.kill = () => { log.push(`kill ${id} (throws)`); throw new Error("x"); };
    else if (kind !== "nokill") p.kill = () => log.push(`kill ${id}`);
    return p;
  });
  const wss = [{ id: "w1" }, { id: "w2" }, { id: "w3" }];
  const ref = (v) => (v === undefined ? "undefined" : v === null ? "null" : typeof v === "object" && v.id ? v.id : JSON.stringify(v));
  const pty = (i) => (i === -1 ? null : i === -2 ? undefined : ptys[i]);
  const ws = (i) => (i === -1 ? null : i === -2 ? undefined : wss[i]);
  const snap = (s) => (s === null ? "null" : JSON.stringify({ ...s, pty: ref(s.pty), ws: ref(s.ws) }));
  let timerSeq = 0;
  const timers = new Map();
  let reg;
  try {
    reg = new TerminalSessionRegistry({
      ...(cfg.timeoutMs === undefined ? {} : { timeoutMs: cfg.timeoutMs }),
      ...(cfg.maxBufferEntries === undefined ? {} : { maxBufferEntries: cfg.maxBufferEntries }),
      setTimeoutFn: (fn, ms) => { const id = ++timerSeq; timers.set(id, fn); log.push(`setTimer ${id} ${ms}`); return id; },
      clearTimeoutFn: (id) => { timers.delete(id); log.push(`clearTimer ${id}`); },
    });
  } catch (e) { log.push(`ctor ${e.constructor.name}: ${e.message}`); return log; }
  for (const op of ops) {
    log.push(`> ${JSON.stringify(op)}`);
    try {
      let r;
      switch (op.op) {
        case "register": r = snap(reg.register(op.key, { pty: pty(op.pty), ws: ws(op.ws), projectPath: op.projectPath, sessionId: op.sessionId })); break;
        case "replace": r = reg.replace(op.key); break;
        case "reconnect": r = snap(reg.reconnect(op.key, pty(op.pty), ws(op.ws))); break;
        case "append": r = snap(reg.appendOutput(op.key, pty(op.pty), op.chunk)); break;
        case "complete": r = snap(reg.complete(op.key, pty(op.pty))); break;
        case "write": r = reg.write(op.key, pty(op.pty), ws(op.ws), op.data); break;
        case "resize": r = reg.resize(op.key, pty(op.pty), ws(op.ws), op.cols, op.rows); break;
        case "detach": r = snap(reg.detach(op.key, pty(op.pty), ws(op.ws))); break;
        case "get": r = snap(reg.get(op.key)); break;
        case "size": r = reg.size; break;
        case "dispose": reg.dispose(); break;
        case "fire": { const fn = timers.get(op.id); if (fn) { timers.delete(op.id); fn(); } break; }
      }
      if (r !== undefined) log.push(`= ${r}`);
    } catch (e) { log.push(`throw ${e.constructor.name}: ${e.message}`); }
  }
  return log;
}

const terminal = [];
const CFGS = [{}, { timeoutMs: 0 }, { timeoutMs: 50, maxBufferEntries: 2 }, { maxBufferEntries: 1 }, { timeoutMs: -1 }, { timeoutMs: Infinity }, { timeoutMs: NaN }, { maxBufferEntries: 0 }, { maxBufferEntries: 1.5 }];
const FIXED_TERMINAL = [
  [{}, { maxBufferEntries: 2 }, { maxBufferEntries: 1 }],
  [{ op: "register", key: "a", pty: 0, ws: 0 }, { op: "append", key: "a", pty: 0, chunk: "1" }, { op: "append", key: "a", pty: 0, chunk: "2" }, { op: "append", key: "a", pty: 0, chunk: "3" }, { op: "get", key: "a" }],
  [{ op: "register", key: "a", pty: 0, ws: 0 }, { op: "detach", key: "a", pty: 0, ws: 0 }, { op: "reconnect", key: "a", pty: 0, ws: 1 }, { op: "fire", id: 1 }, { op: "get", key: "a" }, { op: "detach", key: "a", pty: 0, ws: 1 }, { op: "fire", id: 2 }, { op: "get", key: "a" }],
  [{ op: "register", key: "a", pty: 0, ws: 0 }, { op: "detach", key: "a", pty: 0, ws: 0 }, { op: "dispose" }, { op: "reconnect", key: "a", pty: 0, ws: 1 }, { op: "register", key: "b", pty: 1, ws: 0 }, { op: "size" }],
];
const encCfg = (c) => Object.fromEntries(Object.entries(c).map(([k, v]) => [k, typeof v === "number" && !Number.isFinite(v) ? { $num: String(v) } : v]));
for (let i = 0; i < 700; i++) {
  const cfg = i < CFGS.length ? CFGS[i] : (rnd(8) ? pick(CFGS.slice(0, 4)) : pick(CFGS));
  const ptyKinds = Array.from({ length: 4 }, () => pick(PTY_KINDS));
  const ops = [];
  let timersMade = 0;
  for (let j = 0; j < 4 + rnd(22); j++) {
    const key = pick(["a", "a", "b", ""]); // Rust keys are typed strings: non-string keys are not representable
    const p = rnd(10) ? rnd(4) : pick([-1, -2]);
    const w = rnd(6) ? rnd(3) : pick([-1, -2]);
    const r = rnd(24);
    if (r < 5) ops.push({ op: "register", key, pty: p, ws: w, ...(rnd(3) ? { projectPath: pick(["/p", ""]) } : {}), ...(rnd(3) ? { sessionId: pick(["s1", null]) } : {}) });
    else if (r < 6) ops.push({ op: "replace", key });
    else if (r < 8) ops.push({ op: "reconnect", key, pty: p, ws: w });
    else if (r < 11) ops.push({ op: "append", key, pty: p, chunk: pick(["x", "yz", ""]) });
    else if (r < 12) ops.push({ op: "complete", key, pty: p });
    else if (r < 14) ops.push({ op: "write", key, pty: p, ws: w, data: pick(["ls\n", ""]) });
    else if (r < 15) ops.push({ op: "resize", key, pty: p, ws: w, cols: 80, rows: pick([24, 0]) });
    else if (r < 18) { ops.push({ op: "detach", key, pty: p, ws: w }); timersMade++; }
    else if (r < 19) ops.push({ op: "get", key });
    else if (r < 20) ops.push({ op: "size" });
    else if (r < 21 && !rnd(3)) ops.push({ op: "dispose" });
    else ops.push({ op: "fire", id: 1 + rnd(Math.max(1, timersMade + 1)) });
  }
  terminal.push({ cfg: encCfg(cfg), ptys: ptyKinds, ops, log: terminalScript(cfg, ptyKinds, ops) });
}

for (const cfg of FIXED_TERMINAL[0]) for (const ops of FIXED_TERMINAL.slice(1)) {
  const ptyKinds = ["full", "full", "full", "full"];
  terminal.push({ cfg: encCfg(cfg), ptys: ptyKinds, ops, log: terminalScript(cfg, ptyKinds, ops) });
}

// ---- runtime coordination
function runtimeScript(cfg, ops) {
  const log = [];
  const handlers = [];
  const processLike = {};
  if (cfg.send) processLike.send = (m) => { log.push(`send ${JSON.stringify(m)}`); if (processLike.__throw) throw new Error("x"); };
  if (cfg.connected !== undefined) processLike.connected = cfg.connected;
  if (cfg.on) processLike.on = (ev, fn) => { log.push(`on ${ev}`); handlers.push(fn); };
  if (cfg.off) processLike.off = (ev, fn) => { log.push(`off ${ev} ${handlers.includes(fn)}`); };
  let n = 0;
  const rc = createRuntimeCoordination({
    processLike,
    env: cfg.env,
    readConfigurationState: () => { n++; return { state: "ready", n }; },
  });
  for (const op of ops) {
    log.push(`> ${JSON.stringify(op)}`);
    let r;
    switch (op.op) {
      case "message": for (const h of handlers) h(op.message); break;
      case "gateway": r = rc.getGatewayState(); break;
      case "configuration": r = rc.getConfigurationState(); break;
      case "publish": r = rc.publishConfigurationState(); break;
      case "retry": r = rc.requestGatewayRetry(); break;
      case "dispose": rc.dispose(); break;
      case "connected": processLike.connected = op.value; break;
      case "throw": processLike.__throw = op.value; break;
    }
    if (r !== undefined) log.push(`= ${JSON.stringify(r)}`);
  }
  return log;
}
const runtime = [];
const MESSAGES = [
  { type: "pilotdeck:gateway-state", state: "ready" }, { type: "pilotdeck:gateway-state", state: "error", error: "boom" },
  { type: "pilotdeck:gateway-state", state: "error", error: "" }, { type: "pilotdeck:gateway-state", state: "error", error: 5 },
  { type: "pilotdeck:gateway-state", state: "bogus" }, { type: "other", state: "ready" }, null, "str", { type: "pilotdeck:gateway-state", state: "starting", error: "e" },
  { type: "pilotdeck:gateway-state", state: "stopped" },
];
for (let i = 0; i < 300; i++) {
  const cfg = { send: rnd(5) > 0, on: rnd(4) > 0, off: rnd(3) > 0, env: rnd(5) ? { PILOTDECK_RUNTIME_SUPERVISED: pick(["1", "1", "0", "true"]) } : {}, ...(rnd(3) ? {} : { connected: pick([true, false, 0]) }) };
  const ops = [];
  for (let j = 0; j < 3 + rnd(12); j++) {
    const r = rnd(9);
    if (r < 3) ops.push({ op: "message", message: pick(MESSAGES) });
    else if (r < 4) ops.push({ op: "gateway" });
    else if (r < 5) ops.push({ op: "configuration" });
    else if (r < 6) ops.push({ op: "publish" });
    else if (r < 7) ops.push({ op: "retry" });
    else if (r < 8) ops.push(pick([{ op: "dispose" }, { op: "connected", value: rnd(2) === 0 }, { op: "throw", value: rnd(2) === 0 }]));
    else ops.push({ op: "gateway" });
  }
  runtime.push({ cfg, ops, log: runtimeScript(cfg, ops) });
}

const text = JSON.stringify({ terminal, runtime }, null, 0).replace(/\},\{"(cfg)"/g, '},\n{"$1"') + "\n";
if (process.argv.includes("--check")) {
  console.log(fs.existsSync(out) && fs.readFileSync(out, "utf8") === text ? "fixture stable" : "fixture DIFFERS");
} else {
  fs.writeFileSync(out, text);
  console.log(`wrote ${terminal.length} terminal, ${runtime.length} runtime cases`);
}
