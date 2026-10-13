// Regenerates rust/crates/agent-loop-sidecar/src/ask_mode.json from the built TS sources (dist/).
// Usage: node rust/parity/gen_ask_mode.mjs [--check]
import { buildAskModeAgentToolSchema } from "../../dist/src/tool/builtin/agent.js";
import { ASK_MODE_ALLOWED_TOOLS, ASK_MODE_DESCRIPTION_SUFFIX } from "../../dist/src/tool/askModeConstraints.js";
import fs from "node:fs";
const path = new URL("../crates/agent-loop-sidecar/src/ask_mode.json", import.meta.url);
const text = JSON.stringify({ agent: buildAskModeAgentToolSchema(), allowed: [...ASK_MODE_ALLOWED_TOOLS], suffix: ASK_MODE_DESCRIPTION_SUFFIX }, null, 1) + "\n";
if (process.argv.includes("--check")) {
  if (fs.readFileSync(path, "utf8") !== text) { console.error("ask_mode.json is stale"); process.exit(1); }
  console.log("ask_mode.json up to date");
} else fs.writeFileSync(path, text);
