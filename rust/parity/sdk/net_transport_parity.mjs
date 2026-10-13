// Live parity for the SDK network layer: the same scripted ws@8 gateway scenarios are run against
// packages/sdk/dist/transport.js (Node 22 global WebSocket) and the Rust probe (crates/sdk/examples/sdk_net_probe).
// Covers connect/hello (ok, silent, 4003, protocol_mismatch, auth_failed, close before hello, refused port, bad URL),
// reconnect attempts, requests (ok, undefined result, gateway error codes, timeout, invalid JSON frame, server close),
// streams (events, final, duplicate seq, gap, bad seq, error response), notifications and client close.
// Usage: node rust/parity/sdk/net_transport_parity.mjs --rust <probe binary> [--out FILE]
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { spawnSync, spawn } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const require = createRequire(import.meta.url);
const { WebSocketServer } = require(path.join(root, "node_modules/.pnpm/ws@8.21.0/node_modules/ws"));
const { GatewayTransport } = await import(path.join(root, "packages/sdk/dist/transport.js"));
const arg = (n) => { const i = process.argv.indexOf(n); return i < 0 ? undefined : process.argv[i + 1]; };
const rustBin = arg("--rust");
// --wasm: also run the rust/sdk-wasm npm wrapper (wasm core + JS WebSocket host) as a third side.
const wasm = process.argv.includes("--wasm") ? await import(path.join(root, "rust/sdk-wasm/npm/index.js")) : undefined;
if (!rustBin) throw new Error("--rust <probe> required");

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ev = (id, seq, event, final = false) => JSON.stringify({ type: "event", id, seq, final, event });
const res = (id, result) => JSON.stringify({ type: "response", id, ok: true, ...(result === undefined ? {} : { result }) });
const fail = (id, error) => JSON.stringify({ type: "response", id, ok: false, error });
const HELLO = JSON.stringify({ type: "hello_ok", protocolVersion: "1.2", serverVersion: "9.9.9", serverInfo: { mode: "local", projectKey: "p", capabilities: ["a", "b"] } });

