// Live parity for packages/sdk/src/hook-server.ts HostedHookServer vs crates/sdk/src/hook_server.rs.
// Random hook configs (shared callback catalogue), start options and raw HTTP requests (method, target, auth,
// query selectors, JSON bodies). Each request goes to both servers; status, content-type and body (uuids
// normalized) must match, as must the start config/error.
// Usage: node rust/parity/sdk/hook_server_parity.mjs --rust <sdk_hook_probe> [--seed N] [--scripts N] [--out FILE]
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import readline from "node:readline";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const { HostedHookServer } = await import(path.join(root, "packages/sdk/dist/hook-server.js"));
const { PilotDeckError } = await import(path.join(root, "packages/sdk/dist/types.js"));
const arg = (n, d) => { const i = process.argv.indexOf(n); return i < 0 ? d : process.argv[i + 1]; };
const rustBin = arg("--rust");
let seed = Number(arg("--seed", 77));
const scripts = Number(arg("--scripts", 60));
const rnd = (n) => { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; };
const pick = (a) => a[rnd(a.length)];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const catalogue = {
  allow: (i) => ({ hookSpecificOutput: { hookEventName: i.hook_event_name, permissionDecision: "allow" } }),
  defer: (i) => ({ hookSpecificOutput: { hookEventName: i.hook_event_name, permissionDecision: "defer", additionalContext: "d" } }),
  wrong: () => ({ hookSpecificOutput: { hookEventName: "Stop" } }),
  specStr: () => ({ hookSpecificOutput: "abc" }),
  block: () => ({ decision: "block", reason: "r" }),
  approve: () => ({ decision: "approve", reason: "ok" }),
  cont: () => ({ continue: false, stopReason: "s" }),
  sys1: () => ({ systemMessage: "one" }),
  sys2: () => ({ systemMessage: "two", continue: true }),
  echo: (inp, tid, ctx) => ({ systemMessage: JSON.stringify({ inp, tid, signal: typeof ctx.signal, idLen: ctx.asyncHookId.length }) }),
  async: () => ({ async: true, asyncTimeout: 5 }),
  asyncNoT: () => ({ async: true }),
  asyncBad: () => ({ async: true, asyncTimeout: 0 }),
  asyncStr: () => ({ async: true, asyncTimeout: "5" }),
  none: () => undefined,
  zero: () => 0,
  str: () => "xy",
  num: () => 7,
  arr: () => ["a"],
  throwSdk: () => { throw new PilotDeckError({ code: "conflict", message: "c" }); },
  throwErr: () => { throw new Error("e"); },
  throwStr: () => { throw "s"; },
  slow: async () => { await sleep(400); return { systemMessage: "slow" }; },
  idx: () => JSON.parse('{"2":"b","1":"a","z":1}'),
};
const names = Object.keys(catalogue).filter((n) => n !== "slow");
const events = ["PreToolUse", "PostToolUse", "Stop", "SessionStart", "PermissionRequest", "UserPromptSubmit"];
const num = (v) => (v === "NaN" ? NaN : v === "Infinity" ? Infinity : v);

// Fixed scripts for merge/validation paths that random selection rarely reaches with a valid payload.
const fixedCombos = [["defer"], ["defer", "allow"], ["block", "approve"], ["approve", "block"], ["cont", "sys2"], ["sys1", "sys2", "sys1"], ["sys1", "async"], ["async", "sys1"], ["async", "async"],
  ["none", "async"], ["async", "none"], ["allow", "wrong"], ["specStr"], ["idx", "sys1"], ["asyncBad"], ["asyncStr"], ["asyncNoT"], ["str", "num", "arr"], ["echo"]];
let fixedIndex = 0;
function genScript() {
  if (fixedIndex < fixedCombos.length) {
    const hooks = { PreToolUse: [{ matcher: "Bash", hooks: fixedCombos[fixedIndex++] }] };
    return { hooks, start: {}, fixed: true };
  }
  const hooks = {};
  const nev = 1 + rnd(3);
  for (let i = 0; i < nev; i++) {
    const ev = rnd(25) === 0 ? pick(["Notification", "CwdChanged", "Bogus", "WorktreeRemove"]) : pick(events);
    hooks[ev] = Array.from({ length: 1 + rnd(3) }, () => {
      const m = { hooks: Array.from({ length: rnd(4) }, () => (rnd(30) === 0 ? "slow" : pick(names))) };
      if (rnd(2)) m.matcher = pick(["Bash", "", "Read|Write", "*"]);
      if (rnd(3) === 0) m.timeout = pick([0.1, 2, 0, -1, "NaN", "Infinity", 1.5]);
      if (m.hooks.includes("slow") && m.timeout === undefined) m.timeout = 0.1;
      return m;
    });
  }
  const start = {};
  if (rnd(4) === 0) start.path = pick(["/hooks", "/a/b?x", "nope", "/", "/p%20q"]);
  if (rnd(5) === 0) start.publicUrl = pick(["https://example.com/h", "http://EXAMPLE.com:80/a/../b", "ftp://x/y", "not a url", "http://[::1]:9/x?y#z"]);
  if (rnd(10) === 0) start.host = pick(["::1", "127.0.0.1"]);
  return { hooks, start };
}

