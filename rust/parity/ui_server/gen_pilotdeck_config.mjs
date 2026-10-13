// Oracle for the pure part of ui/server/services/pilotdeckConfig.js: buildDefaultPilotDeckConfig,
// normalizePilotDeckConfig, sanitizeProviderCredentials, resolveModel, validatePilotDeckConfig, maskSecrets,
// preserveMaskedSecrets, hasUnresolvedMaskedSecrets, resolveConfiguredProviderUrl/ApiKey, buildRuntimeEnv,
// buildMemoryLlmOptions, buildMemoryDefaults, configRevision, expandTilde, syncAgentModelWithRouter.
// The process env is controlled per case (only the recorded variables exist besides the host's own).
// Usage: env HOME=/tmp/pdhome PILOT_HOME=/tmp/pdhome/.pd node --experimental-transform-types gen_pilotdeck_config.mjs [--check]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const out = path.join(root, "rust/crates/ui-server-core/tests/fixtures/pilotdeck_config.json");
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
const pc = await import(path.join(root, "ui/server/services/pilotdeckConfig.js"));

let seed = 4040;
function rnd(n) { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; }
const pick = (a) => a[rnd(a.length)];
const U = { $undef: 1 };
const j = (v) => (v === undefined ? U : JSON.parse(JSON.stringify(v)));
const call = (fn) => { try { return { v: j(fn()) }; } catch (e) { return { err: e.name, message: e.message }; } };
const clone = (v) => (v === undefined ? undefined : JSON.parse(JSON.stringify(v)));