// Server: per connection, hello policy then method handlers.
function methods(ws, f) {
  const id = f.id;
  const p = f.params ?? {};
  switch (f.method) {
    case "echo": return ws.send(res(id, { echo: f.params }));
    case "undef": return ws.send(res(id));
    case "fail": return ws.send(fail(id, p.error ?? { code: "SESSION_BUSY", message: "busy", details: { n: 1 } }));
    case "slow": return;
    case "kill": return ws.close(p.code ?? 1011, p.reason ?? "boom");
    case "bad": return ws.send("not json");
    case "notify": ws.send(JSON.stringify({ type: "notification", name: "changed", payload: { x: 1 } })); ws.send(JSON.stringify({ type: "notification", name: "bare" })); return ws.send(res(id, "n"));
    case "events": { for (let i = 0; i < (p.n ?? 3); i++) ws.send(ev(id, i, { type: "text", i }, i === (p.n ?? 3) - 1 && !p.noFinal)); if (p.noFinal) ws.send(res(id, null)); return; }
    case "dup": ws.send(ev(id, 0, { a: 0 })); ws.send(ev(id, 0, { a: "dup" })); ws.send(ev(id, 1, { a: 1 }, true)); return;
    case "gap": ws.send(ev(id, 0, { a: 0 })); ws.send(ev(id, 2, { a: 2 })); return;
    case "badseq": ws.send(ev(id, 0.5, { a: 0 })); return;
    case "streamfail": ws.send(ev(id, 0, { a: 0 })); ws.send(fail(id, { code: "invalid_args", message: "nope" })); return;
    case "streamkill": ws.send(ev(id, 0, { a: 0 })); setTimeout(() => ws.close(1001, ""), 30); return;
    case "other": ws.send(res("someone-else", 1)); ws.send(ev("nobody", 0, {})); return ws.send(res(id, 2));
  }
}
const scenarios = [
  { name: "basic", hello: "ok", ops: [{ op: "connect" }, { op: "connect" }, { op: "request", method: "echo", params: { a: [1, "x"], "2": 1, b: null } }, { op: "request", method: "undef" }, { op: "request", method: "other" }] },
  { name: "errors", hello: "ok", ops: [{ op: "connect" }, { op: "request", method: "fail" }, { op: "request", method: "fail", params: { error: { code: "invalid_x", message: "bad x" } } }, { op: "request", method: "fail", params: { error: { code: "capability_unavailable" } } }, { op: "request", method: "fail", params: { error: { code: "PROJECT_NOT_FOUND", message: 7 } } }, { op: "request", method: "fail", params: { error: {} } }] },
  { name: "timeout", hello: "ok", opts: { requestTimeoutMs: 150 }, ops: [{ op: "connect" }, { op: "request", method: "slow" }, { op: "request", method: "echo", params: 1 }] },
  { name: "streams", hello: "ok", ops: [{ op: "connect" }, { op: "stream", method: "events" }, { op: "stream", method: "events", params: { n: 2, noFinal: true } }, { op: "stream", method: "dup" }, { op: "stream", method: "gap" }, { op: "stream", method: "badseq" }, { op: "stream", method: "streamfail" }, { op: "request", method: "echo", params: "still" }] },
  { name: "streamkill", hello: "ok", ops: [{ op: "connect" }, { op: "stream", method: "streamkill" }, { op: "request", method: "echo" }, { op: "stream", method: "events" }] },
  { name: "notify", hello: "ok", ops: [{ op: "connect" }, { op: "request", method: "notify" }, { op: "sleep", ms: 50 }, { op: "notes" }] },
  { name: "kill", hello: "ok", ops: [{ op: "connect" }, { op: "request", method: "kill" }, { op: "request", method: "echo" }] },
  { name: "kill4003", hello: "ok", ops: [{ op: "connect" }, { op: "request", method: "kill", params: { code: 4003, reason: "" } }] },
  { name: "killmismatch", hello: "ok", ops: [{ op: "connect" }, { op: "request", method: "kill", params: { code: 1008, reason: "protocol_mismatch" } }] },
  { name: "badframe", hello: "ok", ops: [{ op: "connect" }, { op: "request", method: "bad" }, { op: "request", method: "echo" }] },
  { name: "close", hello: "ok", ops: [{ op: "connect" }, { op: "close" }, { op: "request", method: "echo" }, { op: "connect" }, { op: "close" }] },
  { name: "hello-silent", hello: "silent", opts: { connectTimeoutMs: 200 }, ops: [{ op: "connect" }] },
  { name: "hello-4003", hello: "4003", ops: [{ op: "connect" }, { op: "request", method: "echo" }] },
  { name: "hello-auth-reason", hello: "auth_failed", ops: [{ op: "connect" }] },
  { name: "hello-mismatch", hello: "protocol_mismatch", opts: { protocolVersion: "0.1" }, ops: [{ op: "connect" }] },
  { name: "hello-drop", hello: "drop", ops: [{ op: "connect" }] },
  { name: "hello-drop-retry", hello: "drop", opts: { reconnect: { maxAttempts: 3, initialDelayMs: 10, jitter: 0 } }, ops: [{ op: "connect" }], countConnections: true },
  { name: "flaky-then-ok", hello: "drop-first-2", opts: { reconnect: { maxAttempts: 4, initialDelayMs: 10, maxDelayMs: 20, jitter: 0.5 } }, ops: [{ op: "connect" }, { op: "request", method: "echo", params: "after" }], countConnections: true },
  { name: "auth-no-retry", hello: "4003", opts: { reconnect: { maxAttempts: 5, initialDelayMs: 1 } }, ops: [{ op: "connect" }], countConnections: true },
  { name: "refused", hello: "refused", ops: [{ op: "connect" }] },
  { name: "refused-retry", hello: "refused", opts: { reconnect: { maxAttempts: 2, initialDelayMs: 5 } }, ops: [{ op: "connect" }] },
  { name: "not-connected", hello: "ok", ops: [{ op: "request", method: "echo" }, { op: "stream", method: "events" }, { op: "connect" }] },
  { name: "hello-fields", hello: "custom", ops: [{ op: "connect" }] },
];

async function withServer(sc, fn) {
  if (sc.hello === "refused") {
    const s = net.createServer(); await new Promise((r) => s.listen(0, "127.0.0.1", r)); const port = s.address().port; await new Promise((r) => s.close(r));
    return fn(port, () => 0);
  }
  const wss = new WebSocketServer({ host: "127.0.0.1", port: 0 });
  await new Promise((r) => wss.on("listening", r));
  let conns = 0;
  const hellos = [];
  wss.on("connection", (ws) => {
    const n = ++conns;
    ws.on("message", (data) => {
      const f = JSON.parse(String(data));
      if (f.type === "hello") {
        hellos.push(f);
        const h = sc.hello;
        if (h === "ok" || (h === "drop-first-2" && n > 2)) ws.send(HELLO);
        else if (h === "custom") ws.send(JSON.stringify({ type: "hello_ok", protocolVersion: "1.2", serverVersion: "1", serverInfo: { mode: null, extra: 1 } }));
        else if (h === "4003") ws.close(4003, "");
        else if (h === "auth_failed") ws.close(1008, "auth_failed");
        else if (h === "protocol_mismatch") ws.close(1002, "protocol_mismatch");
        else if (h === "drop" || h === "drop-first-2") ws.terminate();
        return;
      }
      if (f.type === "request") methods(ws, f);
    });
  });
  try { return await fn(wss.address().port, () => conns, hellos); } finally { for (const c of wss.clients) c.terminate(); await new Promise((r) => wss.close(r)); }
}