const tsConfigOf = (hooks) => Object.fromEntries(Object.entries(hooks).map(([ev, ms]) => [ev, ms.map((m) => ({ ...(m.matcher !== undefined ? { matcher: m.matcher } : {}), ...(m.timeout !== undefined ? { timeout: num(m.timeout) } : {}), hooks: m.hooks.map((h) => catalogue[h]) }))]));

function httpRaw(port, host, raw) {
  return new Promise((resolve) => {
    const s = net.connect({ port, host });
    const chunks = [];
    const timer = setTimeout(() => { s.destroy(); resolve(`<timeout> ${Buffer.concat(chunks).toString("utf8").slice(0, 200)}`); }, 5000);
    s.on("close", () => clearTimeout(timer));
    s.on("data", (d) => chunks.push(d));
    const done = () => {
      const buf = Buffer.concat(chunks);
      if (!buf.length) return resolve("<no response>");
      const h = buf.indexOf("\r\n\r\n");
      const lines = buf.subarray(0, h < 0 ? buf.length : h).toString("utf8").split("\r\n");
      let body = h < 0 ? Buffer.alloc(0) : buf.subarray(h + 4);
      const ct = lines.find((l) => /^content-type:/i.test(l));
      if (lines.some((l) => /^transfer-encoding:\s*chunked/i.test(l))) { // HTTP framing only: decode chunks (byte sizes)
        const parts = []; let i = 0;
        while (true) { const j = body.indexOf("\r\n", i); const n = parseInt(body.subarray(i, j).toString(), 16); if (!n) break; parts.push(body.subarray(j + 2, j + 2 + n)); i = j + 2 + n + 2; }
        body = Buffer.concat(parts);
      }
      resolve(`${lines[0]} | ${ct ? ct.split(":")[1].trim() : "-"} | ${body.toString("utf8")}`);
    };
    s.on("end", done);
    s.on("error", () => { if (!chunks.length) resolve("<no response>"); });
    s.on("close", done);
    s.write(raw);
  });
}

const payloads = (ev) => [
  { hookEventName: ev, sessionId: "s1", transcriptPath: "/t", cwd: "/w", toolName: "Bash", toolInput: { cmd: "ls", n: 1.5, big: 1e21 }, toolUseId: "tu1" },
  { hook_event_name: ev, session_id: "s", transcript_path: "/t", cwd: "/w", tool_call_id: "tc" },
  { hookEventName: ev, sessionId: "s", transcriptPath: "/t", cwd: "/w", "2": "two", "1": "one", unicode: "é\u2028" },
  { hookEventName: ev, sessionId: "s", transcriptPath: "/t" },
  { hookEventName: "Notification", sessionId: "s", transcriptPath: "/t", cwd: "/w" },
  { hookEventName: ev, sessionId: 1, transcriptPath: "/t", cwd: "/w" },
  { hookEventName: ev, sessionId: "a", session_id: "b", transcriptPath: "/t", cwd: "/w" },
];
function fixedRequest(cfg) {
  const p = new URL(cfg.url).pathname;
  const body = JSON.stringify(payloads("PreToolUse")[rnd(3)]);
  return `POST ${p}?event=PreToolUse&matcher=0 HTTP/1.1\r\nhost: x\r\nauthorization: ${cfg.headers.authorization}\r\nconnection: close\r\ncontent-length: ${Buffer.byteLength(body)}\r\n\r\n${body}`;
}
function genRequest(hooks, cfg) {
  const p = new URL(cfg.url).pathname;
  const evs = Object.keys(hooks);
  const ev = rnd(8) === 0 ? pick(["", "Stop", "toString", "PreToolUse%20"]) : pick(evs);
  const q = [];
  if (rnd(10)) q.push(`event=${ev}`);
  if (rnd(6)) q.push(`matcher=${pick(["0", "0", "1", "2", "", "-0", "1.0", "%201%20", "0x1", "abc", "-1", "1e0", "0.5"])}`);
  const pathPart = rnd(8) === 0 ? pick(["/other", `${p}/`, `/x/..${p}`, p.replace("-", "%2D"), `${p}#frag`, "//", `/${p}`]) : p;
  const target = `${pathPart}${q.length ? "?" + q.join("&") : ""}`;
  const method = rnd(12) === 0 ? pick(["GET", "PUT"]) : "POST";
  const token = cfg.headers.authorization;
  const auth = rnd(10) === 0 ? pick(["", "authorization: Bearer nope\r\n", `authorization: ${token.toLowerCase().replace("bearer", "bearer")}\r\n`, `authorization:   ${token}  \r\n`, `authorization: ${token}\r\nauthorization: x\r\n`]) : `authorization: ${token}\r\n`;
  let body = rnd(10) === 0 ? pick(["", "[]", "null", "{bad", "\"s\"", "{\"a\":1"]) : JSON.stringify(pick(payloads(ev.replace("%20", ""))));
  if (rnd(150) === 0) body = JSON.stringify({ hookEventName: "Stop", x: "y".repeat(1_100_000) });
  const chunked = rnd(6) === 0 && body.length > 0; // an empty chunked body would end the message early and send stray bytes
  const raw = chunked
    ? `${method} ${target} HTTP/1.1\r\nhost: x\r\n${auth}connection: close\r\ntransfer-encoding: chunked\r\n\r\n${Buffer.byteLength(body).toString(16)}\r\n${body}\r\n0\r\n\r\n`
    : `${method} ${target} HTTP/1.1\r\nhost: x\r\n${auth}connection: close\r\ncontent-length: ${Buffer.byteLength(body)}\r\n\r\n${body}`;
  return rnd(80) === 0 ? "BROKEN\r\n\r\n" : raw;
}

