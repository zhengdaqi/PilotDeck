// JSON-lines Gateway host for the Rust gateway server (protocol: rust/crates/gateway-server/src/backend.rs).
// Usage: node bridge.mjs [variant]
import readline from "node:readline";
import { createFakeGateway } from "./fake_gateway.mjs";
import { createFakePublicApprovals, createFakePublicHost } from "./fake_public.mjs";
import { isHostCapabilityStreamBody, encodeHostCapabilityStream, PUBLIC_HOST_WIRE_HEADERS } from "../../../dist/src/composition/publicHostWire.js";
import { SkillManagerError, SkillValidationError } from "../../../dist/src/extension/skills/index.js";
import { DialogGatewayError } from "../../../dist/src/gateway/dialog/errors.js";

const gateway = createFakeGateway(process.argv[2] ?? "full");
const out = (v) => process.stdout.write(JSON.stringify(v) + "\n");
const variant = process.argv[2] ?? "full";
const approvals = variant === "full" ? createFakePublicApprovals(gateway.__log) : undefined;
const hostResolve = variant === "full" ? createFakePublicHost(gateway.__log) : undefined;
const providers = new Map(); let nextHandle = 0;
const b64 = (v) => Buffer.from(v).toString("base64");
const failure = (e) => ({ error: { status: e?.status, code: e?.code, message: e?.message } });
// Public transports (rust/crates/gateway-server/src/public.rs): the Rust server owns HTTP validation and framing;
// this host only invokes the bound bridge/provider and reports JS-value facts the server cannot observe.
const extra = {
  "__approvals.binding": async () => approvals.binding,
  "__approvals.status": async ({ input }) => { try { return { value: await approvals.status({ ...input, approverAuthorization: input.approverAuthorization }) }; } catch (e) { return failure(e); } },
  "__approvals.resume": async ({ input }) => { try { return { value: await approvals.resume({ ...input, approverAuthorization: input.approverAuthorization }) }; } catch (e) { return failure(e); } },
  "__host.resolve": async ({ principal }) => {
    try { const p = await hostResolve(principal); const h = ++nextHandle; providers.set(h, p); return { handle: h, operations: p.operations }; }
    catch (e) { return failure(e); }
  },
  "__host.release": async ({ handle }) => { providers.delete(handle); return {}; },
};
async function hostCall(id, { handle, operation, input, principal }) {
  const provider = providers.get(handle); providers.delete(handle);
  let response;
  try { response = await provider.call(operation, input, { principal, signal: new AbortController().signal }); }
  catch (e) { return out({ id, ok: true, result: failure(e) }); }
  const head = { status: response.status };
  try { const h = new Headers(response.headers); head.headers = PUBLIC_HOST_WIRE_HEADERS.filter((k) => h.has(k)).map((k) => [k, h.get(k)]); head.contentType = h.get("content-type"); }
  catch (e) { head.headersError = { message: e?.message }; }
  head.isStream = isHostCapabilityStreamBody(response.body);
  const ct = head.contentType;
  if (ct?.startsWith("application/x-ndjson") || ct?.startsWith("text/event-stream")) {
    head.mode = response.body ? "stream" : "missing_stream";
    out({ id, event: { head } });
    if (head.mode === "stream" && Number.isInteger(response.status) && response.status >= 100 && response.status <= 599 && !head.headersError) {
      try { for await (const chunk of encodeHostCapabilityStream(typeof response.body.getReader === "function" ? response.body : response.body, ct)) out({ id, event: { chunk: b64(chunk) } }); }
      catch (e) { return out({ id, ok: true, result: { streamError: e?.message ?? String(e) } }); }
    }
    return out({ id, ok: true, result: {} });
  }
  if (response.rawBody !== undefined) { head.mode = "bytes"; head.body = b64(String(response.rawBody)); }
  else if (response.body instanceof Uint8Array || (typeof response.body === "string" && ct && !ct.includes("json"))) { head.mode = "bytes"; head.body = b64(response.body); }
  else { head.mode = "json"; const j = JSON.stringify(response.body); if (j !== undefined) head.body = b64(j); }
  out({ id, event: { head } });
  out({ id, ok: true, result: {} });
}
out({ type: "ready", methods: [...Object.keys(gateway).filter((k) => typeof gateway[k] === "function"), ...(approvals ? ["__approvals", "__host"] : [])] });

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
    if (req.method === "__host.call") return await hostCall(req.id, req.params);
    if (req.method in extra) return out({ id: req.id, ok: true, result: await extra[req.method](...args) });
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
