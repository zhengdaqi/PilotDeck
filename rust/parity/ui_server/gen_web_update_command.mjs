// Oracle for ui/server/services/webUpdateCommand.js: createCommandClient (port check, health, offline fallback,
// context check, request errors), apply (NDJSON progress stream incl. split UTF-8 and invalid bytes, recovery
// polling with a mocked clock), restart, runWebUpdateCommand and describeUpdateReason.
// Usage: node gen_web_update_command.mjs [--check]   (uses /tmp/pd-wu-root as projectRoot)
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const out = path.join(root, "rust/crates/ui-server-core/tests/fixtures/web_update_command.json");
fs.mkdirSync("/tmp/pd-wu-root", { recursive: true });
const PROJECT = fs.realpathSync("/tmp/pd-wu-root");
const mod = await import(path.join(root, "ui/server/services/webUpdateCommand.js"));

let seed = 3737;
function rnd(n) { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; }
const pick = (a) => a[rnd(a.length)];
let now = 0;
Date.now = () => now;
let log = [];
const realSetTimeout = globalThis.setTimeout;
globalThis.setTimeout = (fn, ms) => { log.push(`sleep ${ms}`); now += clockStep(); queueMicrotask(fn); return 0; };
let clockStep = () => 1000;

const REASONS = ["upToDate", "development", "ahead", "lockBusy", "nope", "", "constructor", "toString", "__proto__", "hasOwnProperty", "valueOf", 5, null];
const reasonsOut = REASONS.map((r) => ({ r, out: mod.describeUpdateReason(r) }));

