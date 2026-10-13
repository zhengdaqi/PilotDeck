// TS oracle for SDK session-store (InMemorySessionStore, foldSessionSummary, summaryToSessionInfo,
// importSessionToStore). Date.now is driven by explicit "now" ops.
// Usage: node --experimental-strip-types rust/parity/sdk/gen_session_store.mjs [--check]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { drive } from "./drive.mjs";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const m = await import(path.join(root, "packages/sdk/src/session-store.ts"));
let clock = 0;
Date.now = () => clock;
const api = { ...m, setNow: (v) => { clock = v; } };

let seed = 99;
// mulberry32 (a plain LCG's low bits cycle and starve some branches)
const rnd = (n) => { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; };
const pick = (a) => a[rnd(a.length)];
const pks = ["p1", "p2", "p1\u0000x", "é"];
const sids = ["s1", "s2", "s3", "a\u0000b"];
const subs = [undefined, undefined, undefined, "", "sub/1", "agent-2"];
const keyOf = () => { const k = { projectKey: pick(pks), sessionId: pick(sids) }; const s = pick(subs); if (s !== undefined) k.subpath = s; return k; };
const texts = ["hi", "", "hello world", "第二", null, 3];
const entry = () => {
  const e = { type: pick(["user", "assistant", "session_metadata", "x"]) };
  const u = pick([undefined, undefined, "u1", "u2", "u3", "", 7, "7", null, 0]); if (u !== undefined) e.uuid = u;
  if (rnd(2)) e.timestamp = pick(["2026-01-01T00:00:00Z", "t2", 5]);
  if (rnd(3) === 0) e.cwd = pick(["/repo", "", 1]);
  if (rnd(3) === 0) e.text = pick(texts);
  if (rnd(4) === 0) e.message = pick([{ text: pick(texts) }, "str", null, [1], {}]);
  if (rnd(4) === 0) e.messages = pick([[{ content: [{ text: "" }, { text: pick(texts) }] }, { content: [{ text: "m2" }] }], [{ content: "x" }, "y", null], "nope", [{ content: [ "s", { type: "t" } ] }]]);
  if (e.type === "session_metadata" || rnd(5) === 0) e.metadata = pick([{ title: "T", aiTitle: null, tag: "tg", gitBranch: "main", other: 1 }, { firstPrompt: "fp", lastPrompt: "lp" }, null, "s", [], { title: undefined }]);
  return e;
};
const entries = () => Array.from({ length: rnd(4) }, entry);
const ops = [];
for (let t = 0; t < 40; t++) {
  ops.push({ op: "new" });
  const s = t; let now = 1000;
  for (let i = 0; i < 60; i++) {
    const k = rnd(14);
    if (k < 2) { now = pick([now, now + 1, now + 50, now - 30, 0]); ops.push({ op: "now", v: now }); }
    else if (k < 6) ops.push({ op: "append", s, key: keyOf(), entries: entries() });
    else if (k === 6) ops.push({ op: "load", s, key: keyOf() });
    else if (k === 7) ops.push({ op: "listSessions", s, pk: pick(pks) });
    else if (k === 8) ops.push({ op: "listSessionSummaries", s, pk: pick(pks) });
    else if (k === 9) ops.push({ op: "listSubkeys", s, pk: pick(pks), sid: pick(sids) });
    else if (k === 10) ops.push({ op: "delete", s, key: keyOf() });
    else if (k === 11) { const opts = { projectKey: pick(pks) }; const sp = pick(subs); if (sp !== undefined) opts.subpath = sp; const b = pick([undefined, 1, 2, 2.7, -3, 0, 500]); if (b !== undefined) opts.batchSize = b; ops.push({ op: "import", s, sid: pick(sids), entries: Array.from({ length: rnd(7) }, entry), opts }); }
    else if (k === 12) { const prev = pick([null, { sessionId: "old", mtime: 9, data: { title: "keep", createdAt: "c0" } }, { sessionId: "old", mtime: 4, data: {} }]); const opts = pick([undefined, {}, { mtime: 77 }]); ops.push({ op: "fold", prev, key: keyOf(), entries: entries(), ...(opts ? { opts } : {}) }); }
    else ops.push({ op: "info", summary: pick([{ sessionId: "a", mtime: 3, data: { title: "x", sessionId: "override", lastModified: 0 } }, { sessionId: "b", mtime: 0, data: {} }, { sessionId: "c", mtime: 12, data: { extra: [1], sessionKey: null } }]) });
  }
}
const expected = await drive(api, ops);
const text = JSON.stringify({ ops: JSON.parse(JSON.stringify(ops)), expected }) + "\n";
const target = path.join(root, "rust/crates/sdk/tests/fixtures/session_store.json");
fs.mkdirSync(path.dirname(target), { recursive: true });
if (process.argv.includes("--check")) { const same = fs.readFileSync(target, "utf8") === text; console.log(same ? "fixture stable" : "fixture DIFFERS"); process.exit(same ? 0 : 1); }
fs.writeFileSync(target, text);
console.log(`wrote ${ops.length} ops`);
