// Oracle fixture for the TUI pure core (dist build): reducer, render state,
// formatToolSummary, groupConsecutiveTools, sidebar helpers, truncate (ANSI-free domain).
// Usage: node rust/parity/tui/gen_tui_core_fixture.mjs [--check]
import fs from "node:fs";
import stringWidth from "string-width";
import isFullwidthCodePoint from "../../../node_modules/.pnpm/slice-ansi@9.0.0/node_modules/is-fullwidth-code-point/index.js";
import { applyGatewayEventToTuiState } from "../../../dist/src/adapters/channel/tui/app/types.js";
import { applyTuiEvent, createTuiRenderState } from "../../../dist/src/adapters/channel/tui/tui-render.js";
import { formatToolSummary } from "../../../dist/src/adapters/channel/tui/app/formatToolSummary.js";
import { groupConsecutiveTools } from "../../../dist/src/adapters/channel/tui/app/groupConsecutiveTools.js";
import * as sb from "../../../dist/src/adapters/channel/tui/app/sidebar-helpers.js";
import { truncateForDisplay, countVisualLines } from "../../../dist/src/adapters/channel/tui/app/truncate.js";
import { defaultTuiSessionKey } from "../../../dist/src/adapters/channel/tui/TuiChannel.js";

let seed = 23;
const rnd = (n) => { seed = (seed + 0x6d2b79f5) | 0; let t = Math.imul(seed ^ (seed >>> 15), 1 | seed); t ^= t + Math.imul(t ^ (t >>> 7), 61 | t); return (((t ^ (t >>> 14)) >>> 0) % n); };
const pick = (a) => a[rnd(a.length)];
const out = { widths: [], fullwidth: [], emojiPresentation: [], graphemes: [], reducer: [], render: [], summary: [], group: [], sidebar: [], truncate: [], relative: [], sessionKey: [] };

// Property tables as run-length ranges [start, value] over all code points (surrogates = -1).
const table = (f) => { const r = []; let prev = null; for (let cp = 0; cp <= 0x10ffff; cp++) { const v = cp >= 0xd800 && cp <= 0xdfff ? -1 : f(cp); if (v !== prev) { r.push([cp, v]); prev = v; } } return r; };
out.widths = table((cp) => stringWidth(String.fromCodePoint(cp)));
out.fullwidth = table((cp) => isFullwidthCodePoint(cp) ? 1 : 0);
const epRe = /\p{Emoji_Presentation}/v;
out.emojiPresentation = table((cp) => epRe.test(String.fromCodePoint(cp)) ? 1 : 0);
const texts = ["", "hi", " ", "a\nb", "你好", "x".repeat(5)];
const ids = ["t1", "t2", "t3"];
const ev = () => {
  switch (rnd(11)) {
    case 0: return { type: "turn_started" };
    case 1: return { type: "assistant_text_delta", text: pick(texts) };
    case 2: return { type: "assistant_thinking_delta", text: pick(texts) };
    case 3: return { type: "tool_call_started", toolCallId: pick(ids), name: pick(["bash", "read"]) };
    case 4: { const e = { type: "tool_call_finished", toolCallId: pick(ids), toolName: pick(["bash", "read", undefined]), ok: !!rnd(2) };
      if (rnd(2)) e.resultPreview = pick(["  out  ", "", "x\ny"]); if (rnd(2)) e.resultLineCount = rnd(9); if (rnd(2)) e.resultPath = "/tmp/r" + rnd(3); return e; }
    case 5: return { type: "permission_request", requestId: "p" + rnd(3), toolName: "bash", payload: { cmd: "ls", n: rnd(3) } };
    case 6: return { type: "structured_output", payload: pick([{ a: 1, b: [1, "x"] }, null, "s", []]) };
    case 7: return { type: "plan_mode_changed", mode: pick(["plan", "default", "ask"]) };
    case 8: return { type: "turn_completed" };
    case 9: return { type: "error", message: pick(["boom", ""]) };
    default: { const e = { type: "tool_result_detail_available", toolCallId: pick(ids) }; if (rnd(2)) e.resultPath = "/p" + rnd(2); if (rnd(2)) e.fullText = "full" + rnd(2); return e; }
  }
};
for (let k = 0; k < 300; k++) {
  const events = Array.from({ length: 1 + rnd(12) }, ev);
  let st = { messages: [], activity: [], mode: "default", isRunning: false, pendingPermissions: [] };
  const steps = [];
  for (const e of events) { st = applyGatewayEventToTuiState(st, e); steps.push(JSON.parse(JSON.stringify(st))); }
  out.reducer.push({ events, steps });
  const rs = createTuiRenderState();
  for (const e of events) applyTuiEvent(rs, e);
  out.render.push({ events, state: JSON.parse(JSON.stringify(rs)) });
}
const previews = ["", "error TS2304: x\nerror again", "12 passing\n3 failing", "5 passed", "ok", "SyntaxError: bad", "ERROR error", "line1\nline2", "7 OK 2 Failed", "Error", "TypeError x"];
for (let k = 0; k < 300; k++) {
  const a = [pick(["bash", "read", "edit"]), pick([undefined, "", "ls -la", "x.ts"]), rnd(50), rnd(1000), !!rnd(3), pick(previews) + (rnd(4) ? "" : "y".repeat(rnd(120)))];
  out.summary.push({ args: a, result: formatToolSummary(...a) });
}
const tmsg = () => { const r = rnd(5); if (r < 3) { const m = { role: "tool", text: "t" }; const n = pick(["bash", "read", undefined]); if (n) m.toolName = n; if (rnd(4) === 0) m.expanded = !!rnd(2); return m; } return pick([{ role: "user", text: "u" }, { role: "assistant", text: "a" }]); };
for (let k = 0; k < 200; k++) { const msgs = Array.from({ length: rnd(12) }, tmsg); out.group.push({ messages: msgs, result: groupConsecutiveTools(msgs) }); }
const sess = () => { const s = { sessionId: "s" + rnd(6) }; if (rnd(3)) s.sessionKey = "k" + rnd(6); if (rnd(4)) s.cwd = pick(["/a/proj", "/b/proj", "/c/other/", "x"]); if (rnd(3)) s.lastModified = rnd(1000);
  if (rnd(3)) s.tag = pick(["running", "waiting", "idle", "done", "error", "background", "weird"]);
  for (const f of ["customTitle", "aiTitle", "firstPrompt", "summary"]) if (rnd(4) === 0) s[f] = pick(["", "Short", "A much longer title that goes on"]); return s; };