async function startRust(spec) {
  const p = spawn(rustBin, [], { stdio: ["pipe", "pipe", "inherit"] });
  const rl = readline.createInterface({ input: p.stdout });
  const it = rl[Symbol.asyncIterator]();
  p.stdin.write(JSON.stringify(spec) + "\n");
  const first = JSON.parse((await it.next()).value);
  p.stdin.on("error", () => {});
  return { first, close: async () => { if (first.err) return "exited"; p.stdin.write("close\n"); const r = (await it.next()).value; p.stdin.end(); return r; } };
}

// SRC-DEFECT-016: for a request target that new URL() rejects (e.g. "//?x" or "//"), TS throws inside the
// fire-and-forget handleRequest. That is an unhandled rejection, which kills the host process (here it is caught and
// recorded). Rust answers 404, because the path cannot match.
let tsCrash = null, intended = 0;
process.on("unhandledRejection", (e) => { tsCrash = e; });
process.on("uncaughtException", (e) => { tsCrash = e; });
const norm = (s) => String(s).replace(/[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}/g, "<uuid>").replace(/:\d{2,5}\//g, ":<port>/").replace(/:\d{2,5}$/g, ":<port>");
let checks = 0, bad = [];
for (let si = 0; si < scripts + fixedCombos.length; si++) {
  const spec = genScript();
  let ts, tsFirst;
  try { ts = new HostedHookServer(tsConfigOf(spec.hooks)); tsFirst = { ok: await ts.start(spec.start) }; }
  catch (e) { tsFirst = { err: { code: e.code, message: e.message } }; }
  const rust = await startRust(spec);
  checks++;
  if (norm(JSON.stringify(tsFirst)) !== norm(JSON.stringify(rust.first))) { bad.push(`script ${si} start\n ts   ${JSON.stringify(tsFirst)}\n rust ${JSON.stringify(rust.first)}`); }
  if (tsFirst.ok && rust.first.ok) {
    // Requests go to the local listener; publicUrl only changes the advertised URL.
    if (spec.start.publicUrl) { await ts.close(); await rust.close(); continue; } // advertised URL only
    const tsAddr = ts.httpServer.address();
    const rustUrl = new URL(rust.first.ok.url);
    const rustPort = Number(rustUrl.port);
    for (let r = 0; r < 12; r++) {
      const rawTs = spec.fixed ? fixedRequest(tsFirst.ok) : genRequest(spec.hooks, tsFirst.ok);
      const rawRust = rawTs.split(tsFirst.ok.headers.authorization).join(rust.first.ok.headers.authorization);
      tsCrash = null;
      const [a, b] = await Promise.all([httpRaw(tsAddr.port, tsAddr.address, rawTs), httpRaw(rustPort, rustUrl.hostname.replace(/^\[|\]$/g, ""), rawRust)]);
      checks++;
      if (tsCrash) {
        const ok = tsCrash.code === "ERR_INVALID_URL" && b === 'HTTP/1.1 404 Not Found | application/json | {"error":"PilotDeck SDK hook endpoint not found."}';
        if (ok) { intended++; continue; }
        bad.push(`script ${si} TS crashed (${tsCrash.code}) rust ${b}`);
        continue;
      }
      if (norm(a) !== norm(b)) bad.push(`script ${si} req ${JSON.stringify(rawTs.slice(0, 300))}\n ts   ${a.slice(0, 400)}\n rust ${b.slice(0, 400)}`);
    }
  }
  if (ts && tsFirst.ok) await ts.close();
  await rust.close();
}
const text = `${checks - bad.length}/${checks} checks identical (incl. ${intended} SRC-DEFECT-016 intended: TS crashes, Rust 404)\n${bad.slice(0, 15).join("\n")}\n`;
process.stdout.write(text);
if (arg("--out")) fs.writeFileSync(arg("--out"), text);
process.exit(bad.length ? 1 : 0);
