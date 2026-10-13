// Oracle generator for ui/server/services/connectionTestTasks.js.
// Runs the real module with a mocked clock, interval, prepare/run and persist deferreds; writes event logs.
// Usage: node gen_connection_test_tasks.mjs [--check]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const out = path.join(root, "rust/crates/ui-server-core/tests/fixtures/connection_test_tasks.json");

let clock = 1_700_000_000_000;
Date.now = () => clock;
let tick = null;
globalThis.setInterval = (fn) => { tick = fn; return { unref() {} }; };
const { createConnectionTestTasks } = await import(path.join(root, "ui/server/services/connectionTestTasks.js"));

let seed = 0x5eed27;
function rnd(n) { seed |= 0; seed = (seed + 0x6d2b79f5) | 0; let t = Math.imul(seed ^ (seed >>> 15), 1 | seed); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return (((t ^ (t >>> 14)) >>> 0) / 4294967296 * n) | 0; }
const pick = (a) => a[rnd(a.length)];
const flush = async () => { for (let i = 0; i < 5; i++) await new Promise((r) => setImmediate(r)); };
const mkErr = (e) => Object.assign(new Error(e.message), e.status !== undefined ? { status: e.status } : {}, e.code !== undefined ? { code: e.code } : {});

async function script(ops) {
  clock = 1_700_000_000_000;
  const log = [];
  const runs = [];
  const persists = [];
  let current = true;
  let rec = { ok: true, reason: "gone" };
  const ids = [];
  const norm = (v) => JSON.parse(JSON.stringify(v ?? null, (k, x) => (k === "id" && typeof x === "string" ? (ids.includes(x) ? `T${ids.indexOf(x) + 1}` : (ids.push(x), `T${ids.length}`)) : x)));
  const tasks = createConnectionTestTasks({
    prepare: (body, userId, flag) => {
      log.push(`prepare ${userId} ${JSON.stringify(body)} ${flag}`);
      if (body.prepareThrow) throw mkErr(body.prepareThrow);
      return (signal) => { const d = {}; d.p = new Promise((res, rej) => { d.res = res; d.rej = rej; }); runs.push(d); log.push(`run ${runs.length}`); return d.p; };
    },
    persist: (userId, testId) => { const d = {}; d.p = new Promise((res, rej) => { d.res = res; d.rej = rej; }); persists.push(d); log.push(`persist ${persists.length} ${userId} ${JSON.stringify(testId) ?? "undefined"}`); return d.p; },
    getRecord: (userId, testId) => { log.push(`getRecord ${userId} ${JSON.stringify(testId) ?? "undefined"}`); return { record: rec.ok ? { testId } : null, reason: rec.reason }; },
    applyImage: (record, body) => ({ status: "passed", testId: record.testId, image: body.image }),
    isCurrent: () => current,
  });
  const started = [];
  for (const op of ops) {
    log.push(`> ${JSON.stringify(op)}`);
    const tid = (t) => (t >= 0 && t < started.length ? started[t] : "bogus");
    try {
      let r;
      switch (op.op) {
        case "start": r = tasks.start(op.user, op.body, op.modelId === undefined ? undefined : { modelId: op.modelId }); started.push(r.id); break;
        case "list": r = tasks.list(op.user); break;
        case "retry": r = tasks.retry(op.user, tid(op.t)); break;
        case "confirm": r = tasks.confirm(op.user, tid(op.t), op.body); break;
        case "acknowledge": r = tasks.acknowledge(op.user, tid(op.t)); break;
        case "cancel": r = tasks.cancel(op.user, tid(op.t)); break;
        case "runResolve": { const d = runs[op.k]; if (d && !d.done) { d.done = true; d.res(op.result); } break; }
        case "runReject": { const d = runs[op.k]; if (d && !d.done) { d.done = true; d.rej(mkErr(op.error)); } break; }
        case "persistResolve": { const d = persists[op.k]; if (d && !d.done) { d.done = true; d.res(); } break; }
        case "persistReject": { const d = persists[op.k]; if (d && !d.done) { d.done = true; d.rej(mkErr(op.error)); } break; }
        case "advance": clock += op.ms; break;
        case "tick": tick(); break;
        case "current": current = op.value; break;
        case "record": rec = { ok: op.ok, reason: op.reason }; break;
      }
      if (r !== undefined) log.push(`= ${JSON.stringify(norm(r))}`);
    } catch (e) {
      log.push(`throw ${JSON.stringify(e.message)} ${JSON.stringify(e.status) ?? "undefined"} ${JSON.stringify(e.code) ?? "undefined"}`);
    }
    await flush();
  }
  return log;
}

const RESULTS = [
  { status: "passed", testId: "t1" }, { status: "passed" }, { status: "failed", error: { message: "bad key", code: "AUTH" } },
  { status: "failed" }, { status: "failed", error: { message: "", code: "" } }, { manualInputRequired: true, testId: "t2" },
  { status: "partial", error: "str" }, null, {},
];
const ERRS = [{ message: "boom" }, { message: "exp", code: "TEST_EXPIRED" }, { message: "net", code: "NET" }, { message: "", code: "" }];
const USERS = ["u1", "u2"];
const cases = [];

