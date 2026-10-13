// Oracle for the interactive TuiApp (dist build) driven through real Ink 7 + ink-text-input.
// State is read from the React fiber tree (Ink instance container) after each scripted keystroke.
// Usage: node rust/parity/tui/gen_tui_app_fixture.mjs [--check]
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { EventEmitter } from "node:events";
import { fileURLToPath } from "node:url";

let lastRoot = null; // FiberRoot of the current Ink instance (ink's internal instances map)

const here = path.dirname(fileURLToPath(import.meta.url));
const React = (await import("react")).default;
const { render, useInput } = await import("ink");
const instances = (await import("../../../node_modules/ink/build/instances.js")).default;
const { TuiApp } = await import("../../../dist/src/adapters/channel/tui/app/TuiApp.js");
const spec = JSON.parse(fs.readFileSync(path.join(here, "tui_app_spec.json"), "utf8"));
const FILES_DIR = "/tmp/pilotdeck-tui-oracle-files";
fs.mkdirSync(FILES_DIR, { recursive: true });
const filePath = (k) => path.join(FILES_DIR, `${k.replace(/@/g, "")}.txt`);
for (const [k, n] of Object.entries(spec.fake.files)) {
  fs.writeFileSync(filePath(k), Array.from({ length: n }, (_, i) => `line ${i + 1}`).join("\n"));
}
const subst = (v) => JSON.parse(JSON.stringify(v), (_k, x) => (typeof x === "string" && spec.fake.files[x] !== undefined ? filePath(x) : x));

class FakeGateway {
  constructor() { this.calls = []; }
  log(method, params) { this.calls.push({ method, params: params === undefined ? null : JSON.parse(JSON.stringify(params)) }); }
  async listSessions(p) { this.log("listSessions", p); return { sessions: structuredClone(spec.fake.sessions).slice(0, p?.limit ?? 50) }; }
  async newSession(p) { this.log("newSession", p); return { sessionKey: spec.fake.newSessionKey }; }
  async resumeSession(p) { this.log("resumeSession", p); return {}; }
  async readSessionMessages(p) {
    this.log("readSessionMessages", p);
    const m = spec.fake.messages[p.sessionKey];
    if (!m) throw new Error(`no session ${p.sessionKey}`);
    return { messages: structuredClone(m) };
  }
  async *submitTurn(p) {
    this.log("submitTurn", p);
    if (p.message === spec.fake.throwTurn) throw new Error(`${p.message} failed`);
    const evs = spec.fake.turns[p.message] ?? [
      { type: "turn_started" }, { type: "assistant_text_delta", text: `echo: ${p.message}` }, { type: "turn_completed" },
    ];
    for (const e of subst(evs)) yield e;
  }
  async permissionDecide(p) { this.log("permissionDecide", p); return {}; }
  async abortTurn(p) { this.log("abortTurn", p); return {}; }
}

class Stdout extends EventEmitter { columns = 100; rows = 30; isTTY = true; writes = []; write = (s) => { this.writes.push(s); return true; }; }
class Stdin extends EventEmitter {
  isTTY = true; data = null;
  write = (d) => { this.data = d; this.emit("readable"); this.emit("data", d); };
  setEncoding() {} setRawMode() {} resume() {} pause() {} ref() {} unref() {}
  read = () => { const d = this.data; this.data = null; return d; };
}

let probeLog = [];
function Probe() {
  useInput((input, key) => { probeLog.push({ input, key: Object.fromEntries(Object.entries(key).filter(([, v]) => v === true).map(([k]) => [k, true])) }); });
  return null;
}

