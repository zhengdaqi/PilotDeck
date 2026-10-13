// TS/JS oracle for ui/server/services/modelReferences.js (all exports), mulberry32-seeded.
// Usage: node rust/parity/ui_server/gen_model_references.mjs [--check]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const m = await import(path.join(root, "ui/server/services/modelReferences.js"));

let seed = 4242;
const rnd = (n) => { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) % n; };
const pick = (a) => a[rnd(a.length)];
const maybe = (p = 2) => rnd(p) === 0;
const provIds = ["HX API", "deepseek", "p", "0", "7", " sp ", "", "a/b"];
const modelIds = ["m1", "m2", "qwen3.6-27b", "x/y", "", "3", " m "];
const refStr = () => pick([`${pick(provIds)}/${pick(modelIds)}`, `${pick(provIds)}/${pick(modelIds)}`, " HX API/m1 ", "noslash", "/m1", "p/", "", "inherit"]);
const refVal = () => {
  const k = rnd(9);
  if (k < 5) return refStr();
  if (k === 5) return { provider: pick(provIds), model: pick(modelIds), ...(maybe() ? { extra: 1 } : {}) };
  if (k === 6) return { id: refStr(), ...(maybe() ? { providerId: pick(provIds) } : {}), ...(maybe() ? { modelId: pick(modelIds) } : {}) };
  if (k === 7) return pick([null, 3, [], true, { provider: "", model: "m1", id: "p/m2" }]);
  return { providerId: pick(provIds), modelId: pick(modelIds) };
};
const obj = (keys, val) => { const o = {}; for (const k of keys) if (maybe(3) !== true || true) { if (rnd(3)) o[k] = val(k); } return o; };
function config() {
  const c = {};
  if (rnd(8)) {
    c.model = { providers: {} };
    const n = rnd(4);
    for (let i = 0; i < n; i++) {
      const id = pick(provIds);
      c.model.providers[id] = rnd(10) ? { protocol: "openai", models: obj(modelIds, () => ({})) } : pick([{ models: [] }, "str", null, {}]);
    }
    if (rnd(25) === 0) c.model.providers = [{ models: { m1: {} } }];
  }
  if (rnd(6)) c.agent = rnd(15) ? { model: refVal(), ...(maybe() ? { subagents: rnd(8) ? { default: refVal() } : pick(["s", 0]) } : {}) } : pick(["str", [], 5]);
  if (maybe()) c.memory = rnd(12) ? { enabled: true, ...(maybe(3) ? {} : { model: refVal() }) } : "mem";
  if (rnd(5)) {
    const r = {};
    if (rnd(4)) r.enabled = pick([true, false, undefined, 0]);
    if (rnd(4)) r.scenarios = rnd(10) ? obj(["default", "think", "1", "long", "background"], refVal) : pick([[], "s"]);
    if (rnd(3)) { r.fallback = { maxFallbacks: 2 }; for (const k of ["default", "think", "2"]) if (rnd(2)) r.fallback[k] = rnd(8) ? Array.from({ length: rnd(4) }, refVal) : "x"; }
    if (rnd(3)) r.tokenSaver = rnd(12) ? { ...(maybe() ? { enabled: pick([true, false]) } : {}), ...(rnd(3) ? { judge: refVal() } : {}), ...(rnd(3) ? { tiers: obj(["fast", "smart", "9"], () => (rnd(8) ? { model: refVal() } : pick([null, "t"]))) } : {}) } : pick(["ts", 1]);
    if (rnd(3)) r.stats = { ...(rnd(2) ? { modelPricing: obj([`${pick(provIds)}/${pick(modelIds)}`, `${pick(provIds)}/${pick(modelIds)}`, "nos", "12"], () => ({ input: pick([1, 0.5, 2]) })) } : {}), ...(rnd(2) ? { baselineModel: refVal() } : {}) };
    c.router = rnd(15) ? r : pick(["r", null]);
  }
  return JSON.parse(JSON.stringify(c));
}
const wrap = (f) => { try { const r = f(); return r === undefined ? { __undefined: true } : JSON.parse(JSON.stringify(r)); } catch (e) { return { __error: `${e?.constructor?.name}: ${e?.message}` }; } };
const cases = [];
for (let i = 0; i < 8000; i++) {
  const c = config();
  const providers = c?.model?.providers && typeof c.model.providers === "object" ? Object.keys(c.model.providers) : [];
  const t = { providerId: pick([...providers, ...providers, pick(provIds), "", "__proto__", "constructor"]), ...(rnd(3) ? { modelId: pick([...modelIds, "m1", "m1", ""]) } : {}) };
  const k = rnd(4);
  if (k === 0) cases.push({ fn: "find", config: c, target: t, out: wrap(() => m.findModelReferences(structuredClone(c), t)) });
  else if (k === 1) {
    const provs = c?.model?.providers && typeof c.model.providers === "object" && !Array.isArray(c.model.providers) ? c.model.providers : {};
    const all = Object.entries(provs).flatMap(([id, p]) => (p && typeof p.models === "object" && p.models && !Array.isArray(p.models) ? Object.keys(p.models).map((mm) => [id, mm]) : []));
    const tp = all.length && rnd(5) ? pick(all) : null;
    const tt = tp ? { providerId: tp[0], ...(rnd(3) ? { modelId: tp[1] } : {}) } : t;
    // Seed the config with references to the target so the repair paths run.
    if (tp && rnd(3)) { c.agent = { model: `${tp[0]}/${tp[1]}`, ...(c.agent && typeof c.agent === "object" && !Array.isArray(c.agent) ? { subagents: c.agent.subagents } : {}) }; }
    const others = all.filter(([a, b]) => !(tt.providerId === a && (!tt.modelId || tt.modelId === b))).map(([a, b]) => `${a}/${b}`);
    const opts = rnd(4) ? { replacement: rnd(2) && others.length ? pick(others) : pick(["", "deepseek/m1", "HX API/m2", " p/m1 ", "/m1", "nope/none", "7/3"]) } : undefined;
    const cc = JSON.parse(JSON.stringify(c));
    cases.push({ fn: "plan", config: cc, target: tt, ...(opts ? { opts } : {}), out: wrap(() => m.planModelRemoval(structuredClone(cc), tt, opts)) });
  } else if (k === 2) {
    const pr = Array.from({ length: rnd(3) }, () => [pick(provIds), pick([...provIds, "renamed", ""])]);
    const mr = Array.from({ length: rnd(3) }, () => [`${pick(provIds)}/${pick(modelIds)}`, pick([{ modelId: "mm" }, { modelId: "" }, {}, null])]);
    cases.push({ fn: "rewrite", config: c, providerRenames: pr, modelRenames: mr, out: wrap(() => m.rewriteModelReferences(structuredClone(c), { providerRenames: new Map(pr), modelRenames: new Map(mr) })) });
  } else cases.push({ fn: "configured", config: c, out: wrap(() => [...m.configuredModelIds(structuredClone(c))].map(([k, v]) => [k, [...v]])) });
}
// Hand-written cases for paths the random configs rarely reach (only one model left; router/tokenSaver on or off; prototype keys).
for (const routerEnabled of [true, false, undefined]) for (const tsEnabled of [true, false, undefined]) for (const repl of ["", "p/m1"]) for (const withAgent of [true, false]) {
  const c = { ...(withAgent ? { agent: { model: "p/m1" } } : {}), model: { providers: { p: { models: { m1: {} } } } },
    router: { ...(routerEnabled === undefined ? {} : { enabled: routerEnabled }), scenarios: { default: "p/m1", think: { provider: "p", model: "m1" } },
      tokenSaver: { ...(tsEnabled === undefined ? {} : { enabled: tsEnabled }), judge: "p/m1", tiers: { fast: { model: "p/m1" } } }, fallback: { default: ["p/m1"] } } };
  const t = { providerId: "p", modelId: "m1" };
  cases.push({ fn: "plan", config: c, target: t, opts: { replacement: repl }, out: wrap(() => m.planModelRemoval(structuredClone(c), t, { replacement: repl })) });
}
for (const pid of ["__proto__", "constructor", "toString", "hasOwnProperty"]) {
  const c = JSON.parse(`{"model":{"providers":{"q":{"models":{"m":{}}}}},"agent":{"model":"q/m"}}`);
  cases.push({ fn: "plan", config: c, target: { providerId: pid }, out: wrap(() => m.planModelRemoval(structuredClone(c), { providerId: pid })) });
  cases.push({ fn: "find", config: c, target: { providerId: pid }, out: wrap(() => m.findModelReferences(structuredClone(c), { providerId: pid })) });
}
cases.push({ fn: "actions", out: [...m.REFERENCE_ACTIONS] });
const text = JSON.stringify(cases) + "\n";
const target = path.join(root, "rust/crates/ui-server-core/tests/fixtures/model_references.json");
fs.mkdirSync(path.dirname(target), { recursive: true });
if (process.argv.includes("--check")) { const same = fs.readFileSync(target, "utf8") === text; console.log(same ? "fixture stable" : "fixture DIFFERS"); process.exit(same ? 0 : 1); }
fs.writeFileSync(target, text);
const c = {}; for (const x of cases) { const key = x.fn + (x.out?.__error ? ":error" : x.out?.blocked ? ":blocked:" + x.out.blocked.code : x.fn === "plan" ? ":ok" : ""); c[key] = (c[key] ?? 0) + 1; }
console.log(cases.length, JSON.stringify(c));