const ENV_NAMES = ["OPENAI_API_KEY", "ANTHROPIC_API_KEY", "DEEPSEEK_API_KEY", "GEMINI_API_KEY", "MY_KEY", "EMPTY_KEY", "SERVER_PORT", "VITE_PORT", "HOST", "PILOTDECK_MAX_OUTPUT_TOKENS", "TAVILY_API_KEY"];
const PIDS = ["openai", "anthropic", "deepseek", "google", "ollama", "myp", "zhipu", "0", "a b"];
const MODELS = ["gpt-4o", "m1", "claude", "", "deepseek-chat", "x/y"];
const sv = () => pick([undefined, null, 5, true, "", " ", "s", [], {}]);
function provider(pid) {
  if (rnd(10) === 0) return pick([null, 5, "x", []]);
  const p = {};
  if (rnd(6)) p.protocol = pick(["openai", "anthropic", "google", "openai-responses", "OpenAI", " anthropic ", "", "grpc", 5]);
  if (rnd(3)) p.url = pick(["https://api.openai.com/v1", " https://api.deepseek.com/v1 ", "", "ftp://x", "not a url", "http://localhost:11434/v1", "https://generativelanguage.googleapis.com/v1beta/openai", 5]);
  if (rnd(3)) p.apiKey = pick(["sk-1", " sk-2 ", "", "  ", "${MY_KEY}", "${EMPTY_KEY}", "${MISSING_KEY}", "${1BAD}", "${OPENAI_API_KEY}", 7]);
  if (rnd(8)) {
    p.models = rnd(10) === 0 ? pick([[], "x", null]) : {};
    if (p.models && typeof p.models === "object" && !Array.isArray(p.models)) for (let i = 0; i < rnd(3) + (rnd(2)); i++) {
      const m = pick(MODELS);
      p.models[m] = rnd(5) === 0 ? pick([null, 5, "x", []]) : (rnd(3) === 0 ? { thinking: pick([undefined, { state: "enabled" }, { state: "bad" }, { format: "anthropic" }, { efforts: ["low"] }, 5]) } : {});
    }
  }
  if (rnd(4) === 0) p.headers = pick([{ "X-A": "1" }, [], "h"]);
  return p;
}
const ref = () => pick(["openai/gpt-4o", "anthropic/claude", "myp/m1", "deepseek/deepseek-chat", "ollama/m1", "0/m1", "a b/m1", "openai/", "/m1", "nope/x", "openai/missing", "inherit", "", " openai/gpt-4o ", 5, null, undefined, "google/x/y", "_placeholder/x"]);
function config() {
  if (rnd(25) === 0) return pick([null, 5, "x", [], undefined]);
  const c = {};
  if (rnd(8)) {
    c.model = rnd(12) === 0 ? sv() : { providers: rnd(12) === 0 ? sv() : Object.fromEntries(Array.from({ length: rnd(4) }, () => { const id = pick(PIDS); return [id, provider(id)]; })) };
  }
  if (rnd(6)) c.agent = rnd(15) === 0 ? pick([null, 5, "x", []]) : { model: ref(), ...(rnd(2) ? { params: pick([{}, { maxOutputTokens: pick([4096, "8192", " 12 ", "abc", 0, -5, 1e21, "99999999999999999999", null, 3.7]) }, { max_tokens: 100, tavilyApiKey: pick(["t", 5, "", null]) }, { tavily_api_key: "tt" }]) } : {}), ...(rnd(3) === 0 ? { subagents: { default: ref() } } : {}) };
  if (rnd(3) === 0) c.memory = rnd(8) === 0 ? sv() : { ...(rnd(2) ? { enabled: pick([true, false, "yes"]) } : {}), ...(rnd(2) ? { model: ref() } : {}), ...(rnd(4) === 0 ? { apiType: pick(["custom", " ", 5]) } : {}) };
  if (rnd(3) === 0) {
    const r = rnd(8) === 0 ? sv() : {};
    if (r && typeof r === "object" && !Array.isArray(r)) {
      if (rnd(2)) r.enabled = pick([true, false, "on"]);
      if (rnd(3) === 0) r.stats = pick([5, { enabled: pick([true, "x"]), baselineModel: pick([undefined, ref(), { provider: "openai", model: "gpt-4o" }, { provider: "", model: "x" }, 5, "bad"]), modelPricing: pick([undefined, 5, { "openai/gpt-4o": { input: pick([1, -1, "2", 0]), unit: pick(["$/百万 Token", "¥/百万 Token", "USD", undefined]) }, "bad": 5, "nope/x": {} }]) }]);
      if (rnd(3) === 0) r.scenarios = { default: pick([ref(), { id: "openai/gpt-4o" }]), think: ref() };
      if (rnd(4) === 0) r.fallback = { default: [ref(), ref()], x: "notarray" };
      if (rnd(4) === 0) r.tokenSaver = pick([5, { enabled: pick([true, false, "x", undefined]), judge: ref(), defaultTier: pick([undefined, "fast", 5, "missing"]), tiers: pick([undefined, {}, 5, { fast: { model: ref(), label: pick(["L", 5]) }, slow: 5, mid: { model: ref(), description: 3 } }]), subagent: pick([undefined, 5, { policy: "skip" }, { policy: "x" }]) }]);
    }
    c.router = r;
  }
  if (rnd(4) === 0) c.gateway = pick([5, { bindAddress: "0.0.0.0" }, { tokenPath: "x" }, {}]);
  if (rnd(4) === 0) c.tools = pick([5, { webSearch: { enabled: "x" } }, { webSearch: {} }, { webSearch: { region: "cn" } }, { zzz: 1 }]);
  if (rnd(3) === 0) c.webui = rnd(10) === 0 ? pick([5, null, "x", []]) : {
    ...(rnd(2) ? { runtime: rnd(8) === 0 ? pick([5, "x"]) : { ...(rnd(2) ? { serverPort: pick([4000, "4001", null, {}]) } : {}), ...(rnd(2) ? { databasePath: pick(["~/db.sqlite", "~", "~x", " ~/a/../b ", "/abs/db", "", 5]) } : {}), ...(rnd(3) === 0 ? { workspacesRoot: pick(["~/ws/", "/w", ""]) } : {}), ...(rnd(4) === 0 ? { httpsProxy: "http://p:1" } : {}), ...(rnd(5) === 0 ? { contextWindow: 1 } : {}), ...(rnd(4) === 0 ? { host: pick(["127.0.0.1", 5]) } : {}) } } : {}),
    ...(rnd(2) ? { officePreview: rnd(10) === 0 ? pick([5, null, "x"]) : { ...(rnd(2) ? { service: pick(["builtin", "LibreOffice", " none ", "", "onlyoffice", 5]) } : {}), ...(rnd(2) ? { spreadsheetMode: pick(["print", "interactive", "", " PRINT "]) } : {}), ...(rnd(3) === 0 ? { binaryPath: pick(["/usr/bin/soffice", 5]) } : {}) } } : {}),
  };
  if (rnd(5) === 0) c.proxy = pick(["http://proxy:8080", { url: "http://u:1" }, { url: "" }, 5, ""]);
  if (rnd(4) === 0) c.customEnv = pick([{ FOO: "bar", EMPTY: " ", NUM: 5, "12": "twelve" }, 5, { SERVER_PORT: "9999" }]);
  if (rnd(6) === 0) c.schemaVersion = pick([1, 2]);
  return c;
}
// Fixed cases first: responses-protocol memory API type, ollama default URL, inherit memory ref.
const FIXED = [
  { agent: { model: "openai/gpt-4o" }, memory: { enabled: true }, model: { providers: { openai: { protocol: "openai-responses", apiKey: "sk-1", models: { "gpt-4o": {} } } } } },
  { agent: { model: "ollama/m1" }, memory: { model: "inherit" }, model: { providers: { ollama: { protocol: "openai", models: { m1: null } } } } },
  { agent: { model: "google/x/y" }, model: { providers: { google: { protocol: "openai", models: { "x/y": {} } } } } },
];
const base = Object.fromEntries(ENV_NAMES.map((k) => [k, process.env[k]]));
const cases = [];
for (let i = 0; i < 2500; i++) {
  const env = {};
  for (const k of ENV_NAMES) if (rnd(3) === 0) env[k] = pick({ SERVER_PORT: ["4100", ""], VITE_PORT: ["5200"], HOST: ["::1"], PILOTDECK_MAX_OUTPUT_TOKENS: ["777", ""], TAVILY_API_KEY: ["tv"], EMPTY_KEY: ["  "] }[k] ?? ["k-" + k, " sp ", ""]);
  for (const k of ENV_NAMES) delete process.env[k];
  Object.assign(process.env, env);
  const cfg = clone(i < FIXED.length ? FIXED[i] : config()); // JSON view: undefined-valued keys would otherwise reach Object.entries
  const r = ref();
  const allowMissing = rnd(2) === 0;
  const masked = (() => { try { return pc.maskSecrets(cfg); } catch { return undefined; } })();
  let edited = clone(masked);
  if (edited && typeof edited === "object" && rnd(2)) { edited = JSON.parse(JSON.stringify(edited).replace(/"sk-1"/g, '"new"')); }
  const pid = pick(PIDS);
  const prov = cfg?.model?.providers?.[pid];
  const raw = pick([undefined, null, "", "schemaVersion: 1\n", 5, "é😀"]);
  const tilde = pick(["~", "~/", "~/a", "~/a/../../b", " ~/x/./y/ ", "~x", "/abs", "", 5, null, "~//a"]);
  cases.push({
    env, cfg: j(cfg), ref: j(r), allowMissing, pid, raw: j(raw), tilde: j(tilde), edited: j(edited),
    normalize: call(() => pc.normalizePilotDeckConfig(cfg)),
    validate: call(() => pc.validatePilotDeckConfig(cfg)),
    sanitize: call(() => pc.sanitizeProviderCredentials(clone(cfg))),
    resolve: call(() => pc.resolveModel(cfg, r, { allowMissing })),
    mask: call(() => pc.maskSecrets(cfg)),
    preserve: call(() => pc.preserveMaskedSecrets(edited, cfg)),
    unresolved: call(() => pc.hasUnresolvedMaskedSecrets(edited)),
    url: call(() => pc.resolveConfiguredProviderUrl(pid, prov)),
    apiKey: call(() => pc.resolveConfiguredProviderApiKey(pid, prov)),
    runtimeEnv: call(() => pc.buildRuntimeEnv(cfg)),
    memoryLlm: call(() => pc.buildMemoryLlmOptions(cfg)),
    memoryDefaults: call(() => pc.buildMemoryDefaults(cfg)),
    sync: call(() => pc.syncAgentModelWithRouter(clone(cfg))),
    revision: call(() => pc.configRevision(raw)),
    expand: call(() => pc.expandTilde(tilde)),
  });
}
for (const k of ENV_NAMES) { delete process.env[k]; if (base[k] !== undefined) process.env[k] = base[k]; }
const text = JSON.stringify({ home: process.env.HOME, pilotHome: process.env.PILOT_HOME, defaults: pc.buildDefaultPilotDeckConfig(), cases }) + "\n";
if (process.argv.includes("--check")) console.log(fs.existsSync(out) && fs.readFileSync(out, "utf8") === text ? "fixture stable" : "fixture DIFFERS");
else { fs.writeFileSync(out, text); console.log(`cases ${cases.length}`); }
