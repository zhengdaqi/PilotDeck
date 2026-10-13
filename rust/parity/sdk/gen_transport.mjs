// TS oracle for packages/sdk/dist/transport.js GatewayTransport (frame handling, close mapping, send failures),
// driven through a mock WebSocket. Each op's observable effects are logged in a canonical order:
// sent frames, request settlements, stream items/ends, notifications.
// Usage: node rust/parity/sdk/gen_transport.mjs (needs packages/sdk dist: pnpm run build) [--check]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const { GatewayTransport } = await import(path.join(root, "packages/sdk/dist/transport.js"));

let seed = 1515;
const rnd = (n) => { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; };
const pick = (a) => a[rnd(a.length)];
const tick = () => new Promise((r) => setImmediate(r));

let current;
class MockSocket {
  constructor(url) { this.url = url; this.readyState = 0; this.l = {}; this.sent = []; current = this; setImmediate(() => { this.readyState = 1; this.emit("open", {}); }); }
  addEventListener(n, h) { (this.l[n] ||= []).push(h); }
  send(d) { this.sent.push(d); }
  close() { this.readyState = 3; }
  emit(n, ev) { for (const h of this.l[n] || []) h(ev); }
}
globalThis.WebSocket = MockSocket;
const errOf = (e) => ({ name: e?.name, code: e?.code, message: e?.message, retryable: e?.retryable, ...(e?.details === undefined ? {} : { details: e.details }), ...(e?.requestId === undefined ? {} : { requestId: e.requestId }) });

async function run(script) {
  const t = new GatewayTransport({ url: "ws://127.0.0.1:1/ws", token: "tok", requestTimeoutMs: 1e9, connectTimeoutMs: 1e9 });
  const ids = []; // index -> uuid
  const norm = (v) => { let s = JSON.stringify(v); ids.forEach((u, i) => { if (u) s = s.split(u).join(`$id${i}`); }); return JSON.parse(s); };
  let log = [];
  const logs = [];
  const p = t.connect();
  await tick(); await tick();
  const sock = current;
  log.push({ sent: JSON.parse(sock.sent.shift()) });
  sock.emit("message", { data: JSON.stringify({ type: "hello_ok", protocolVersion: "1.2", serverVersion: "9", serverInfo: { mode: "local", projectKey: "pk", capabilities: ["a"] } }) });
  log.push({ connected: await p });
  t.onNotification((n) => log.push({ notif: n }));
  logs.push({ op: "connect", log: norm(log) });
  for (const op of script) {
    log = [];
    try {
      if (op.op === "request" || op.op === "stream") {
        const n = ids.length;
        if (op.op === "request") {
          t.request(op.method, op.params).then((v) => log.push({ req: n, ok: v === undefined ? "__undefined" : v }), (e) => log.push({ req: n, err: errOf(e) }));
        } else {
          (async () => { try { for await (const it of t.stream(op.method, op.params)) log.push({ stream: n, item: it }); log.push({ stream: n, end: true }); } catch (e) { log.push({ stream: n, err: errOf(e) }); } })();
        }
        const f = sock.sent.length ? JSON.parse(sock.sent.shift()) : null;
        ids.push(f?.id);
        if (f) log.push({ sent: f });
      } else if (op.op === "frame") {
        let raw = op.raw; ids.forEach((u, i) => { if (u) raw = raw.split(`$id${i}`).join(u); });
        try { sock.emit("message", { data: raw }); } catch { log.push({ crash: true }); }
      } else if (op.op === "serverClose") { sock.readyState = 3; sock.emit("close", { code: op.code, reason: op.reason }); }
      else if (op.op === "clientClose") t.close();
    } catch (e) { log.push({ threw: String(e?.message) }); }
    for (let i = 0; i < 6; i++) await tick();
    logs.push({ op: op.op, log: norm(canon(log)) });
  }
  t.close();
  for (let i = 0; i < 6; i++) await tick();
  return logs;
}
const rank = (e) => ("sent" in e ? 0 : "crash" in e ? 1 : "req" in e ? 2 : "stream" in e ? 3 : 4);
function canon(log) {
  return log.map((e, i) => [e, i]).sort((a, b) => rank(a[0]) - rank(b[0]) || ((a[0].req ?? a[0].stream ?? 0) - (b[0].req ?? b[0].stream ?? 0)) || a[1] - b[1]).map((x) => x[0]);
}

