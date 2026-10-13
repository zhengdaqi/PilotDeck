// Oracle for the pilotdeckConfig dependencies: src/model/catalog (PROVIDER_CATALOG snapshot + lookupCatalogProvider),
// src/model/config/providerCredentialScope.ts, src/model/thinking/settings.ts, src/pilot/config/parseGatewayConfig.ts
// (gateway + adapters) and src/pilot/config/parseToolsConfig.ts.
// Writes the catalog snapshot (a Rust source asset) and the fixture. Usage: node --experimental-transform-types gen_config_parsers.mjs [--check]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const catalogOut = path.join(root, "rust/crates/ui-server-core/src/provider_catalog.json");
const out = path.join(root, "rust/crates/ui-server-core/tests/fixtures/config_parsers.json");
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
const cat = await import(path.join(root, "src/model/catalog/index.ts"));
const scope = await import(path.join(root, "src/model/config/providerCredentialScope.ts"));
const thinking = await import(path.join(root, "src/model/thinking/settings.ts"));
const gw = await import(path.join(root, "src/pilot/config/parseGatewayConfig.ts"));
const tools = await import(path.join(root, "src/pilot/config/parseToolsConfig.ts"));

let seed = 3939;
function rnd(n) { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; }
const pick = (a) => a[rnd(a.length)];
const U = { $undef: 1 };
const enc = (v) => (v === undefined ? U : v);
const call = (fn) => { try { return { v: enc(fn()) }; } catch (e) { return { err: e.name, message: e.message }; } };
// JSON.stringify drops undefined-valued keys exactly as the Rust port omits them.
const j = (v) => (v === undefined ? U : JSON.parse(JSON.stringify(v)));

const catalogText = JSON.stringify(cat.PROVIDER_CATALOG, null, 1) + "\n";
const ids = [...Object.keys(cat.PROVIDER_CATALOG), "nope", "", "constructor", "toString", "__proto__", "hasOwnProperty", "OPENAI"];
const PROTOCOLS = [undefined, null, "openai", "anthropic", "google", "openai-responses", "other", 5];
const URLS = [undefined, null, "", "  ", 5, "https://api.openai.com/v1", "https://api.openai.com/v1/", "https://api.openai.com/v1//", " https://api.openai.com/v1 ", "HTTPS://API.OPENAI.COM/v1", "https://api.openai.com:443/v1",
  "https://api.openai.com/v1?x=1", "ftp://api.openai.com/v1", "not a url", "https://api.anthropic.com", "https://api.anthropic.com/", "https://generativelanguage.googleapis.com/v1beta/openai", "https://generativelanguage.googleapis.com",
  "http://localhost:11434/v1", "https://api.deepseek.com/v1", "https://api.deepseek.com", "https://dashscope.aliyuncs.com/compatible-mode/v1", "https://api.moonshot.cn/v1", "https://openrouter.ai/api/v1", "https://api.z.ai/api/paas/v4", "https://user@api.openai.com/v1", "https://api.openai.com/./v1", "https://api.openai.com/a/../v1"];