const enc = new TextEncoder();
function chunked(bytes) {
  const parts = [];
  let i = 0;
  while (i < bytes.length) { const n = 1 + rnd(9); parts.push(Array.from(bytes.slice(i, i + n))); i += n; }
  return parts;
}
function eventsBody() {
  const lines = [];
  const n = rnd(5);
  for (let i = 0; i < n; i++) lines.push(JSON.stringify(pick([{ message: "Downloading…" }, { stage: "fetch", message: "fetching ✓" }, { message: "" }, { message: 5 }, { message: { a: 1 } }])));
  const end = pick(["complete", "error", "errorNoReason", "none", "junk", "null", "blank", "noNewline", "badbytes"]);
  if (end === "complete") lines.push(JSON.stringify({ stage: "complete", status: "success", message: "Done" }));
  if (end === "error") lines.push(JSON.stringify({ status: "error", reason: pick(["diverged", "lockBusy"]), message: "nope ✗" }));
  if (end === "errorNoReason") lines.push(JSON.stringify(pick([{ status: "error", message: "bad" }, { status: "error" }])));
  if (end === "junk") lines.push("{not json");
  if (end === "null") lines.push("null");
  if (end === "blank") lines.push("   ", JSON.stringify({ stage: "complete", status: "success" }));
  let text = lines.join(pick(["\n", "\r\n"])) + (end === "noNewline" ? "\n" + JSON.stringify({ stage: "complete", status: "success" }) : "\n");
  let bytes = Array.from(enc.encode(text));
  if (end === "badbytes") bytes = [...Array.from(enc.encode('{"message":"a')), 0xff, 0xe2, 0x82, ...Array.from(enc.encode('b"}\n')), ...Array.from(enc.encode(JSON.stringify({ stage: "complete", status: "success" }) + "\n"))];
  return { chunks: chunked(bytes), streamError: rnd(6) === 0 };
}
const json = (v) => ({ status: 200, chunks: [Array.from(enc.encode(JSON.stringify(v)))] });
function scenario() {
  const s = { env: {}, routes: {} };
  const portPick = rnd(4) ? pick([undefined, "8080"]) : pick([undefined, undefined, "", "3001", "8080", " 8080 ", "0x1F90", "1e3", "0", "70000", "abc", "8080.5"]);
  if (portPick !== undefined) s.env.SERVER_PORT = portPick;
  s.health = rnd(3) ? { status: 200 } : pick([{ status: 200 }, { status: 200 }, { status: 200 }, { status: 503 }, { throw: { message: "fetch failed", causeCode: "ECONNREFUSED" } }, { throw: { message: "fetch failed", causeCode: "ECONNRESET" } }]);
  const latest = rnd(4) ? { tagName: "v1.2.3", sourceSha: "0123456789abcdef" } : pick([{ tagName: "v2", sourceSha: "abc" }, null, { tagName: 7, sourceSha: "deadbeefcafe" }]);
  const checkStatus = rnd(3) ? { canUpdate: true, latest } : pick([{ reason: "upToDate" }, { canUpdate: true, latest }, { canUpdate: true, latest }, { canUpdate: true, latest }, { canUpdate: false, reason: pick(REASONS) }, { canUpdate: false }]);
  s.routes.context = [rnd(3) ? json({ projectRoot: PROJECT }) : pick([json({ projectRoot: PROJECT }), json({ projectRoot: PROJECT }), json({ projectRoot: "/elsewhere" }), { status: 200, chunks: [[0xef, 0xbb, 0xbf, ...Array.from(enc.encode(JSON.stringify({ projectRoot: PROJECT })))]] }, { status: 401, chunks: [Array.from(enc.encode(JSON.stringify(pick([{ reason: "authenticationRequired", message: "login" }, { error: "nope" }, {}, null, 5]))))] }, { status: 500, chunks: [Array.from(enc.encode("not json"))] }])];
  s.routes.check = [pick([json(checkStatus), json(checkStatus), { status: 503, chunks: [Array.from(enc.encode('{"reason":"lockBusy","message":"busy"}'))] }])];
  s.routes.apply = [pick([{ status: 200, ...eventsBody() }, { status: 200, ...eventsBody() }, { status: 409, chunks: [Array.from(enc.encode('{"reason":"lockBusy","message":"held"}'))] }, { throw: { message: "socket hang up" } }])];
  s.routes.status = Array.from({ length: 1 + rnd(4) }, () => json(pick([
    { updateInProgress: true, currentUpdateId: "$ID" },
    { lastUpdateResult: { updateId: "$ID", success: true } },
    { lastUpdateResult: { updateId: "$ID", success: false, reason: pick(["diverged", undefined]) } },
    { lastUpdateResult: { updateId: "other" }, updateInProgress: true, currentUpdateId: "$ID" },
    { updateInProgress: false },
    { updateInProgress: false, currentUpdateId: "$ID" },
    { updateInProgress: true, currentUpdateId: "other" },
  ])));
  s.routes.restart = [pick([json({ status: "accepted" }), json({ status: "rejected" }), { status: 500, chunks: [] }])];
  s.offline = { check: checkStatus, applyThrows: rnd(4) === 0 };
  s.checkOnly = rnd(4) === 0;
  s.restart = rnd(2) === 0;
  s.clockStep = pick([1000, 1000, 1800000, 3600000]);
  return s;
}
const cases = [];
for (let i = 0; i < 3000; i++) {
  const s = scenario();
  log = []; now = 1_000_000; clockStep = () => s.clockStep;
  const queues = Object.fromEntries(Object.entries(s.routes).map(([k, v]) => [k, [...v]]));
  let updateId = null;
  const sub = (v) => JSON.parse(JSON.stringify(v).split('"$ID"').join(JSON.stringify(updateId)));
  const fetchImpl = async (url, init) => {
    const route = url.replace(/^http:\/\/127\.0\.0\.1:\d+\//, "");
    let body = init.body;
    if (body && route === "api/update/apply") { updateId = JSON.parse(body).updateId; body = body.split(updateId).join("$ID"); }
    log.push(`fetch ${url} ${init.method ?? "-"} ${init.headers ? JSON.stringify(init.headers) : "-"} ${body ?? "-"} ${init.redirect}`);
    let r;
    if (route === "health") r = s.health;
    else {
      const q = queues[route.replace("api/update/", "")];
      r = q && q.length ? q.shift() : { status: 500, chunks: [Array.from(enc.encode("{}"))] };
      if (r.chunks && route === "api/update/status") r = { ...r, chunks: [Array.from(enc.encode(JSON.stringify(sub(JSON.parse(new TextDecoder().decode(new Uint8Array(r.chunks[0])))))))] };
    }
    if (r.throw) throw Object.assign(new Error(r.throw.message), r.throw.causeCode ? { cause: { code: r.throw.causeCode } } : {});
    const chunks = r.chunks ?? [];
    let k = 0;
    const stream = new ReadableStream({
      pull(c) {
        if (k < chunks.length) c.enqueue(new Uint8Array(chunks[k++]));
        else if (r.streamError) c.error(new Error("stream reset"));
        else c.close();
      },
    });
    return new Response(stream, { status: r.status });
  };
  const offline = () => {
    log.push("offline");
    return {
      check: async () => { log.push("offline.check"); return s.offline.check; },
      apply: async (target, output) => { log.push(`offline.apply ${JSON.stringify(target)}`); output("offline applying"); if (s.offline.applyThrows) throw Object.assign(new Error("offline failed"), { reason: "applyFailed" }); },
    };
  };
  const output = (m) => log.push(`out ${JSON.stringify(m)}`);
  let result;
  try {
    const client = await mod.createCommandClient({ projectRoot: "/tmp/pd-wu-root", env: s.env, fetchImpl, headers: async () => ({ Authorization: "Bearer T" }), offline });
    result = { code: await mod.runWebUpdateCommand({ checkOnly: s.checkOnly, restart: s.restart, client, output }) };
  } catch (e) {
    result = e.reason !== undefined ? { reason: e.reason, message: e.message } : { plain: e.name, message: e.name === "SyntaxError" ? null : e.message };
  }
  cases.push({ s, project: PROJECT, log, result });
}
const text = JSON.stringify({ reasons: reasonsOut, cases }) + "\n";
if (process.argv.includes("--check")) console.log(fs.existsSync(out) && fs.readFileSync(out, "utf8") === text ? "fixture stable" : "fixture DIFFERS");
else { fs.writeFileSync(out, text); console.log(`cases ${cases.length}`); }
