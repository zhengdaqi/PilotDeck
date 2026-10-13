// Deterministic fake public approval bridge + public host provider resolver, bound by both the TS oracle
// (ts_server.mjs) and the Rust server's host (bridge.mjs). Behaviour is selected by input fields.
const fail = (status, code, message = code) => Object.assign(new Error(message), { status, code });
export function createFakePublicApprovals(log) {
  const binding = Object.freeze({ tenantId: "t1", agentId: "a1", pilotDeckUserId: "u1" });
  const act = async (op, input) => {
    log.push(["approvals." + op, input]);
    switch (input.sessionKey) {
      case "fail_status": throw fail(418, "TEAPOT");
      case "conflict": throw Object.assign(new Error("c"), { code: "SOP_RESUME_REQUEST_CONFLICT" });
      case "plain": throw new Error("plain");
      case "nullish": return op === "status" ? null : { ok: true };
      case "undef": return undefined;
      default: return { op, sessionKey: input.sessionKey ?? null, auth: input.approverAuthorization ?? null, keys: Object.keys(input) };
    }
  };
  return { binding, status: (input) => act("status", input), resume: (input) => act("resume", input) };
}
const enc = new TextEncoder();
export function createFakePublicHost(log) {
  return async (principal) => {
    log.push(["host.resolve", principal]);
    if (principal.agentId === "resolve_fail") throw fail(451, "RESOLVE_FAIL", "resolver refused");
    if (principal.agentId === "resolve_plain") throw new Error("resolver plain");
    return {
      operations: ["list_tools", "model_stream", "task_events", "file_parse", "not_wire_op", "task_status", "extract_sop_text"],
      async call(operation, input, ctx) {
        log.push(["host.call", operation, input, Object.keys(ctx)]);
        const mode = input.mode;
        if (mode === "throw") throw fail(503, "HOST_DOWN", "host down");
        if (mode === "badstatus") return { status: 99, body: {} };
        if (mode === "floatstatus") return { status: 200.5, body: {} };
        if (mode === "badheaders") return { status: 200, headers: [["bad header", "x"]], body: {} };
        if (mode === "raw") return { status: 201, headers: { "content-type": "text/plain", etag: "e1", "x-other": "dropped" }, body: { ignored: 1 }, rawBody: "raw-text" };
        if (mode === "bytes") return { status: 200, headers: { "content-type": "application/octet-stream" }, body: enc.encode("bytes\u00e9") };
        if (mode === "text") return { status: 202, headers: { "content-type": "text/html" }, body: "<b>hi</b>" };
        if (mode === "jsonstring") return { status: 200, headers: { "content-type": "application/json" }, body: "str" };
        if (mode === "undefbody") return { status: 204 };
        if (mode === "ndjson") return { status: 200, headers: { "content-type": "application/x-ndjson", "x-request-id": "r1" },
          body: (async function* () { yield { a: 1 }; yield "raw-line\n"; yield enc.encode("bytes-line\n"); yield { b: [2] }; })() };
        if (mode === "sse") return { status: 200, headers: { "content-type": "text/event-stream" }, body: (async function* () { yield "data: 1\n\n"; yield "data: 2\n\n"; })() };
        if (mode === "stream_bad_item") return { status: 200, headers: { "content-type": "application/x-ndjson" }, body: (async function* () { yield { ok: 1 }; yield 5; })() };
        if (mode === "stream_sse_obj") return { status: 200, headers: { "content-type": "text/event-stream" }, body: (async function* () { yield { a: 1 }; })() };
        if (mode === "stream_throw") return { status: 200, headers: { "content-type": "application/x-ndjson" }, body: (async function* () { yield { a: 1 }; throw new Error("mid"); })() };
        if (mode === "stream_no_ct") return { status: 200, body: (async function* () { yield "x"; })() };
        if (mode === "stream_json_ct") return { status: 200, headers: { "content-type": "application/json" }, body: (async function* () { yield "x"; })() };
        return { status: 200, body: { operation, input, principal } };
      },
    };
  };
}