const errOf = (e) => ({ name: e?.name, code: e?.code, message: e?.message, retryable: e?.retryable, ...(e?.details === undefined ? {} : { details: e.details }), ...(e?.requestId === undefined ? {} : { requestId: "<id>" }) });
async function runTs(options, ops, Impl = GatewayTransport) {
  const t = new Impl(options);
  const notes = [];
  t.onNotification((n) => notes.push(n));
  const out = [];
  for (const op of ops) {
    if (op.op === "connect") { try { out.push({ ok: await t.connect() }); } catch (e) { out.push({ err: errOf(e) }); } }
    else if (op.op === "request") { try { const v = await t.request(op.method, op.params); out.push({ ok: v === undefined ? "__undefined" : v }); } catch (e) { out.push({ err: errOf(e) }); } }
    else if (op.op === "stream") { const items = []; let end = "done"; try { for await (const x of t.stream(op.method, op.params)) items.push(x); } catch (e) { end = { err: errOf(e) }; } out.push({ items, end }); }
    else if (op.op === "close") { t.close(); out.push("closed"); }
    else if (op.op === "sleep") { await sleep(op.ms); out.push("slept"); }
    else if (op.op === "notes") { out.push(notes.splice(0)); }
  }
  return JSON.parse(JSON.stringify(out));
}
function runRust(options, ops) {
  return new Promise((resolve, reject) => {
    const p = spawn(rustBin, [], { stdio: ["pipe", "pipe", "ignore"] });
    const kill = setTimeout(() => p.kill(), 20_000);
    let buf = "";
    p.stdout.on("data", (d) => (buf += d));
    p.on("close", (code) => (clearTimeout(kill), code === 0 ? resolve(JSON.parse(buf)) : reject(new Error(`probe exit ${code}`))));
    p.stdin.end(JSON.stringify({ options, ops }));
  });
}

const lines = [];
let pass = 0;
for (const sc of scenarios) {
  const results = {};
  for (const side of ["ts", "rust", ...(wasm ? ["wasm"] : [])]) {
    results[side] = await withServer(sc, async (port, conns, hellos) => {
      const options = { url: `ws://127.0.0.1:${port}/ws`, token: "tok", connectTimeoutMs: 2000, requestTimeoutMs: 2000, ...sc.opts };
      const r = side === "ts" ? await runTs(options, sc.ops) : side === "wasm" ? await runTs(options, sc.ops, wasm.GatewayTransport) : await runRust(options, sc.ops).catch((e) => `probe failed: ${e.message}`);
      return { r, ...(sc.countConnections ? { connections: conns() } : {}), hello: hellos?.[0] };
    });
  }
  const uuid = (x) => JSON.stringify(x).replace(/[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}/g, "<uuid>"); // random request ids (v4 shape checked)
  const a = uuid(results.ts), b = uuid(results.rust), w = wasm ? uuid(results.wasm) : a;
  const ok = a === b && a === w;
  if (ok) pass++;
  lines.push(`${ok ? "PASS" : "FAIL"} ${sc.name}${ok ? "" : `\n  ts   ${a}\n  rust ${b}${wasm ? `\n  wasm ${w}` : ""}`}`);
}
// Invalid URLs: the constructor throws before any IO.
for (const url of ["http://127.0.0.1:1/ws", "ftp://x/", "ws://h/#frag", "not a url"]) {
  const ts = await runTs({ url, token: "t" }, [{ op: "connect" }]);
  const rust = await runRust({ url, token: "t" }, [{ op: "connect" }]).catch((e) => `probe failed: ${e.message}`);
  const wz = wasm ? await runTs({ url, token: "t" }, [{ op: "connect" }], wasm.GatewayTransport) : ts;
  const ok = JSON.stringify(ts) === JSON.stringify(rust) && JSON.stringify(ts) === JSON.stringify(wz);
  if (ok) pass++;
  lines.push(`${ok ? "PASS" : "FAIL"} url ${url}${ok ? "" : `\n  ts   ${JSON.stringify(ts)}\n  rust ${JSON.stringify(rust)}`}`);
}
const total = scenarios.length + 4;
lines.push(`${pass}/${total} scenarios identical`);
const text = lines.join("\n") + "\n";
process.stdout.write(text);
if (arg("--out")) fs.writeFileSync(arg("--out"), text);
process.exit(pass === total ? 0 : 1);
