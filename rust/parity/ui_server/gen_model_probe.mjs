// Oracle for src/model/providerEndpoint.ts, src/network/fetch.ts normalizeNetworkError and
// ui/server/services/modelConnectionProbe.js (helpers re-exported by a load hook; probeModelConnection driven
// with a scripted global fetch and retries off). VITEST=1 fixes the probe colour to red, as in the TS tests.
// Usage: node --experimental-transform-types rust/parity/ui_server/gen_model_probe.mjs [--check]
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const out = path.join(root, "rust/crates/ui-server-core/tests/fixtures/model_probe.json");
process.env.VITEST = "1";
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
  if (url.endsWith("/ui/server/services/modelConnectionProbe.js")) {
    return { ...r, source: String(r.source) + "\\nexport { hasErrorFinish, extractProbeText, stripThinking, isIncomplete, responseDetail, looksLikeImageUnsupported, classifyProbeError, requestFor, encodeSolidPng };\\n" };
  }
  return r;
}`));
const ep = await import(path.join(root, "src/model/providerEndpoint.ts"));
const net = await import(path.join(root, "src/network/fetch.ts"));
const probe = await import(path.join(root, "ui/server/services/modelConnectionProbe.js"));

let seed = 3434;
function rnd(n) { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; }
const pick = (a) => a[rnd(a.length)];
const U = undefined;
const enc = (v) => JSON.parse(JSON.stringify(v === undefined ? { $undef: 1 } : v));
const v = { endpoints: [], google: [], shapes: [], network: [], helpers: [], requests: [], probes: [], png: null };

const PROTOCOLS = ["openai", "openai-responses", "anthropic", "google", "other"];
const BASES = ["https://api.openai.com", "https://api.openai.com/", "https://api.openai.com/v1", "https://x.ai/v1/", " https://h/api/v2beta3 ", "https://h/api/V1", "https://h/v1/chat/completions",
  "https://h/v1/messages", "https://h/responses", "https://h/v1beta/models/gemini:generateContent", "https://h/models/gemini-pro:streamGenerateContent", "https://h/Models/X:GENERATECONTENT", "", "  ", "not a url",
  "http://h:8080/a//b/", "https://h/vbeta", "https://h/v1x", "https://h/%7Euser/", "https://user:pw@h/p?q=1#f", "https://h/models", "https://h/v1/models", "file:///v1", "https://h/v1betax", "https://h/v2beta", "https://h/foo/gemini:generateContent", "https://h/foo/models:generateContent"];
const MODELS = [U, "", "gpt-4o", "google/gemini-3-pro", "gemini-3.1-flash", "gemini-3.1-flash-preview", "gemini-3.1-flash-lite", " google/gemini-3-flash ", "a b/c?d#e&f=g", "émoji☃", "gemini-3.1-pro", "a!*'()~b"];
for (let i = 0; i < 1500; i++) {
  const input = { protocol: pick(PROTOCOLS), baseUrl: pick(BASES), ...(rnd(3) ? { model: pick(MODELS) } : {}), ...(rnd(5) ? {} : { googleMethod: pick(["", "streamGenerateContent", "countTokens"]) }) };
  v.endpoints.push({ input: enc(input), chat: ep.buildProviderChatEndpointCandidates(input), chatOne: ep.buildProviderChatEndpoint(input), models: ep.buildProviderModelsEndpointCandidates(input), modelsOne: ep.buildProviderModelsEndpoint(input) });
}
for (const m of [...MODELS, "google/", "google/gemini-3-pro ", "GOOGLE/gemini-3-pro"]) v.google.push({ m: enc(m), out: ep.normalizeGoogleProbeModel(m) });
const BODIES = [null, U, 0, "s", [], {}, { content: [] }, { type: "message" }, { candidates: [] }, { candidates: {} }, { object: "response" }, { output: [] }, { output_text: "" }, { output_text: 5 }, { choices: [] }, { choices: {} }, { models: [] }, { data: [] }, { data: {} }];
for (const p of PROTOCOLS) for (const b of BODIES) v.shapes.push({ p, b: enc(b), chat: ep.isExpectedProviderResponseShape(p, b), models: ep.isExpectedProviderModelsResponseShape(p, b) });
const NET_ERRS = [{ message: "getaddrinfo ENOTFOUND h" }, { message: "x", code: "EAI_AGAIN" }, { message: "fetch failed", cause: { code: "ECONNRESET" } }, { message: "socket hang up" }, { message: "terminated" },
  { message: "connect ECONNREFUSED 127.0.0.1:1" }, { message: "ETIMEDOUT" }, { message: "Headers Timeout Error" }, { message: "unable to verify the first certificate" }, { message: "TLS handshake" }, { message: "SSL" },
  { message: "proxy auth" }, { message: "This operation was aborted" }, { message: "boom" }, { message: "DNS lookup" }, { message: "x", code: 5, cause: { code: "ECONNREFUSED" } }, { message: "x", cause: "ECONNREFUSED" }, { str: "plain string econnreset" }, { str: null }];
for (const e of NET_ERRS) {
  const err = e.str !== undefined ? e.str : Object.assign(new Error(e.message), e.code !== undefined ? { code: e.code } : {}, e.cause !== undefined ? { cause: e.cause } : {});
  const n = net.normalizeNetworkError(err);
  v.network.push({ e, code: n.code, message: n.message });
}
// helpers
const TEXTS = ["1", " 1 ", "", "<think>x</think>1", "<THINKING>a\nb</thinking> red", "<think>a</thinking>b", "<thinking>open", "x<think>a</think>y<think>b", "<think><think>a</think></think>z", "<Think>a</THINK>Red!", "red", "Blue", "reddish", "  ", "<think >no</think>"];
const partsOf = () => Array.from({ length: rnd(3) }, () => pick([{ type: "text", text: pick(TEXTS) }, { type: "image" }, { text: pick(TEXTS) }, { type: "text", text: 5 }, null, "str", { thought: true, text: "1" }, { output_text: pick(TEXTS) }]));
const bodyFor = (p) => {
  const r = rnd(12);
  if (r === 0) return pick([null, "x", 5, {}, { error: "e" }, { status: "failed" }, { status: "incomplete" }, { incomplete_details: {} }]);
  const extra = rnd(4) ? {} : pick([{ error: { message: "m" } }, { status: "failed" }, { stop_reason: pick(["error", "ERROR", "max_tokens", "pause_turn", "end_turn"]) }, { status: "incomplete" }]);
  if (p === "anthropic") return { ...(rnd(6) ? { content: rnd(8) ? partsOf() : pick(["abc", 5, {}, null]) } : { type: "message" }), ...extra };
  if (p === "google") return { candidates: rnd(8) ? Array.from({ length: rnd(3) }, () => pick([{ content: { parts: partsOf() }, finishReason: pick([U, "STOP", "MAX_TOKENS", "error", "Error"]) }, null, { content: null }, { content: { parts: "x" } }])) : pick([{}, "abc", 5]), ...extra };
  if (p === "openai-responses") return { ...(rnd(3) ? { output_text: pick([...TEXTS, 5, null]) } : {}), output: rnd(8) ? Array.from({ length: rnd(3) }, () => pick([{ type: "reasoning", content: partsOf() }, { type: "message", content: partsOf() }, { content: "x" }, null])) : pick(["abc", 5, {}]), ...(rnd(3) ? {} : { object: "response" }), ...extra };
  return { choices: rnd(8) ? Array.from({ length: rnd(3) }, () => pick([{ message: { content: pick([...TEXTS, partsOf(), 5, null]) }, finish_reason: pick([U, "stop", "length", "error"]) }, { text: pick(TEXTS) }, null, { message: null }])) : pick([{}, "abc", 5]), ...extra };
};
const call = (fn) => { try { return { v: enc(fn()) }; } catch (e) { return { throw: `${e.constructor.name}: ${e.message}` }; } };
for (let i = 0; i < 2500; i++) {
  const p = pick(PROTOCOLS);
  const b = bodyFor(p);
  v.helpers.push({ p, b: enc(b), err: call(() => probe.hasErrorFinish(b, p)), text: call(() => probe.extractProbeText(b, p)), inc: call(() => probe.isIncomplete(b, p)) });
}
for (const t of [...TEXTS, U, null, 5, {}]) v.helpers.push({ strip: enc(t), out: probe.stripThinking(t) });
const DETAILS = ["Image input is not supported for this model", "vision not enabled", "multimodal: not available", "model does not support images", "unsupported image", "content.type: allowed values: ['text']", 'Content type allowed value : [ "text" ]',
  "Invalid API key", "api_key missing", "Authentication failed", "unauthorised", "Unauthorized", "model gpt-x not found", "unknown model", "The model does not exist", "boom", "", 5, null, "image" + "x".repeat(61) + "not supported", "image" + "x".repeat(60) + "not supported", "image\nnot supported", "IMAGE NOT SUPPORTED", "image" + "x".repeat(59) + "\u{1F600}not supported", "image" + "x".repeat(58) + "\u{1F600}not supported", "ſupported image", "APIKEY"];
for (const d of DETAILS) for (const s of [U, 200, 401, 403, 404, 500]) v.helpers.push({ d: enc(d), s: enc(s), img: probe.looksLikeImageUnsupported(d), cls: probe.classifyProbeError(d, s) });
for (const t of ['{"error":{"message":"m"}}', '{"error":{"type":"t"}}', '{"error":{}}', '{"message":"x"}', '{"message":5}', '{"error":"str"}', "null", "5", "not json", "", '{"error":{"message":""},"message":"fallback"}', "[]"]) {
  v.helpers.push({ rt: t, detail: enc(probe.responseDetail(t, { status: 418, statusText: "I'm a teapot" })) });
}
for (const c of ["red", "Red", "", null, U]) for (const a of ["Red.", "it is RED", "blue", "", null, 5]) v.helpers.push({ ans: enc(a), col: enc(c), ok: probe.isValidImageColorAnswer(a, c) });
// requestFor
const imageProbe = probe.createImageCapabilityProbe();
for (let i = 0; i < 200; i++) {
  const input = { protocol: pick(PROTOCOLS), apiKey: pick(["", "k", U]), ...(rnd(4) ? { model: pick(MODELS) } : {}), image: rnd(2) === 0, maxTokens: pick([1, 4096, U]), imageProbe: rnd(2) ? imageProbe : null };
  let r;
  try { r = { v: JSON.parse(JSON.stringify(probe.requestFor(input)).split(imageProbe.data).join("<png>")) }; } catch (e) { r = { throw: `${e.constructor.name}: ${e.message}` }; }
  v.requests.push({ input: enc({ ...input, imageProbe: input.imageProbe ? "probe" : null }), out: r });
}
// PNG: record the decoded rows (deflate output differs between zlib implementations; pixels must not).
const png = Buffer.from(imageProbe.data, "base64");
const idatLen = png.readUInt32BE(33 + 0);
v.png = { color: imageProbe.color, prompt: imageProbe.prompt, header: png.subarray(0, 33).toString("base64"), raw: zlib.inflateSync(png.subarray(41, 41 + idatLen)).toString("base64"), iend: png.subarray(png.length - 12).toString("base64") };
// probeModelConnection with a scripted fetch
const STATUS_TEXT = { 200: "OK", 400: "Bad Request", 401: "Unauthorized", 404: "Not Found", 405: "Method Not Allowed", 429: "Too Many Requests", 500: "Internal Server Error" };
for (let i = 0; i < 1500; i++) {
  const protocol = pick(PROTOCOLS.slice(0, 4));
  const input = { protocol, baseUrl: pick(BASES.slice(0, 12)), ...(rnd(5) ? {} : { endpointUrl: pick(["https://e/x", ""]) }), ...(rnd(3) ? { apiKey: pick(["", "k"]) } : {}), model: pick(MODELS), image: rnd(3) === 0, ...(rnd(4) ? {} : { maxTokens: pick([1, 16]) }), retryPolicy: { maxRetries: 0 } };
  const script = Array.from({ length: 1 + rnd(3) }, () => {
    const r = rnd(10);
    if (r < 1) return { throw: pick(NET_ERRS.filter((e) => !e.str)) };
    const status = pick([200, 200, 200, 400, 401, 404, 405, 429, 500]);
    const text = status === 200 ? (rnd(6) ? JSON.stringify(bodyFor(protocol)) : pick(["not json", "", "null"])) : pick(['{"error":{"message":"Invalid API key"}}', '{"error":{"message":"image input not supported"}}', "model foo not found", "", '{"message":"m"}', "plain"]);
    return { status, text };
  });
  const log = [];
  let k = 0;
  globalThis.fetch = async (url, init) => {
    log.push(`fetch ${url} ${JSON.stringify(init.headers)} ${String(init.body).split(imageProbe.data).join("<png>")}`);
    const s = script[k++];
    if (!s) return new Response("", { status: 404, statusText: "Not Found" });
    if (s.throw) { const e = s.throw; throw Object.assign(new Error(e.message), e.code !== undefined ? { code: e.code } : {}, e.cause !== undefined ? { cause: e.cause } : {}); }
    return new Response(s.text, { status: s.status, statusText: STATUS_TEXT[s.status] });
  };
  let result;
  try { result = { v: await probe.probeModelConnection(input) }; } catch (e) { result = { throw: `${e.constructor.name}: ${e.message}` }; }
  v.probes.push({ input: enc(input), script, log, result: enc(result) });
}
const text = JSON.stringify(v) + "\n";
if (process.argv.includes("--check")) console.log(fs.existsSync(out) && fs.readFileSync(out, "utf8") === text ? "fixture stable" : "fixture DIFFERS");
else { fs.writeFileSync(out, text); console.log(Object.entries(v).map(([k, x]) => `${k} ${Array.isArray(x) ? x.length : 1}`).join(", ")); }