for (let k = 0; k < 200; k++) {
  const sessions = Array.from({ length: rnd(7) }, sess); const by = pick(["project", "status"]); const active = pick(["s1", "k2", "s3", "zz"]);
  const groups = sb.groupSessions(sessions, by); const smart = sb.computeSmartCollapse(groups, active);
  const rows = sb.flattenSidebarRows(groups, smart);
  out.sidebar.push({ sessions, by, active, groups, collapsed: [...smart], rows, icons: sessions.map(sb.sessionStatusIcon), titles: sessions.map((s) => sb.sessionDisplayTitle(s, 3 + rnd(20))).map(String) });
}
// Fix titles: recompute deterministically with recorded maxLen.
out.sidebar.forEach((c) => { c.maxLens = c.sessions.map((_, i) => 3 + (i * 7) % 20); c.titles = c.sessions.map((s, i) => sb.sessionDisplayTitle(s, c.maxLens[i])); });
const pieces = ["a", "word ", "你好", "é", "e\u0301", "\t", "\n", "x".repeat(30), "ｶﾞ", "Ω", " ", "\u200b", "한"];
for (let k = 0; k < 400; k++) {
  const s = Array.from({ length: rnd(40) }, () => pick(pieces)).join(""); const w = pick([0, 5, 15, 20, 30, 80, 120]);
  out.truncate.push({ input: s, width: w, result: truncateForDisplay(s, w), count: countVisualLines(s, w) });
}
const seg = new Intl.Segmenter(undefined, { granularity: "grapheme" });
for (let k = 0; k < 300; k++) { const s = Array.from({ length: rnd(20) }, () => pick(pieces)).join(""); out.graphemes.push({ input: s, segments: [...seg.segment(s)].map((x) => x.segment) }); }
const realNow = Date.now; const now = 1_700_000_000_000; Date.now = () => now;
for (const d of [-5, 0, 999, 1000, 59_999, 60_000, 3_599_999, 3_600_000, 86_399_999, 86_400_000, 9e9]) out.relative.push({ now, ts: now - d, result: sb.relativeTime(now - d) });
Date.now = realNow;
for (const p of ["/x/y", "", "proj"]) out.sessionKey.push({ projectKey: p, result: defaultTuiSessionKey(p) });

const path = new URL("../../crates/tui-core/tests/fixtures/tui_core.json", import.meta.url);
const tpath = new URL("../../crates/tui-core/src/tables.rs", import.meta.url);
const rs = (name, t) => `pub static ${name}: &[(u32, i8)] = &[\n${t.map(([a, v]) => `    (0x${a.toString(16)}, ${v}),`).join("\n")}\n];\n`;
const ttext = "// Generated by rust/parity/tui/gen_tui_core_fixture.mjs from Node " + process.versions.node + " (Unicode " + process.versions.unicode + "). Do not edit.\n"
  + rs("STRING_WIDTH", out.widths) + rs("FULLWIDTH", out.fullwidth) + rs("EMOJI_PRESENTATION", out.emojiPresentation);
const text = JSON.stringify(out) + "\n";
if (process.argv.includes("--check")) {
  if (fs.readFileSync(path, "utf8") !== text || fs.readFileSync(tpath, "utf8") !== ttext) { console.error("fixture drift"); process.exit(1); }
  console.log("ok");
} else {
  fs.mkdirSync(new URL(".", path), { recursive: true }); fs.mkdirSync(new URL(".", tpath), { recursive: true });
  fs.writeFileSync(path, text); fs.writeFileSync(tpath, ttext);
  console.log("wrote", out.widths.length, out.fullwidth.length, out.emojiPresentation.length);
}
