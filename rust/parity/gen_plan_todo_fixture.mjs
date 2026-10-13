// Fixture of parsePlanTodoSnapshot (dist build) on random valid/invalid host snapshots.
// Usage: node rust/parity/gen_plan_todo_fixture.mjs [--check]
import fs from "node:fs";
import { parsePlanTodoSnapshot } from "../../dist/src/plan-todo/projection/PlanTodoProjection.js";
let seed = 11;
const rnd = (n) => { seed = (seed + 0x6d2b79f5) | 0; let t = Math.imul(seed ^ (seed >>> 15), 1 | seed); t ^= t + Math.imul(t ^ (t >>> 7), 61 | t); return (((t ^ (t >>> 14)) >>> 0) % n); };
const pick = (a) => a[rnd(a.length)];
const statuses = ["pending", "in_progress", "completed", "cancelled", "bogus", undefined];
const ids = ["a", "b", "c", "d", undefined, null, "", 7];
const contents = ["write code", "test", " spaced ", undefined, "", "write code"];
const todo = () => {
  if (rnd(25) === 0) return pick([null, "str", 5, ["x"]]);
  const t = {};
  const id = pick(ids); if (id !== undefined || rnd(2)) t.id = id;
  const c = pick(contents); if (c !== undefined) t.content = c;
  const s = pick(statuses); if (s !== undefined) t.status = s;
  if (rnd(3) === 0) t.priority = pick(["high", "low", " "]);
  return t;
};
const todos = () => Array.from({ length: rnd(5) }, todo);
const diag = () => rnd(15) === 0 ? pick([null, [], "x"]) : {
  writeCount: rnd(4), todoCount: rnd(4), activeCount: 0, completedCount: 0, cancelledCount: 0,
  largeRewriteCount: rnd(3), deletedOpenItemCount: rnd(3), completedWithoutActiveCount: rnd(2),
  ...(rnd(2) ? { lastWrite: { mode: "structured", merge: false, addedCount: 1 } } : {}),
};
const hist = () => {
  if (rnd(30) === 0) return pick([null, 3, { todos: "x", mode: "markdown" }, { todos: [], mode: "x" }]);
  const h = { createdAt: "2026-01-0" + (1 + rnd(9)) + "T00:00:00.000Z", mode: pick(["markdown", "structured"]), todos: todos() };
  if (rnd(2)) h.merge = !!rnd(2);
  if (rnd(3) === 0) h.reason = pick([" why ", "", "because", 5]);
  if (rnd(3) === 0) h.markdown = pick(["- [ ] a", null, ""]);
  return h;
};
const snap = () => {
  if (rnd(40) === 0) return pick([null, undefined, [], "s", 1]);
  const s = {
    requiresInitialization: rnd(20) === 0 ? "no" : !!rnd(2),
    toolCallsSinceLastTodoWrite: rnd(20) === 0 ? pick([-1, 1.5, "2", 2 ** 60]) : rnd(5),
    todos: rnd(30) === 0 ? {} : todos(), activeTodos: rnd(30) === 0 ? null : todos(),
    todoHistory: rnd(30) === 0 ? "h" : Array.from({ length: rnd(4) }, hist),
    todoDiagnostics: diag(),
  };
  if (rnd(3) === 0) s.approvedPlan = pick(["  The plan  ", "", "   ", "plan", 42]);
  if (rnd(3) === 0) s.lastMarkdown = pick(["md", null, ""]);
  return s;
};
const cases = [];
for (let i = 0; i < 3000; i++) {
  const input = snap();
  let out;
  try { out = { ok: parsePlanTodoSnapshot(input) }; } catch (e) { out = { err: String(e.message) }; }
  cases.push({ input: input === undefined ? "__undefined__" : input, ...out });
}
const path = new URL("../crates/agent-loop-sidecar/tests/fixtures/plan_todo.json", import.meta.url);
const text = JSON.stringify(cases);
if (process.argv.includes("--check")) {
  if (fs.readFileSync(path, "utf8") !== text) { console.error("plan_todo fixture is stale"); process.exit(1); }
  console.log("plan_todo fixture up to date");
} else fs.writeFileSync(path, text);
