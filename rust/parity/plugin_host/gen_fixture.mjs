// Oracle for patchProjectScopedMcpSpec (dist). Records host facts so Rust can replay with the same inputs.
// Usage: node rust/parity/plugin_host/gen_fixture.mjs
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const mod = await import(path.join(root, "dist/src/mcp/runtime/projectMcpSpec.js"));
const moduleDir = path.join(root, "dist/src/mcp/runtime");
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "pd-plughost-"));
const otherDir = path.join(tmp, "other"); fs.mkdirSync(otherDir);
const fakeRt = path.join(tmp, "rt"); fs.mkdirSync(path.join(fakeRt, "node_modules/@playwright/mcp"), { recursive: true });
fs.writeFileSync(path.join(fakeRt, "node_modules/@playwright/mcp/cli.js"), "");
const linkToRoot = path.join(tmp, "link-root"); fs.symlinkSync(root, linkToRoot);
const funasr = JSON.parse(fs.readFileSync(path.join(root, "src/extension/plugins/builtin/funasr/plugin.json"), "utf8")).mcpServers.funasr;
const browser = JSON.parse(fs.readFileSync(path.join(root, "src/extension/plugins/builtin/browser-use/plugin.json"), "utf8")).mcpServers["browser-use"];
const specs = [
  { id: "funasr", transport: "stdio", ...funasr },
  { id: "browser-use", transport: "stdio", ...browser },
  { id: "funasr", transport: "stdio", command: "__PILOTDECK_NODE_EXECUTABLE__ __PILOTDECK_NODE_EXECUTABLE__", env: { A: "1" } },
  { id: "funasr", transport: "stdio", command: "x", args: ["__PILOTDECK_PROJECT_ROOT__/__PILOTDECK_PROJECT_ROOT__", "__PILOTDECK_PLAYWRIGHT_MCP_ENTRYPOINT__", 7, null, true] },
  { id: "browser-use", transport: "stdio", command: "__PILOTDECK_PROJECT_ROOT__", args: ["__PILOTDECK_PLAYWRIGHT_MCP_ENTRYPOINT__", "__PILOTDECK_FUNASR_RUNTIME_ROOT__"] },
  { id: "other", transport: "stdio", command: "__PILOTDECK_NODE_EXECUTABLE__", args: [] },
  { id: "funasr", transport: "http", url: "http://x" },
  { id: "browser-use", transport: "stdio", command: "a", args: null },
];
const roots = ["/p/proj", "rel/proj", "/p/a/../b/./c/", "."];
const homes = ["/h/.pilotdeck", "relhome", "/h/x/../y"];
const runtimeRoots = [undefined, "", "/nonexistent/rt", root, linkToRoot, otherDir, fakeRt];
const cases = [];
for (const rt of runtimeRoots) {
  if (rt === undefined) delete process.env.PILOTDECK_RUNTIME_ROOT; else process.env.PILOTDECK_RUNTIME_ROOT = rt;
  specs.forEach((spec, i) => roots.forEach((pr, j) => {
    const ph = homes[(i + j) % homes.length];
    cases.push({ runtimeRoot: rt ?? null, spec, projectRoot: pr, pilotHome: ph, out: (() => { try { return mod.patchProjectScopedMcpSpec(structuredClone(spec), pr, ph); } catch (e) { return { __error: e.constructor.name }; } })() });
  }));
  cases.push({ runtimeRoot: rt ?? null, installCommand: mod.getPilotDeckInstallCommand() });
}
const out = { nodeExecutable: process.execPath, moduleDir, cwd: process.cwd(), tmp, cases: JSON.parse(JSON.stringify(cases)) };
const target = path.join(root, "rust/crates/plugin-host/tests/fixtures/project_mcp_spec.json");
fs.writeFileSync(target, JSON.stringify(out, null, 1) + "\n");
console.log(`wrote ${cases.length} cases (fixture is host-bound: tmp dirs kept at ${tmp})`);