function findFiber(fiber, pred) {
  const stack = [fiber];
  while (stack.length) {
    const f = stack.pop();
    if (!f) continue;
    if (pred(f)) return f;
    if (f.sibling) stack.push(f.sibling);
    if (f.child) stack.push(f.child);
  }
  return null;
}
function hookStates(f) { const out = []; let h = f?.memoizedState; while (h) { out.push(h.memoizedState); h = h.next; } return out; }
const name = (f) => (typeof f.type === "function" ? f.type.name : null);
function snapshot() {
  const root = lastRoot.current;
  const app = findFiber(root, (f) => name(f) === "TuiApp");
  const st = app ? hookStates(app).find((s) => s && typeof s === "object" && "activeSessionKey" in s) : null;
  const ti = findFiber(root, (f) => name(f) === "TextInput");
  const tiState = ti ? hookStates(ti).find((s) => s && typeof s === "object" && "cursorOffset" in s) : null;
  const viewer = findFiber(root, (f) => name(f) === "ToolOutputViewer");
  const viewerOffset = viewer ? hookStates(viewer).find((s) => typeof s === "number") : null;
  const state = st ? JSON.parse(JSON.stringify({ ...st, sidebarCollapsed: [...st.sidebarCollapsed].sort() })) : null;
  return { state, textInput: tiState ? { ...tiState } : null, viewerOffset: viewerOffset ?? null };
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
function readPerms(home) {
  try { const j = JSON.parse(fs.readFileSync(path.join(home, "permissions.json"), "utf8")); delete j.lastUpdated; return j; } catch { return null; }
}

const cwd = process.cwd();
const out = { cwd, columns: 100, rows: 30, filesDir: FILES_DIR, scenarios: [] };
for (const sc of spec.scenarios) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "pd-tui-home-"));
  if (sc.permissions) fs.writeFileSync(path.join(home, "permissions.json"), JSON.stringify(sc.permissions));
  if (sc.permissionsRaw) fs.writeFileSync(path.join(home, "permissions.json"), sc.permissionsRaw);
  process.env.PILOT_HOME = home;
  const gw = new FakeGateway();
  const stdout = new Stdout(); const stdin = new Stdin();
  let exited = false;
  const inst = render(React.createElement(React.Fragment, null, React.createElement(Probe), React.createElement(TuiApp, { gateway: gw, connection: "in_process", ...sc.props })), { stdout, stderr: new Stdout(), stdin, debug: true, patchConsole: false, exitOnCtrlC: true });
  lastRoot = instances.get(stdout).container;
  inst.waitUntilExit().then(() => { exited = true; }, () => { exited = true; });
  await sleep(60);
  const rec = { name: sc.name, props: sc.props, initialPermissions: readPerms(home), initial: snapshot(), steps: [] };
  for (const step of sc.steps) {
    // A typed line "text\r" is sent as two writes (Ink treats one multi-char chunk as a paste,
    // which never submits); {"raw": "..."} sends a single chunk verbatim.
    const writes = typeof step === "object" ? [step.raw] : step.length > 1 && step.endsWith("\r") ? [step.slice(0, -1), "\r"] : [step];
    const bytes = writes.join("");
    probeLog = [];
    const before = gw.calls.length;
    const wBefore = stdout.writes.length;
    for (const w of writes) { if (!exited) stdin.write(w); await sleep(150); }
    const scroll = stdout.writes.slice(wBefore).filter((w) => w.startsWith("\n--- "));
    rec.steps.push({ bytes, writes, events: probeLog, ...(exited ? { exited: true } : snapshot()), calls: gw.calls.slice(before), scrollback: scroll, permissions: readPerms(home) });
  }
  if (!exited) inst.unmount();
  rec.exited = exited;
  out.scenarios.push(rec);
  fs.rmSync(home, { recursive: true, force: true });
}
const target = path.join(here, "../../crates/tui-app/tests/fixtures/tui_app.json");
const text = JSON.stringify(out, null, 1) + "\n";
if (process.argv.includes("--check")) {
  const same = fs.existsSync(target) && fs.readFileSync(target, "utf8") === text;
  console.log(same ? "fixture stable" : "fixture DIFFERS");
  process.exit(same ? 0 : 1);
}
fs.mkdirSync(path.dirname(target), { recursive: true });
fs.writeFileSync(target, text);
console.log(`wrote ${target}: ${out.scenarios.length} scenarios, ${out.scenarios.reduce((a, s) => a + s.steps.length, 0)} steps`);
process.exit(0);
