// JSON-lines Gateway host for the Rust gateway server (protocol: rust/crates/gateway-server/src/backend.rs).
// Usage: node bridge.mjs [variant]
import readline from "node:readline";
import { createFakeGateway } from "./fake_gateway.mjs";
import { SkillManagerError, SkillValidationError } from "../../../dist/src/extension/skills/index.js";
import { DialogGatewayError } from "../../../dist/src/gateway/dialog/errors.js";

const gateway = createFakeGateway(process.argv[2] ?? "full");
const out = (v) => process.stdout.write(JSON.stringify(v) + "\n");
out({ type: "ready", methods: Object.keys(gateway).filter((k) => typeof gateway[k] === "function") });

function wireError(error) {
  if (error instanceof SkillValidationError) return { kind: "skill_validation", code: error.code, message: error.message, validation: error.validation };
  if (error instanceof SkillManagerError) return { kind: "skill_manager", code: error.code, message: error.message };
  if (error instanceof DialogGatewayError || (error instanceof Error && typeof error.code === "string")) {
    return { kind: "structured", code: error.code, message: error.message,
      ...("details" in error && error.details !== undefined ? { details: error.details } : {}),
      ...("status" in error && error.status !== undefined ? { status: error.status } : {}) };
  }
  return { kind: "plain", message: error instanceof Error ? error.message : String(error) };
}

readline.createInterface({ input: process.stdin }).on("line", async (line) => {
  const req = JSON.parse(line);
  const args = req.argc === 1 ? [req.params] : [];
  try {
    if (req.method === "submitTurn") {
      for await (const event of gateway.submitTurn(...args)) out({ id: req.id, event });
      out({ id: req.id, ok: true });
    } else {
      out({ id: req.id, ok: true, result: await gateway[req.method](...args) });
    }
  } catch (error) {
    out({ id: req.id, ok: false, error: wireError(error) });
  }
});