const scopeCases = [];
for (let i = 0; i < 3000; i++) {
  const id = pick(ids);
  const own = Object.hasOwn(cat.PROVIDER_CATALOG, id) ? cat.PROVIDER_CATALOG[id] : undefined;
  const protocol = own && rnd(2) ? own.protocol : pick(PROTOCOLS);
  const du = own ? (id === "google" && protocol === "openai" && rnd(2) ? "https://generativelanguage.googleapis.com/v1beta/openai" : own.defaultUrl) : "https://x";
  const url = rnd(2) ? pick([du, du + "/", " " + du + " ", du.replace("https://", "HTTPS://"), du + "/x", du.replace(/\/v\d+$/, "")]) : pick(URLS);
  const p = cat.lookupCatalogProvider(id);
  scopeCases.push({ id, protocol: enc(protocol), url: enc(url), defaultUrl: enc(scope.resolveCatalogProviderDefaultUrl(id, protocol)), envVar: enc(scope.resolveCatalogProviderApiKeyEnvVar(id, protocol, url)),
    lookup: p && typeof p === "object" && Object.hasOwn(cat.PROVIDER_CATALOG, id) ? id : null, lookupProtocol: enc(p?.protocol), lookupDefaultUrl: enc(p?.defaultUrl), lookupEnv: enc(p?.apiKeyEnvVar) });
}
const EFF = ["low", "medium", "high", "xhigh", "max", "LOW", "", 5, null];
const thinkingCases = [];
for (let i = 0; i < 1500; i++) {
  let raw;
  const r = rnd(10);
  if (r === 0) raw = pick([undefined, null, "x", 5, [], true]);
  else {
    raw = {};
    if (rnd(2)) raw.state = pick(["default", "enabled", "disabled", "on", null, undefined, 1]);
    if (rnd(2)) raw.efforts = rnd(6) ? Array.from({ length: rnd(5) }, () => pick(EFF)) : pick(["low", null, {}, undefined]);
    if (rnd(2)) raw.format = pick(["provider", "openai", "thinking-type", "qwen-cloud", "qwen-local", "anthropic", "google", "openrouter", "server-default", "x", null, undefined]);
  }
  const protocol = pick(["openai", "anthropic", "google", "openai-responses", "other", ""]);
  thinkingCases.push({ raw: j(raw), rawHasUndef: raw && typeof raw === "object" ? Object.keys(raw).filter((k) => raw[k] === undefined) : [], protocol, out: call(() => thinking.parseThinkingSettings(raw, protocol)) });
}
const V = () => pick([undefined, null, true, false, 0, 7, 1.5, -3, "s", "", " x ", [], {}, ["a", 5, "b"], { a: 1 }, 18789, "127.0.0.1", "0.0.0.0", Infinity]);
function randRecord(keys) {
  const o = {};
  for (const k of keys) if (rnd(3) === 0) o[k] = V();
  if (rnd(5) === 0) o[pick(["zzz", "extra", "Enabled"])] = V();
  return o;
}
const gwCases = [], adCases = [], toolCases = [];
for (let i = 0; i < 1500; i++) {
  const raw = rnd(8) === 0 ? V() : randRecord(["bindAddress", "tokenPath", "port", "idleSessionTimeoutMinutes", "idleSweepIntervalSeconds", "memoryDiagnostics", "staticAssetsPath", "maxPerSessionMcpInstances"]);
  const d = []; const r = gw.parseGatewayConfig(raw, d);
  gwCases.push({ raw: j(raw), out: j(r), d });
}
const PLAT = ["telegram", "discord", "slack", "matrix", "mattermost", "signal", "whatsapp", "bluebubbles", "dingtalk", "wecom", "wecomCallback", "email", "sms", "homeassistant", "apiServer", "webhook"];
for (let i = 0; i < 1500; i++) {
  let raw;
  if (rnd(8) === 0) raw = V();
  else {
    raw = {};
    for (const k of ["cli", "tui", "feishu", "weixin", "qq", ...PLAT]) if (rnd(4) === 0) raw[k] = rnd(4) === 0 ? V() : randRecord(k === "feishu" ? ["enabled", "appId", "appSecret", "encryptKey", "verifyToken", "defaultSessionLabel", "connectionMode", "domainName", "permissionMode"]
      : k === "qq" ? ["enabled", "appId", "clientSecret", "allowGroups", "triggerPrefixes", "maxMessageLength"] : k === "cli" || k === "tui" ? ["autoConnectServer"] : ["enabled", "token", "apiKey", "webhookUrl", "extra"]);
    if (raw.feishu && typeof raw.feishu === "object" && rnd(2)) Object.assign(raw.feishu, { connectionMode: pick(["stream", "webhook", "x"]), domainName: pick(["feishu", "lark", "x"]), permissionMode: pick(["default", "bypassPermissions", "x"]) });
  }
  const d = []; const r = gw.parseAdaptersConfig(raw, d);
  adCases.push({ raw: j(raw), out: j(r), d });
}
const CP = ["auth", "name", "method", "queryParam", "apiKeyParam", "resultsPath", "titleField", "urlField", "snippetField", "sourceField", "publishedAtField"];
for (let i = 0; i < 2500; i++) {
  let raw;
  if (rnd(10) === 0) raw = V();
  else {
    raw = {};
    if (rnd(6)) {
      if (rnd(8) === 0) raw.webSearch = V();
      else {
        const ws = {};
        if (rnd(2)) ws.enabled = pick([true, true, false, "yes", undefined]);
        if (rnd(2)) ws.provider = pick(["glm", "tavily", "serper", "brave", "baidu", "bocha", "exa", "serpapi", "custom", "google", 5]);
        if (rnd(3) === 0) ws.searchEngine = pick(["google", "bing", "baidu", "duckduckgo", "yahoo", "yandex", "ask", 1]);
        if (rnd(2)) ws.apiKey = pick(["k", " k ", "", "  ", 5]);
        if (rnd(2)) ws.endpoint = pick(["https://e/x", " http://e ", "", "ftp://e", "nope", "https://", 5, "HTTPS://E/Y"]);
        if (rnd(3) === 0) ws.customProvider = rnd(5) === 0 ? V() : Object.fromEntries(CP.filter(() => rnd(3) === 0).map((k) => [k, k === "auth" ? pick(["bearer", "bodyApiKey", "queryApiKey", "none", "x"]) : k === "method" ? pick(["GET", "POST", "get"]) : pick(["v", " v ", "", 5])]).concat(rnd(4) === 0 ? [["unknownX", 1]] : []));
        if (rnd(6) === 0) ws.region = "cn";
        if (rnd(6) === 0) ws.tavilyApiKey = "t";
        if (rnd(6) === 0) ws.other = 1;
        raw.webSearch = ws;
      }
    }
    if (rnd(5) === 0) raw[pick(["fetch", "zzz"])] = 1;
  }
  const d = []; const r = tools.parseToolsConfig(raw, d);
  toolCases.push({ raw: j(raw), out: j(r), d });
}
const text = JSON.stringify({ scope: scopeCases, thinking: thinkingCases, gateway: gwCases, adapters: adCases, tools: toolCases }) + "\n";
if (process.argv.includes("--check")) {
  const ok = fs.existsSync(out) && fs.readFileSync(out, "utf8") === text && fs.readFileSync(catalogOut, "utf8") === catalogText;
  console.log(ok ? "fixture stable (catalog snapshot matches TS)" : "fixture DIFFERS");
} else { fs.writeFileSync(catalogOut, catalogText); fs.writeFileSync(out, text); console.log(`catalog ${Object.keys(cat.PROVIDER_CATALOG).length} providers; scope ${scopeCases.length}, thinking ${thinkingCases.length}, gateway ${gwCases.length}, adapters ${adCases.length}, tools ${toolCases.length}`); }
