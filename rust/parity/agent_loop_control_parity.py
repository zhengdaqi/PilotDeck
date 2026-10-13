#!/usr/bin/env python3
"""Differential test: Rust vs TS agent-loop sidecar control plane (no execute admission)."""
import json, re, subprocess, sys, os
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
TS = ["node", os.path.join(ROOT, "dist/src/cli/pilotdeck-agent-loop-sidecar.js")]
RS = [os.path.join(ROOT, "rust/target/release/pilotdeck-agent-loop-sidecar")]
B = {"moduleInstanceId": "m", "connectionGeneration": "g"}
def req(method, **kw):
    d = {"kind": "request", "messageId": "msg-" + method, "method": method}; d.update(kw); return json.dumps(d)
CASES = {
 "hello": [req("hello", payload={})],
 "capabilities": [req("hello", payload={}), req("capabilities", payload={})],
 "handshake_no_payload": [req("hello")],
 "invalid_json": ["{nope", "[1,2"],
 "not_object": ["1", "[]", "null", '"x"'],
 "bad_kind": ['{"kind":"zzz"}'],
 "missing_message_id": ['{"kind":"request","method":"hello","payload":{}}'],
 "bad_method": [req("frobnicate")],
 "cancel_unknown": [req("cancel", runId="r", operationId="o", reason="x"), req("cancel", runId="r", operationId="o", reason="x", requestId="q")],
 "cancel_invalid": [req("cancel", runId="r"), req("cancel", runId="r", operationId="o", reason="x", requestId="")],
 "status_unknown": [req("status", requestId="q1"), req("status")],
 "resume": [req("resume", streamId="s", previousBinding=B, lastAppliedSequence=-1), req("resume", streamId="s", previousBinding=B, lastAppliedSequence=-2)],
 "ack": [req("ack", streamId="s", lastAppliedSequence=0), req("ack", streamId="s", lastAppliedSequence=1.5)],
 "module_call_rejected": [req("module_call", runId="r", operationId="o", requestId="q", module="model", payload={}), req("module_call", runId="r", operationId="o", requestId="q", module="nope", payload={})],
 "execute_invalid": [req("execute", runId="r"), req("execute", runId="r", operationId="o", requestId="q"), req("execute", runId="r", operationId="o", requestId="q", payload={}, idempotencyKey=""), req("execute", runId="r", operationId="o", requestId="q", payload={}, operationDeadline=5), req("execute", runId="r", operationId="o", requestId="q", payload={}, stepId="x")],
 "execute_expired": [req("execute", runId="r", operationId="o", requestId="q", payload={}, operationDeadline="2000-01-01T00:00:00.000Z"), req("execute", runId="r", operationId="o", requestId="q2", payload={}, attemptDeadline="2001-01-01T00:00:00Z", operationDeadline="2999-01-01T00:00:00Z")],
 "events_responses_errors": ['{"kind":"event","eventType":"x"}', '{"kind":"response","messageId":"a"}', '{"kind":"error","messageId":"a","code":"C","message":"m","retryability":"never"}', '{"kind":"response","messageId":"a","inReplyTo":"b","ok":true}'],
 "blank_lines": ["", "   ", req("hello", payload={})],
}
def norm(t):
    t = re.sub(r'"([a-z-]+)-[0-9a-z]{6,13}"', r'"\1-<id>"', t)
    t = re.sub(r'pilotdeck-agent-loop-instance-[0-9a-z]+(-[0-9a-z]+)*', 'INST', t)
    return t
def run(cmd, lines):
    p = subprocess.run(cmd, input="\n".join(lines) + "\n", capture_output=True, text=True, timeout=30, cwd="/tmp")
    return norm(p.stdout)
fails = 0
for name, lines in CASES.items():
    a, b = run(TS, lines), run(RS, lines)
    ok = a == b
    fails += not ok
    print(("PASS " if ok else "FAIL ") + name)
    if not ok:
        print("  TS:", a); print("  RS:", b)
print(f"{len(CASES)-fails}/{len(CASES)} passed")
sys.exit(1 if fails else 0)