// Fixed scenarios: each status path once.
const FIXED = [
  [{ op: "start", user: "u1", body: { providerId: "p" } }, { op: "runResolve", k: 0, result: { status: "passed", testId: "t1" } }, { op: "list", user: "u1" }, { op: "persistResolve", k: 0 }, { op: "list", user: "u1" }, { op: "current", value: false }, { op: "list", user: "u1" }],
  [{ op: "start", user: "u1", body: { providerId: "p" } }, { op: "runResolve", k: 0, result: { status: "passed", testId: "t1" } }, { op: "persistReject", k: 0, error: { message: "disk" } }, { op: "retry", user: "u1", t: 0 }, { op: "persistResolve", k: 1 }, { op: "list", user: "u1" }],
  [{ op: "start", user: "u1", body: { providerId: "p" } }, { op: "runResolve", k: 0, result: { manualInputRequired: true, testId: "t2" } }, { op: "confirm", user: "u1", t: 0, body: { image: true } }, { op: "persistResolve", k: 0 }, { op: "list", user: "u1" }],
  [{ op: "start", user: "u1", body: { providerId: "p" } }, { op: "runResolve", k: 0, result: { manualInputRequired: true, testId: "t2" } }, { op: "record", ok: false, reason: "expired!" }, { op: "confirm", user: "u1", t: 0, body: {} }, { op: "list", user: "u1" }],
  [{ op: "start", user: "u1", body: { providerId: "p" } }, { op: "cancel", user: "u1", t: 0 }, { op: "list", user: "u1" }, { op: "start", user: "u1", body: { providerId: "q" } }, { op: "runResolve", k: 0, result: { status: "passed" } }, { op: "list", user: "u1" }],
  [{ op: "start", user: "u1", body: { providerId: "p" }, modelId: "m" }, { op: "runResolve", k: 0, result: { status: "passed" } }, { op: "acknowledge", user: "u1", t: 0 }, { op: "list", user: "u1" }],
  [{ op: "start", user: "u1", body: { providerId: "p" }, modelId: "m" }, { op: "current", value: false }, { op: "runResolve", k: 0, result: { status: "passed" } }, { op: "list", user: "u1" }],
  [{ op: "start", user: "u1", body: { providerId: "p" } }, { op: "runResolve", k: 0, result: { manualInputRequired: true } }, { op: "advance", ms: 600_001 }, { op: "tick" }, { op: "list", user: "u1" }, { op: "advance", ms: 3_000_000 }, { op: "list", user: "u1" }, { op: "list", user: "u2" }],
  [{ op: "start", user: "u1", body: { providerId: "p" } }, { op: "runResolve", k: 0, result: { manualInputRequired: true } }, { op: "advance", ms: 600_000 }, { op: "tick" }, { op: "list", user: "u1" }, { op: "advance", ms: 1 }, { op: "list", user: "u1" }],
  [{ op: "start", user: "u1", body: { providerId: "p" } }, { op: "runResolve", k: 0, result: null }, { op: "list", user: "u1" }, { op: "start", user: "u1", body: {} }, { op: "runResolve", k: 1 }, { op: "list", user: "u1" }],
  [{ op: "start", user: "u1", body: { providerId: "p" } }, { op: "runResolve", k: 0, result: { status: "passed", testId: "t9" } }, { op: "persistReject", k: 0, error: { message: "exp", code: "TEST_EXPIRED" } }, { op: "list", user: "u1" }, { op: "retry", user: "u1", t: 0 }],
  [{ op: "start", user: "u1", body: { providerId: "p" } }, { op: "runResolve", k: 0, result: { status: "failed" } }, { op: "advance", ms: 2_400_000 }, { op: "list", user: "u1" }, { op: "advance", ms: 1_200_001 }, { op: "list", user: "u1" }],
];
for (const ops of FIXED) cases.push({ ops, log: await script(ops) });

for (let i = 0; i < 600; i++) {
  const ops = [];
  let starts = 0;
  for (let j = 0; j < 3 + rnd(18); j++) {
    const r = rnd(20);
    const user = rnd(4) ? "u1" : pick(USERS);
    const t = rnd(6) ? rnd(Math.max(1, starts)) : -1;
    if (r < 4) {
      const body = rnd(6) ? { providerId: pick(["p", "q", "p"]) } : {};
      if (!rnd(8)) body.prepareThrow = pick([{ message: "invalid", status: 400, code: "BAD" }, { message: "slot" }]);
      const op = { op: "start", user, body };
      if (!rnd(3)) op.modelId = pick(["m", "", "m2"]);
      ops.push(op); starts++;
    } else if (r < 7) ops.push({ op: "runResolve", k: rnd(Math.max(1, starts)), result: pick(RESULTS) });
    else if (r < 8) ops.push({ op: "runReject", k: rnd(Math.max(1, starts)), error: pick(ERRS) });
    else if (r < 10) ops.push({ op: "persistResolve", k: rnd(3) });
    else if (r < 11) ops.push({ op: "persistReject", k: rnd(3), error: pick(ERRS) });
    else if (r < 12) ops.push({ op: "list", user });
    else if (r < 13) ops.push({ op: "retry", user, t });
    else if (r < 14) ops.push({ op: "confirm", user, t, body: pick([{ image: true }, {}]) });
    else if (r < 15) ops.push({ op: "acknowledge", user, t });
    else if (r < 16) ops.push({ op: "cancel", user, t });
    else if (r < 17) ops.push({ op: "advance", ms: pick([1000, 600_001, 3_600_001, 59_000]) });
    else if (r < 18) ops.push({ op: "tick" });
    else if (r < 19) ops.push({ op: "current", value: rnd(2) === 0 });
    else ops.push({ op: "record", ok: rnd(2) === 0, reason: pick(["expired!", "gone"]) });
  }
  ops.push({ op: "list", user: "u1" }, { op: "list", user: "u2" });
  cases.push({ ops, log: await script(ops) });
}

const text = JSON.stringify({ cases }, null, 1) + "\n";
if (process.argv.includes("--check")) {
  console.log(fs.existsSync(out) && fs.readFileSync(out, "utf8") === text ? "fixture stable" : "fixture DIFFERS");
} else {
  fs.writeFileSync(out, text);
  console.log(`wrote ${cases.length} cases`);
}
