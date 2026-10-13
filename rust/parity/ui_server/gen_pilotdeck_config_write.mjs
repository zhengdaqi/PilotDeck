// Oracle for the write-side pure pipeline of ui/server/services/pilotdeckConfig.js: buildDefaultPilotDeckConfig,
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
const out = path.join(root, "rust/crates/ui-server-core/tests/fixtures/pilotdeck_config_write.json");
const tmp = "/tmp/pd-write-oracle";
fs.rmSync(tmp, { recursive: true, force: true }); fs.mkdirSync(tmp, { recursive: true });
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
  if (url.endsWith("/ui/server/services/pilotdeckConfig.js")) {
    r.source = String(r.source) + "\\nexport { purgeLegacyProviderDrafts, finalizeBootstrapPlaceholder, detectYamlIndent, valueAtPath };\\n";
  }
  return r;
}`));
const pc = await import(path.join(root, "ui/server/services/pilotdeckConfig.js"));

let seed = 4141;
function rnd(n) { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; }
const pick = (a) => a[rnd(a.length)];
const U = { $undef: 1 };
const j = (v) => (v === undefined ? U : JSON.parse(JSON.stringify(v)));
const call = (fn) => { try { return { v: j(fn()) }; } catch (e) { return { err: e.name, message: e.message }; } };
const clone = (v) => (v === undefined ? undefined : JSON.parse(JSON.stringify(v)));

const ENV_NAMES = ["OPENAI_API_KEY", "ANTHROPIC_API_KEY", "DEEPSEEK_API_KEY", "GEMINI_API_KEY", "MY_KEY", "EMPTY_KEY", "SERVER_PORT", "VITE_PORT", "HOST", "PILOTDECK_MAX_OUTPUT_TOKENS", "TAVILY_API_KEY"];
const PIDS = ["openai", "anthropic", "deepseek", "google", "ollama", "myp", "zhipu", "0", "_placeholder", "draft", "boot"];
const MODELS = ["gpt-4o", "m1", "claude", "", "deepseek-chat", "x/y"];
const sv = () => pick([undefined, null, 5, true, "", " ", "s", [], {}]);
function provider(pid) {
  if (rnd(10) === 0) return pick([null, 5, "x", []]);
  const p = {};
  if (rnd(6)) p.protocol = pick(["openai", "anthropic", "google", "openai-responses", "OpenAI", " anthropic ", "", "grpc", 5]);
  if (rnd(3)) p.url = pick(["https://api.openai.com/v1", " https://api.deepseek.com/v1 ", "", "ftp://x", "not a url", "http://localhost:11434/v1", "https://generativelanguage.googleapis.com/v1beta/openai", 5]);
  if (rnd(3)) p.apiKey = pick(["sk-1", " sk-2 ", "", "  ", "${MY_KEY}", "${EMPTY_KEY}", "${MISSING_KEY}", "${1BAD}", "${OPENAI_API_KEY}", 7, "PLACEHOLDER_RUN_ONBOARDING_TO_REPLACE", " PLACEHOLDER_RUN_ONBOARDING_TO_REPLACE "]);
  if (pid === "draft" && rnd(2)) return pick([{ protocol: "openai", url: "https://d.example/v1" }, { protocol: "openai", models: {} }, { protocol: "openai", apiKey: "" }]);
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
const ref = () => pick(["draft/m1", "boot/m1", "_placeholder/m1", " _placeholder/x ", "openai/gpt-4o", "anthropic/claude", "myp/m1", "deepseek/deepseek-chat", "ollama/m1", "0/m1", "a b/m1", "openai/", "/m1", "nope/x", "openai/missing", "inherit", "", " openai/gpt-4o ", 5, null, undefined, "google/x/y", "_placeholder/x"]);
function config() {
  if (rnd(25) === 0) return pick([null, 5, "x", [], undefined]);
  const c = {};
  if (rnd(8)) {
    c.model = rnd(12) === 0 ? sv() : { providers: rnd(12) === 0 ? sv() : Object.fromEntries(Array.from({ length: rnd(4) }, () => { const id = pick(PIDS); return [id, provider(id)]; })) };
  }
  if (rnd(6)) c.agent = rnd(15) === 0 ? pick([null, 5, "x", []]) : { model: ref(), ...(rnd(2) ? { params: pick([{}, { maxOutputTokens: pick([4096, "8192", " 12 ", "abc", 0, -5, 1e21, "99999999999999999999", null, 3.7]) }, { max_tokens: 100, tavilyApiKey: pick(["t", 5, "", null]) }, { tavily_api_key: "tt" }]) } : {}), ...(rnd(3) === 0 ? { subagents: { default: ref() } } : {}) };
  if (rnd(3) === 0) c.memory = rnd(8) === 0 ? sv() : { ...(rnd(3) === 0 ? { model: pick(["", "  ", "boot/m1"]) } : {}), ...(rnd(2) ? { enabled: pick([true, false, "yes"]) } : {}), ...(rnd(2) ? { model: ref() } : {}), ...(rnd(4) === 0 ? { apiType: pick(["custom", " ", 5]) } : {}) };
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

const KEY = "PLACEHOLDER_RUN_ONBOARDING_TO_REPLACE";
function bootstrapConfig() {
  const pref = () => pick(["/m1", "_placeholder/m1", "boot/m1", " boot/m1 ", "openai/gpt-4o", "inherit", "", 5, "boot", "/boot"]);
  const providers = {
    _placeholder: pick([{ protocol: "openai", apiKey: KEY, models: { m1: {} } }, 5, { models: { m1: {} } }]),
    boot: { protocol: "openai", url: "https://b.example/v1", apiKey: pick([KEY, " " + KEY + " ", "real"]), models: { m1: {} } },
    openai: { protocol: "openai", apiKey: "sk-1", models: { "gpt-4o": {} } },
  };
  if (rnd(2)) delete providers._placeholder;
  if (rnd(4) === 0) providers[""] = { apiKey: KEY, models: { m1: {} } };
  const c = { model: { providers }, agent: { model: pick(["openai/gpt-4o", " openai/gpt-4o ", "_placeholder/m1", "boot/m1", "openai/nope"]) } };
  if (rnd(2)) c.agent.subagents = { default: pref() };
  if (rnd(2)) c.memory = { enabled: true, model: pref() };
  if (rnd(4)) {
    c.router = { scenarios: { default: pref(), think: pref(), obj: { id: "boot/m1" } }, fallback: { default: [pref(), pref(), null], x: "s" } };
    if (rnd(2)) c.router.stats = { modelPricing: { "boot/m1": { input: 1 }, "openai/gpt-4o": { input: 2 }, "_placeholder/m1": {} }, baselineModel: pick(["boot/m1", "openai/gpt-4o", { provider: "boot", model: "m1", note: 1 }, { provider: " _placeholder ", label: "x" }, { provider: "openai", model: "gpt-4o" }, 5]) };
    if (rnd(2)) c.router.tokenSaver = pick([{ judge: pref(), tiers: { fast: { model: pref() }, slow: 5, mid: { model: "openai/gpt-4o" } } }, { judge: 5, tiers: [] }]);
    if (rnd(5) === 0) c.router = pick([5, [], "x"]);
  }
  return c;
}
function mutateCopy(cfg) {
  const p = clone(cfg);
  if (!p || typeof p !== "object" || Array.isArray(p)) return p;
  const provs = p.model?.providers;
  if (provs && typeof provs === "object" && !Array.isArray(provs) && rnd(2)) {
    const k = pick(Object.keys(provs).concat(["draft"]));
    if (rnd(2)) provs[k] = { protocol: "openai", url: "https://d.example/v1" }; else if (provs[k] && typeof provs[k] === "object") provs[k].protocol = "anthropic";
  }
  return p;
}
const ENVK = ENV_NAMES;
const saved = Object.fromEntries(ENVK.map((k) => [k, process.env[k]]));
const cases = [];
for (let i = 0; i < 1500; i++) {
  const env = {};
  for (const k of ENVK) if (rnd(3) === 0) env[k] = pick({ SERVER_PORT: ["4100"], HOST: ["::1"], EMPTY_KEY: ["  "] }[k] ?? ["k-" + k, ""]);
  for (const k of ENVK) delete process.env[k];
  Object.assign(process.env, env);
  const cfg = clone(rnd(3) === 0 ? bootstrapConfig() : config());
  const prevKind = rnd(4);
  const prev = prevKind === 0 ? pick([undefined, null, 5]) : prevKind === 1 ? clone(cfg) : mutateCopy(cfg);
  const cfgPath = path.join(tmp, `c${i}`, "pilotdeck.yaml");
  process.env.PILOTDECK_CONFIG_PATH = cfgPath;
  let write;
  try {
    const r = await pc.writePilotDeckConfig(clone(cfg), { previousConfig: clone(prev) });
    write = { v: j({ raw: r.raw, validation: r.validation, config: r.config, disk: fs.readFileSync(cfgPath, "utf8") }) };
  } catch (e) { write = { err: e.name, message: e.message, ...(e.validation ? { validation: j(e.validation) } : {}) }; }
  delete process.env.PILOTDECK_CONFIG_PATH;
  const recKind = rnd(3);
  const record = {
    exists: rnd(4) !== 0, configPath: "/cfg/" + i + ".yaml", raw: pick(["", "a: 1\n", "x\n", "schemaVersion: 1\n"]),
    config: recKind === 0 ? cfg : (() => { try { return pc.normalizePilotDeckConfig(cfg); } catch { return cfg; } })(),
    rawYaml: pick([null, {}, cfg, clone(prev), 5, [], [1, { apiKey: "s" }]]),
    parseError: rnd(6) === 0 ? pick(["bad indent", "Nested mappings are not allowed", ""]) : null,
  };
  const reload = pick([null, undefined, { ok: true }, { ok: false, error: "x" }]);
  const vkeys = rnd(4) === 0 ? ["router", "fallback", "default", pick(["0", 1, "5", "01"])] : Array.from({ length: rnd(4) + 1 }, () => pick(["model", "providers", "openai", "models", "router", "fallback", "default", "0", "1", "agent", "x", 0, 1]));
  const yamlText = pick(["", "a: 1\n", "a:\n    b: 1\n", "a:\n\tb: 1\n", "# c\n  # x\na:\n      b\n", "a:\n                  deep: 1\n", "x:\n   y: 1\r\n   z: 2\n", "  \n a: 1\n", "a:\n\u00a0 b: 1\n", 5, null, undefined]);
  cases.push({
    env, cfg: j(cfg), prev: j(prev), record: j(record), reload: j(reload), vkeys, yamlText: j(yamlText),
    purge: call(() => pc.purgeLegacyProviderDrafts(clone(cfg), clone(prev))),
    finalize: call(() => pc.finalizeBootstrapPlaceholder(clone(cfg))),
    write,
    serialize: call(() => pc.serializePilotDeckConfigResponse(clone(record), reload)),
    toYaml: call(() => pc.configToYaml(cfg)),
    rawMasked: call(() => pc.rawYamlToMaskedString(clone(record.rawYaml))),
    indent: call(() => pc.detectYamlIndent(yamlText)),
    valueAt: call(() => pc.valueAtPath(cfg, vkeys)),
  });
}
for (const k of ENVK) { delete process.env[k]; if (saved[k] !== undefined) process.env[k] = saved[k]; }
const text = JSON.stringify({ home: process.env.HOME, pilotHome: process.env.PILOT_HOME, cases }) + "\n";
if (process.argv.includes("--check")) console.log(fs.existsSync(out) && fs.readFileSync(out, "utf8") === text ? "fixture stable" : "fixture DIFFERS");
else { fs.writeFileSync(out, text); console.log(`cases ${cases.length}`); }
