// Oracle for packages/sdk/src/client.ts validateOptions (not exported: a load hook appends the export).
// Options are random JS values; functions, undefined members and non-finite numbers use JSON markers
// ({"$fn":1}, {"$undef":1}, {"$num":"NaN"|"Infinity"|"-Infinity"}). Output: thrown code/message or "ok".
// Usage: node --experimental-transform-types rust/parity/sdk/gen_validate_options.mjs [--check]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const out = path.join(root, "rust/crates/sdk/tests/fixtures/validate_options.json");
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
}
export async function load(url, ctx, next) {
  const r = await next(url, ctx);
  if (url.endsWith("/packages/sdk/src/client.ts")) {
    return { ...r, source: String(r.source) + "\\nexport { validateOptions as __validateOptions };\\n" };
  }
  return r;
}`));
const { __validateOptions: validateOptions } = await import(path.join(root, "packages/sdk/src/client.ts"));

let seed = 0x0b7105;
function rnd(n) { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; }
const pick = (a) => a[rnd(a.length)];
const FN = () => {};
const U = undefined;

// Generic junk used for "wrong type" probes.
const JUNK = () => pick([U, null, true, false, 0, 1, -1, 1.5, NaN, Infinity, "", " ", "x", [], ["x"], {}, { a: 1 }, FN]);
const s = () => pick(["", "  ", "a", " b ", "tool", "search_tools", "\u00a0", "\ufeffx"]);
const absPath = () => pick(["/abs/p", "/abs/q", "rel/p", "", "  ", "/", "C:\\win", 5]);
const tool = () => rnd(5) ? { name: s(), ...(rnd(3) ? {} : { searchHint: pick([U, "", "hint", 3]) }) } : JUNK();
const tools = () => rnd(6) ? Array.from({ length: rnd(3) }, tool) : JUNK();
const mcpServer = () => {
  const r = rnd(10);
  if (r < 2) return { type: "stdio", command: pick(["node", "", " ", 5, U]), ...(rnd(3) ? {} : { deferredTools: tools() }) };
  if (r < 5) return { type: pick(["http", "streamable_http", "sse"]), url: pick(["http://x", "", " ", 5, U]), ...(rnd(3) ? {} : { deferredTools: tools() }) };
  if (r < 6) return { start: rnd(4) ? FN : 1, type: "sdk" };
  if (r < 7) return { type: pick(["sdk", "claude_ai_proxy", "ws", U]) };
  return JUNK();
};
const mcpMap = () => rnd(6) ? Object.fromEntries(Array.from({ length: 1 + rnd(2) }, () => [pick(["a", "b", " ", "", " a"]), mcpServer()])) : JUNK();
const agent = () => {
  if (!rnd(8)) return JUNK();
  const a = { description: pick(["d", "d", "", " ", 5, U]), prompt: pick(["p", "p", "", 5, U]) };
  const f = () => rnd(4) === 0;
  if (f()) a.background = pick([true, "yes", U]);
  if (f()) a.observer = pick(["a", "b", "c", "", 3]);
  if (f()) a.observerMessage = pick(["m", " ", "", 3]);
  if (f()) a.tools = pick([["t"], ["t", " "], [5], "t", U]);
  if (f()) a.disallowedTools = pick([["t"], [""], []]);
  if (f()) a.maxTurns = pick([1, 0, 1.5, "2", -1]);
  if (f()) a.model = pick(["m", "", 5]);
  if (f()) a.initialPrompt = pick(["i", "", 5, U]);
  if (f()) a.criticalSystemReminder_EXPERIMENTAL = pick(["c", " ", null]);
  if (f()) a.effort = pick(["low", "high", "max", 1]);
  if (f()) a.permissionMode = pick(["default", "plan", "acceptEdits", "bypassPermissions"]);
  if (f()) a.skills = pick(["all", ["s"], ["s", "s"], [], [" "], "some", [1]]);
  if (f()) a.memory = pick(["inherit", "disabled", "on", U]);
  if (f()) a.mcpServers = pick([() => "ref", () => [" ref ", "ref"], () => [""], () => ["a", "b"], () => mcpMap(), () => [mcpMap(), "x"], () => [null], () => [["x"]], () => ({ start: FN }), () => [{ x: { start: FN } }]])();
  return a;
};

const FIELDS = {
  continue: () => pick([true, false, 1, ""]),
  resume: () => pick(["sid", "", U]),
  sessionId: () => pick(["sid", ""]),
  resumeSessionAt: () => pick(["e1", "", " ", 5, U]),
  resumeDropsTurn: () => pick(["e2", "", 5]),
  persistSession: () => pick([false, true, 0]),
  forkSession: () => pick([true, false]),
  promptSuggestions: () => pick([true, "yes", U]),
  agent: () => pick([U, "x", null]),
  env: () => pick([U, {}, null]),
  debug: () => pick([U, false]),
  stderr: () => pick([U, FN]),
  perTaskStopAffordance: () => pick([U, true]),
  sandbox: () => pick([U, null, "x", [], {}, { type: "tool_policy" }, { network: "deny" }, { filesystem: "read_only", process: "deny" }, { filesystem: "write" }, { network: "allow" }, { process: 1 }, { type: "docker", network: "deny" }, { extra: 1 }, { network: U }, { type: U, filesystem: "deny" }, { type: ["a", "b"] }, { type: { a: 1 } }, { type: null }]),
  pluginDelivery: () => pick(["initialize", "argv", U, null]),
  plugins: () => pick([U, [], "x", null, () => [{ type: "local", path: absPath() }], () => [{ type: "local", path: "/abs/p" }, { type: "local", path: absPath() }], () => [{ type: "remote", path: "/a" }], () => [null], () => [{ type: "local" }]]),
  agentProgressSummaries: () => pick([true, 1, U]),
  forwardSubagentText: () => pick([false, "no"]),
  onElicitation: () => pick([FN, U, 0]),
  onUserDialog: () => pick([FN, U, 1]),
  userDialogMode: () => pick(["manual", "auto", U, null, 5, ["a", "b"], {}]),
  supportedDialogKinds: () => pick([U, [], ["input"], ["elicitation"], ["input", "input"], ["input", "bogus"], ["form", "select", "confirm"], "elicitation", "input", 5, null, [NaN, NaN], [{}, {}], [1, "1"]]),
  skills: () => pick(["all", ["s"], [], ["s", "s"], [" "], [1], "x", null, U]),
  sessionStore: () => pick([{}, U, null, 1]),
  projectKey: () => pick(["pk", "", U]),
  hookServer: () => pick([{}, U, 0]),
  hooks: () => pick([{}, U]),
  permissionPrompts: () => pick(["host", "none", "ask", U, null]),
  loadTimeoutMs: () => pick([1, 0, -1, NaN, Infinity, "5", U]),
  includeHookEvents: () => pick([true, 1, U]),
  fallbackModel: () => pick(["m", "", " ", 5, U]),
  thinking: () => pick([U, null, { type: "disabled", display: "summarized" }, { type: "enabled", display: "summarized" }, { display: "summarized" }, { type: "enabled" }, "summarized", 5]),
  maxThinkingTokens: () => pick([0, 10, -1, 1.5, null, U, "5", NaN]),
  reconnect: () => pick([U, true, 0, {}, { maxAttempts: 0 }, { maxAttempts: 2 }, { maxAttempts: 1.5 }, { initialDelayMs: -1 }, { initialDelayMs: 5, maxDelayMs: 4 }, { initialDelayMs: 5, maxDelayMs: 5 }, { maxDelayMs: Infinity }, { jitter: 1 }, { jitter: 1.1 }, { jitter: -0 }, { jitter: "0.5" }, "x"]),
  systemPrompt: () => pick([U, "sp", "", ["a", "b"], ["a", 1], { type: "preset" }, { type: "custom", prompt: "x" }, { type: "custom", prompt: ["a", 2] }, { type: "custom", prompt: { type: "preset" } }, { type: "other" }, null, 5, true, [], { type: "custom", prompt: null }]),
  tools: () => pick([U, ["a"], { type: "preset" }, "x", 0, null, []]),
  allowedTools: () => pick([U, ["a"], [""], [" "], [1], "a", null, []]),
  disallowedTools: () => pick([U, ["b"], ["\t"], {}]),
  deferredTools: () => pick([U, () => tools()]),
  maxTurns: () => pick([1, 0, 2.5, -3, "3", U, NaN]),
  maxBudgetUsd: () => pick([1, 0, 0.01, Infinity, "1", U]),
  taskBudget: () => pick([U, null, 5, "x", {}, { total: 1 }, { total: 0 }, { total: 1, scope: "session" }, { total: 1, scope: "global" }, { total: 1, projectRetentionMs: 10 }, { total: 1, scope: "project", projectRetentionMs: 10 }, { total: 1, scope: "project", projectRetentionMs: 1.5 }, { total: 1, scope: "project", projectRetentionMs: 2 ** 60 }, { total: Infinity }, [1]]),
  outputStyle: () => pick(["s", "", " ", 5, U]),
  additionalDirectories: () => pick([U, [], ["/a"], ["/a", "b"], [5], null, "x", 5]),
  toolAliases: () => pick([U, {}, { a: "b" }, { a: " " }, { " ": "b" }, { a: 5 }, "ab", ["x"], 0]),
  outputFormat: () => pick([U, {}, { schema: { type: "object" } }, { schema: { type: ["a", 1] } }, { schema: { required: ["a"] } }, { schema: { required: "a" } }, { schema: { additionalProperties: 1 } }, { schema: { enum: 1 } }, { schema: { properties: { a: { type: "string" } } } }, { schema: { properties: { a: 5 } } }, { schema: { properties: [] } }, { schema: { items: { type: 1 } } }, { schema: [] }, { schema: null }, "x", 0]),
  mcpServers: () => mcpMap(),
  agents: () => rnd(8) ? Object.fromEntries(Array.from({ length: 1 + rnd(3) }, (_, i) => [pick(["a", "b", "c", " "][i] === undefined ? "a" : ["a", "b", "c", " "][rnd(4)]), agent()])) : JUNK(),
};
const KEYS = Object.keys(FIELDS);
const val = (k) => { const v = FIELDS[k](); return typeof v === "function" && v !== FN ? v() : v; };

function enc(v) {
  if (v === undefined) return { $undef: 1 };
  if (typeof v === "function") return { $fn: 1 };
  if (typeof v === "number" && !Number.isFinite(v)) return { $num: String(v) };
  if (Array.isArray(v)) return v.map(enc);
  if (v && typeof v === "object") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, enc(x)]));
  return v;
}

const cases = [];
const seen = new Set();
function run(options) {
  let res;
  try { validateOptions(options); res = "ok"; } catch (e) {
    res = e?.constructor?.name === "PilotDeckError" || e?.code ? `${e.code}: ${e.message}` : `${e.constructor.name}: ${e.message}`;
  }
  const o = enc(options);
  const key = JSON.stringify(o);
  if (seen.has(key)) return;
  seen.add(key);
  cases.push({ options: o, result: res });
}
// Fixed probes for paths random combos rarely reach.
for (const o of [
  { onUserDialog: FN, supportedDialogKinds: [NaN, NaN] }, { onUserDialog: FN, supportedDialogKinds: [NaN, 1] },
  { onUserDialog: FN, supportedDialogKinds: [0, -0] }, { onUserDialog: FN, supportedDialogKinds: ["input", "form"] },
  { userDialogMode: "manual", supportedDialogKinds: "xxelicitationxx" }, { userDialogMode: "manual", supportedDialogKinds: "input" },
  { userDialogMode: "manual", supportedDialogKinds: 5 }, { userDialogMode: "manual", supportedDialogKinds: null },
  { onElicitation: FN, onUserDialog: FN }, { onElicitation: FN, onUserDialog: FN, supportedDialogKinds: "elicitation" },
  { onElicitation: FN, onUserDialog: FN, supportedDialogKinds: 7 }, { onElicitation: FN, onUserDialog: FN, supportedDialogKinds: null },
  { agents: { a: { description: "d", prompt: "p", observer: "a" } } }, { agents: { a: { description: "d", prompt: "p", observer: "b" } } },
  { agents: { a: { description: "d", prompt: "p", observer: "b" }, b: { description: "d", prompt: "p" } } },
  { agents: { a: { description: "d", prompt: "p", mcpServers: { s: { start: FN } } } } },
  { agents: { a: { description: "d", prompt: "p", mcpServers: { start: FN } } } }, { agents: { a: { description: "d", prompt: "p", mcpServers: [{ start: FN, x: 1 }] } } },
  { agents: { a: { description: "d", prompt: "p", mcpServers: [{ s: { type: "stdio", command: "x" } }, "s"] } } },
  { mcpServers: "ab" }, { mcpServers: ["x"] }, { mcpServers: { a: { start: FN } } }, { mcpServers: { a: { start: FN }, b: null } },
  { toolAliases: "ab" }, { toolAliases: ["", "x"] }, { toolAliases: { " ": "x" } },
  ...["agent", "pathToClaudeCodeExecutable", "env", "executable", "executableArgs", "extraArgs", "betas", "toolConfig", "allowDangerouslySkipPermissions", "permissionPromptToolName", "debug", "debugFile", "stderr", "spawnClaudeCodeProcess", "perTaskStopAffordance"].map((k) => ({ [k]: null })),
  { reconnect: { jitter: NaN } }, { reconnect: { jitter: -Infinity } }, { taskBudget: { total: 1, scope: "project", projectRetentionMs: 2 ** 53 - 1 } }, { taskBudget: { total: 1, scope: "project", projectRetentionMs: 2 ** 53 } },
  { plugins: [{ type: "local", path: "/a" }, { type: "local", path: "/a" }] }, { additionalDirectories: ["C:\\x"] },
]) run(o);
// One field at a time, every candidate several times.
for (const k of KEYS) for (let i = 0; i < 25; i++) run({ [k]: val(k) });
// Random combinations.
for (let i = 0; i < 12000; i++) {
  const o = {};
  for (let j = 0; j < 1 + rnd(5); j++) { const k = pick(KEYS); o[k] = val(k); }
  run(o);
}
const counts = {};
for (const c of cases) { const k = c.result === "ok" ? "ok" : c.result.split(":")[0]; counts[k] = (counts[k] || 0) + 1; }
const text = JSON.stringify({ cases }, null, 0).replace(/\},\{"options"/g, '},\n{"options"') + "\n";
if (process.argv.includes("--check")) {
  console.log(fs.existsSync(out) && fs.readFileSync(out, "utf8") === text ? "fixture stable" : "fixture DIFFERS");
} else {
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, text);
  console.log(`wrote ${cases.length} cases`, JSON.stringify(counts));
}