// Random scripts (no frames that crash TS; those are fixed cases below).
const codes = [undefined, "SESSION_NOT_FOUND", "session_busy", "Capability_Unavailable", "invalid_x", "INVALID_PERMISSION_MODE", "", "timeout", "weird_code", "project_not_found"];
const messages = [undefined, "boom", "", null, 42, true, [1, null, "x"], { a: 1 }, 1e21, 0.1];
const payloads = [{ a: 1 }, { "2": "two", b: 1, "1": "one" }, { sequence: "mine", z: 0 }, "ab", [7, 8], null, 5, true, {}];
function script() {
  const ops = []; const kinds = []; const nextSeq = [];
  const nOps = 4 + rnd(20);
  for (let i = 0; i < nOps; i++) {
    const r = rnd(40);
    if (r < 6 || kinds.length === 0) { const k = pick(["request", "stream"]); kinds.push(k); nextSeq.push(0); ops.push({ op: k, method: pick(["sessions.list", "turn.submit", "x"]), params: pick([{}, { a: [1, 2] }, null, "p"]) }); continue; }
    const n = rnd(8) === 0 ? kinds.length : rnd(kinds.length); // == length: unknown id
    const idRef = n === kinds.length ? JSON.stringify(pick(["nope", 5, null])) : JSON.stringify(`$id${n}`);
    if (r < 18) {
      const ok = pick([true, true, true, false, 1, 0, "", "y", null]);
      const fr = { type: "response" };
      let raw = `{"type":"response","id":${idRef},"ok":${JSON.stringify(ok)}`;
      if (rnd(3)) raw += `,"result":${JSON.stringify(pick([{ x: 1 }, [1], "r", 0, null]))}`;
      if (!ok) { const e = {}; const c = pick(codes); if (c !== undefined) e.code = c; const m = pick(messages); if (m !== undefined) e.message = m; if (rnd(3) === 0) e.details = pick([{ d: 1 }, "d", null]); raw += `,"error":${JSON.stringify(e)}`; }
      ops.push({ op: "frame", raw: raw + "}" });
    } else if (r < 32) {
      let seq = n < kinds.length ? nextSeq[n] : 0;
      const sv = rnd(24);
      if (sv === 0) seq = seq + 1; else if (sv === 1 && seq > 0) seq = seq - 1; else if (sv === 2) seq = pick([-1, 1.5, "1", null]);
      else if (n < kinds.length) nextSeq[n] += 1;
      const ev = { type: "event", seq, final: pick([false, false, false, true, 1, 0, "x"]) };
      const p = pick(payloads); const evRaw = rnd(8) === 0 ? "" : `,"event":${JSON.stringify(p)}`;
      const seqRaw = sv === 3 ? "" : `,"seq":${JSON.stringify(seq)}`;
      ops.push({ op: "frame", raw: `{"type":"event","id":${idRef}${seqRaw},"final":${JSON.stringify(ev.final)}${evRaw}}` });
    } else if (r < 36) {
      ops.push({ op: "frame", raw: pick([`{"type":"notification","name":"cfg.changed","payload":{"k":1}}`, `{"type":"notification","name":"x"}`, `{"type":"notification"}`, `{"type":"notification","name":3,"payload":null}`, `{"type":"hello_ok","protocolVersion":"1.2"}`, `{"type":"zzz","id":${idRef}}`, `[1,2]`, `"str"`, `7`]) });
    } else if (r < 37) {
      ops.push({ op: "frame", raw: pick(["{bad json", "", "{\"type\":\"response\",}"]) });
    } else if (r < 39) {
      ops.push({ op: "serverClose", code: pick([1000, 4003, 1006, 4001]), reason: pick(["", "auth_failed", "protocol_mismatch", "bye"]) });
    } else ops.push({ op: "clientClose" });
  }
  return ops;
}
const cases = [];
for (let i = 0; i < 600; i++) { const s = script(); cases.push({ script: s, logs: await run(s) }); }
// Fixed scripts with frames that throw inside the TS message listener (SRC-DEFECT-015).
const crashCases = [];
for (const raw of ["null", `{"type":"response","id":"$id0","ok":false}`, `{"type":"response","id":"$id0","ok":false,"error":{"code":5}}`, `{"type":"response","id":"$id0","ok":false,"error":null}`]) {
  const s = [{ op: "request", method: "m", params: {} }, { op: "frame", raw }];
  crashCases.push({ script: s, logs: await run(s) });
}
const text = JSON.stringify({ cases, crashCases }) + "\n";
const target = path.join(root, "rust/crates/sdk/tests/fixtures/transport.json");
if (process.argv.includes("--check")) { const same = fs.readFileSync(target, "utf8") === text; console.log(same ? "fixture stable" : "fixture DIFFERS"); process.exit(same ? 0 : 1); }
fs.writeFileSync(target, text);
const count = (pred) => cases.reduce((a, c) => a + c.logs.reduce((b, l) => b + l.log.filter(pred).length, 0), 0);
console.log("cases", cases.length, "req ok", count((e) => "req" in e && "ok" in e), "req err", count((e) => "req" in e && "err" in e), "items", count((e) => "item" in e), "stream err", count((e) => "stream" in e && "err" in e), "notif", count((e) => "notif" in e), "crash", count((e) => "crash" in e));
console.log(JSON.stringify(crashCases.map((c) => c.logs.at(-1))));
