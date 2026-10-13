// Deterministic fake Gateway used by the gateway-server differential (TS server vs Rust server).
// Every call is logged; `listSessions()` (no mode) returns the log so gateway-side effects
// (close cleanup, binding injection, argument shapes) are observable over the protocol.
import { SkillManagerError, SkillValidationError } from "../../../dist/src/extension/skills/index.js";
import { DialogGatewayError } from "../../../dist/src/gateway/dialog/errors.js";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export function createFakeGateway(variant = "full") {
  const log = [];
  const aborted = new Set();
  const behave = (method, args) => {
    log.push(args.length ? [method, args[0] === undefined ? "<undefined>" : args[0]] : [method]);
    const mode = args[0] && typeof args[0] === "object" ? args[0].mode : undefined;
    switch (mode) {
      case "structured": throw Object.assign(new Error("boom"), { code: "E_STRUCT", details: { a: 1 }, status: 503 });
      case "dialog": throw new DialogGatewayError("dialog_bad", "Dialog failed", { x: 1 });
      case "dialog_nodetails": throw new DialogGatewayError("dialog_bad", "Dialog failed");
      case "plain": throw new Error("plain failure");
      case "value": throw "raw string";
      case "object": throw { code: "X" };
      case "numcode": throw Object.assign(new Error("num code"), { code: 42 });
      case "skill_manager": throw new SkillManagerError("conflict", "Skill exists.");
      case "skill_validation": throw new SkillValidationError({ ok: false, issues: [{ rule: "name", severity: "error" }] });
      case "status_404": throw Object.assign(new Error("sop missing"), { code: "SOP_NOT_FOUND", status: 404, details: { d: 1 } });
      case "status_409": throw Object.assign(new Error("sop conflict"), { code: "SOP_CONFLICT", status: 409 });
      case "status_700": throw Object.assign(new Error("bad status"), { code: "SOP_X", status: 700 });
      case "status_arr": throw Object.assign(new Error("arr details"), { code: "SOP_Y", status: 500, details: [1, 2] });
      case "undefined_result": return undefined;
      default: return { method, echo: args[0] ?? null };
    }
  };
  const m = (name) => async (...args) => behave(name, args);
  const required = {
    describeServer: async () => { log.push(["describeServer"]); return { name: "fake-gateway", version: "1", variant }; },
    listSessions: async (input) => {
      if (input && input.mode) return behave("listSessions", [input]);
      log.push(["listSessions", input === undefined ? "<undefined>" : input]);
      return { calls: log.slice() };
    },
    abortTurn: async (input) => { log.push(["abortTurn", input]); if (input && input.sessionKey) aborted.add(input.sessionKey); },
    async *submitTurn(input) {
      log.push(["submitTurn", input]);
      const mode = input?.mode;
      const base = { sessionKey: input?.sessionKey };
      if (mode === "throw_now") throw Object.assign(new Error("submit refused"), { code: "SUBMIT_REFUSED" });
      yield { type: "turn_started", ...base };
      if (mode === "permission") yield { type: "permission_request", requestId: "perm-1", ...base };
      if (mode === "throw_mid") throw new Error("stream broke");
      if (mode === "hang") {
        for (let i = 0; i < 100 && !aborted.has(input.sessionKey); i++) await sleep(30);
        yield { type: "turn_aborted", ...base };
        return;
      }
      yield { type: "text_delta", text: "hello", ...base };
      if (mode === "noterminal") return;
      if (mode === "error_event") yield { type: "error", code: "model_failed", message: "x", recoverable: false, ...base };
      yield { type: "turn_completed", ...base, result: { ok: true } };
    },
  };
  if (variant === "minimal") return required;
  const full = { ...required };
  for (const name of [
    "steerTurn", "cancelSteer", "resumeSession", "newSession", "closeSession", "cronCreate", "cronList", "cronUpdate", "cronDelete",
    "cronStop", "cronRunNow", "respondElicitation", "respondUserDialog", "permissionDecide", "grantSessionPermission",
    "readSessionMessages", "readSubagentMessages", "forkSession", "replaceLastTurn", "finalizeLastTurnReplacement",
    "listProjects", "describeProject", "uploadCreate", "uploadGet", "runGet", "listPermissions", "memoryList", "snapshotList",
    "sopStatus", "resumeSop", "closeProjectSessions", "deleteSession", "renameSession", "recordAgentStatusMessage",
    "projectFilesList", "commandsList", "getActiveTurnSnapshot", "mcpServerStatus", "reconnectMcpServer", "resolveSettings",
    "supportedAgents", "reconnectInteraction", "reloadConfig", "prepareWeixinLogin", "reloadExtensions", "skillsList",
    "skillRead", "skillValidate", "alwaysOnApply", "alwaysOnAbort", "alwaysOnRerunPlan", "disconnectInteraction",
  ]) full[name] = m(name);
  full.disconnectInteraction = async (input) => { log.push(["disconnectInteraction", input]); return { preserveTurn: input.sessionKey === "keep" }; };
  full.describeProject = async (input) => {
    log.push(["describeProject", input]);
    if (input.projectKey === "missing") throw new Error("Project missing not found.");
    return { projectKey: input.projectKey, name: "Project " + input.projectKey };
  };
  full.listProjects = async (...args) => { log.push(args.length ? ["listProjects", args[0]] : ["listProjects"]); return { projects: [{ projectKey: "p1" }] }; };
  return full;
}
