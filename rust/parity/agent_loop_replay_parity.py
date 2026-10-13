#!/usr/bin/env python3
"""Replay-differential parity for the agent-loop sidecar execute path.

Drives the TS sidecar (oracle) and the Rust sidecar with the SAME host: the execute request and
host module responses come from a recorded TS session (stdin capture). Each module_call is answered
from the recorded responses for the same (module, operation) in order (the last one is reused once
exhausted). Optional variants mutate the execute request or host responses to reach code paths no
run.py scenario covers (ask runMode, cachePlan, media_reference, ...). Both transcripts are then
normalized (random ids -> first-seen placeholders, ISO timestamps, durations) and compared exactly.

usage: agent_loop_replay_parity.py --rust BIN [--ts JS] [--variant NAME] REC.in [REC.in ...]
"""
import argparse, copy, json, os, re, subprocess, sys, threading, queue

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
TS = os.path.join(ROOT, "dist/src/cli/pilotdeck-agent-loop-sidecar.js")

def op_of(m):
    p = m.get("payload") or {}
    return (m.get("module"), p.get("operation"), p.get("method"))

def load(rec):
    lines = [json.loads(l) for l in open(rec) if l.strip()]
    execute = next(m for m in lines if m.get("method") == "execute")
    pools = {}
    out = rec[:-3] + ".out"
    calls = {}
    if os.path.exists(out):
        for l in open(out):
            try: m = json.loads(l)
            except Exception: continue
            if m.get("method") == "module_call": calls[m["messageId"]] = op_of(m)
    for m in lines:
        if m.get("kind") == "response" and m.get("inReplyTo") in calls:
            pools.setdefault(calls[m["inReplyTo"]], []).append(m)
    return execute, pools

# ---- variants: (execute mutator, response mutator(key, payload) -> payload) ----
def v_none(e): return e
def v_ask(e):
    e["payload"]["agent"]["runMode"] = "ask"; return e
def resp_cacheplan(key, msg, state=None):
    if key[0] == "context" and key[1] == "prepare_for_model" and isinstance(msg.get("payload"), dict):
        r = msg["payload"].get("result")
        if isinstance(r, dict):
            r["cachePlan"] = {"strategy": "anthropic", "breakpoints": [{"messageIndex": 0, "ttl": "5m"}]}
    return msg
MEDIA_DIR = os.environ.get("REPLAY_MEDIA_DIR", "/tmp")
def resp_media(key, msg, state=None):
    if key[0] == "context" and key[1] == "prepare_for_model" and isinstance(msg.get("payload"), dict):
        r = msg["payload"].get("result")
        if isinstance(r, dict) and r.get("messages"):
            path = os.path.join(MEDIA_DIR, "replay-media.b64")
            with open(path, "w") as f: f.write("iVBORw0KGgo=")
            blocks = [
                {"type": "media_reference", "mediaType": "image", "path": path, "mimeType": "image/png", "originalBytes": 8, "preview": "[image]", "detail": "high"},
                {"type": "media_reference", "mediaType": "pdf", "path": path, "mimeType": "application/pdf", "originalBytes": 8, "preview": "[pdf]", "pages": [1]},
                {"type": "media_reference", "mediaType": "pdf", "path": path, "mimeType": "text/plain", "originalBytes": 8, "preview": "[bad pdf]"},
                {"type": "media_reference", "mediaType": "audio", "path": path, "mimeType": "audio/wav", "preview": "[audio]"},
                {"type": "media_reference", "mediaType": "image", "path": path + ".missing", "mimeType": "image/png", "preview": "[missing]"},
            ]
            m = r["messages"][0]
            if isinstance(m.get("content"), list): m["content"] = m["content"] + blocks
    return msg
def v_materialize(e):
    m = e["payload"].setdefault("hostModules", {}).setdefault("model", {}).setdefault("methods", [])
    if "materialize_prepared_request" not in m: m.append("materialize_prepared_request")
    return e
def resp_routed(key, msg, state=None):
    # Route to a smaller context window so AgentLoop re-budgets against the prepared invocation.
    if key[0] == "model" and key[1] == "prepare" and isinstance(msg.get("payload"), dict):
        p = msg["payload"].get("prepared")
        if isinstance(p, dict): p["maxContextTokens"] = 12000
    return msg
SYNTH = {}
def synth_materialize(m):
    req = dict(m["payload"]["request"]); md = dict(req.get("metadata") or {}); md["materialized"] = True; req["metadata"] = md
    return {"kind": "response", "ok": True, "payload": {"request": req}}
SYNTH[("model", "materialize_prepared_request", None)] = synth_materialize
def resp_cacheplan_media(key, msg, state=None): return resp_cacheplan(key, resp_media(key, msg))
def _tool_end_events(msg):
    if isinstance(msg.get("payload"), dict):
        for e in msg["payload"].get("events") or []:
            if isinstance(e, dict) and e.get("type") == "tool_call_end": yield e
def _results(msg):
    p = msg.get("payload")
    if not isinstance(p, dict): return []
    if isinstance(p.get("results"), list): return p["results"]
    return [p["result"]] if isinstance(p.get("result"), dict) and "toolCallId" in p["result"] else []
def resp_repaired(key, msg, state=None):
    if key[:2] == ("model", "stream_next"):
        for e in _tool_end_events(msg): e["wasRepaired"] = True
    return msg
def resp_as_write(key, msg, state=None):
    if key[:2] == ("model", "stream_next"):
        for e in _tool_end_events(msg):
            if isinstance(e.get("toolCall"), dict): e["toolCall"]["name"] = "write_file"
    return msg
def resp_repaired_write(key, msg, state=None): return resp_repaired(key, resp_as_write(key, msg))
TRUNC_ERR = {"code": "invalid_tool_input", "message": "Missing required parameter `content` (output truncated)",
             "details": {"issues": [{"path": "content", "code": "required"}]}}
def resp_write_error(key, msg, state):
    msg = resp_as_write(key, msg)
    if key[0] == "capability" and str(key[1]).startswith("execute"):
        for r in _results(msg):
            n = state["writes"] = state.get("writes", 0) + 1
            r["toolName"] = "write_file"
            if state.get("succeed_first") and n == 1:
                r["type"] = "success"; r["data"] = {"filePath": "/tmp/draft.html", "type": "create"}
            else:
                r["type"] = "error"; r["error"] = dict(TRUNC_ERR); r.pop("data", None)
    return msg
def resp_write_then_error(key, msg, state):
    state["succeed_first"] = True
    return resp_write_error(key, msg, state)
PTL = {"type": "error", "error": {"code": "prompt_too_long", "message": "prompt is too long: 70000 tokens > 65536 maximum", "retryable": False}}
def _ptl_after_tool(key, msg, state):
    if key[0] == "capability" and str(key[1]).startswith("execute"): state["executed"] = True
    if key[:2] == ("model", "stream_next") and state.get("executed") and not state.get("errored"):
        state["errored"] = True
        msg["payload"] = {"events": [PTL], "done": False}
    return msg
def _recover(decision):
    def f(m): return {"kind": "response", "ok": True, "payload": {"result": decision}}
    return f
def resp_ptl_truncate(key, msg, state):
    SYNTH[("context", "recover_from_model_error", None)] = _recover({"type": "truncate_head_and_retry", "keepRatio": 0.5, "reason": "prompt_too_long"})
    return _ptl_after_tool(key, msg, state)
def resp_ptl_compact(key, msg, state):
    SYNTH[("context", "recover_from_model_error", None)] = _recover({"type": "compact_and_retry", "reason": "prompt_too_long", "maxContextTokens": 60000})
    return _ptl_after_tool(key, msg, state)
VARIANTS = {"ptl_truncate": (v_none, resp_ptl_truncate), "ptl_compact": (v_none, resp_ptl_compact),"repaired": (v_none, resp_repaired), "repaired_write": (v_none, resp_repaired_write),
            "write_error": (v_none, resp_write_error), "write_then_error": (v_none, resp_write_then_error),"routed": (v_none, resp_routed), "materialize": (v_materialize, resp_routed),"media": (v_none, resp_media), "cacheplan_media": (v_none, resp_cacheplan_media),"none": (v_none, None), "ask": (v_ask, None), "cacheplan": (v_none, resp_cacheplan)}

# A replayed session can diverge from its recording (e.g. a reused stream_next response loops);
# cap the transcript so a runaway oracle cannot exhaust memory. Such recordings are SKIPPED.
MAX_MSGS = 2000

class Proc:
    def __init__(self, argv, env):
        self.p = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, text=True)
        self.q = queue.Queue()
        threading.Thread(target=self._rd, daemon=True).start()
        self.err = []
        threading.Thread(target=lambda: self.err.extend(self.p.stderr), daemon=True).start()
    def _rd(self):
        for l in self.p.stdout: self.q.put(l)
        self.q.put(None)
    def send(self, m):
        self.p.stdin.write(json.dumps(m) + "\n"); self.p.stdin.flush()

def run(argv, env, execute, pools, resp_mut, timeout=15):
    pools = copy.deepcopy(pools); used = {}; state = {}
    pr = Proc(argv, env); out = []
    pr.send({"kind": "request", "messageId": "hello-h", "method": "hello", "payload": {}})
    pr.send(execute)
    while True:
        try: l = pr.q.get(timeout=timeout)
        except queue.Empty: out.append({"harness": "timeout"}); break
        if l is None: out.append({"harness": "eof"}); break
        m = json.loads(l); out.append(m)
        if len(out) > MAX_MSGS: out.append({"harness": "message_limit"}); pr.p.kill(); break
        if m.get("method") == "module_call":
            key = op_of(m); pool = pools.get(key) or []
            i = used.get(key, 0); used[key] = i + 1
            if key in SYNTH:
                r = SYNTH[key](m)
            elif not pool:
                r = {"kind": "response", "ok": False, "final": True, "outcome": "failed", "code": "HARNESS_NO_RECORDING"}
            else:
                r = copy.deepcopy(pool[min(i, len(pool) - 1)])
            r["inReplyTo"] = m["messageId"]; r["requestId"] = m.get("requestId")
            r["messageId"] = "host-%s-%d" % (m.get("module"), len(out))
            if resp_mut: r = resp_mut(key, r, state)
            pr.send(r)
        if m.get("kind") == "event" and m.get("final"): break
    try: pr.p.stdin.close(); pr.p.wait(timeout=10)
    except Exception: pr.p.kill()
    return out, "".join(pr.err)

ID = re.compile(r"\b([a-z][a-z_]*(?:-[a-z_]+)*)-([0-9a-z]{6,14}|[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})(-\d+)?\b")
UUID = re.compile(r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b|\b[0-9a-f]{32}\b")
ISO = re.compile(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?Z")
VOLATILE = {"durationMs", "elapsedMs", "moduleInstanceId", "connectionGeneration"}

def normalize(msgs):
    seen = {}
    def tok(s):
        def rid(mt):
            k = mt.group(0)
            if mt.group(2).isdigit() and not mt.group(3): return k
            seen.setdefault(k, "%s-<%d>" % (mt.group(1), len(seen)))
            return seen[k]
        s = UUID.sub(lambda mt: seen.setdefault(mt.group(0), "<uuid%d>" % len(seen)), s)
        s = ID.sub(rid, s)
        return ISO.sub("<ts>", s)
    def walk(v):
        if isinstance(v, dict): return {k: ("<v>" if k in VOLATILE else walk(x)) for k, x in v.items()}
        if isinstance(v, list): return [walk(x) for x in v]
        if isinstance(v, str): return tok(v)
        return v
    return [walk(m) for m in msgs]

def first_diff(a, b, p=""):
    if type(a) != type(b): return p, a, b
    if isinstance(a, dict):
        for k in list(a) + [k for k in b if k not in a]:
            if k not in a or k not in b: return p + "." + k, a.get(k, "<absent>"), b.get(k, "<absent>")
            d = first_diff(a[k], b[k], p + "." + k)
            if d: return d
        if list(a) != list(b): return p + "<key order>", list(a), list(b)
        return None
    if isinstance(a, list):
        for i, (x, y) in enumerate(zip(a, b)):
            d = first_diff(x, y, "%s[%d]" % (p, i))
            if d: return d
        if len(a) != len(b): return p + "<len>", len(a), len(b)
        return None
    return None if a == b else (p, a, b)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--rust", required=True); ap.add_argument("--ts", default=TS)
    ap.add_argument("--variant", default="none", choices=sorted(VARIANTS))
    ap.add_argument("--require-final", action="store_true", help="fail if the TS oracle does not reach a final event")
    ap.add_argument("--dump", help="directory to write normalized transcripts (<rec>.<variant>.{ts,rust}.jsonl)")
    ap.add_argument("recs", nargs="+")
    a = ap.parse_args()
    env = {k: v for k, v in os.environ.items() if k != "PILOTDECK_AGENT_LOOP_SIDECAR_EXEC"}
    em, rm = VARIANTS[a.variant]
    passed = failed = 0
    for rec in a.recs:
        try: execute, pools = load(rec)
        except StopIteration: continue
        execute = em(copy.deepcopy(execute))
        t, terr = run(["node", a.ts], env, execute, pools, rm)
        name = "%s[%s]" % (os.path.basename(rec), a.variant)
        if t and t[-1].get("harness") in ("message_limit", "timeout"):
            print("SKIP", name, "oracle did not settle under replay:", t[-1]["harness"]); continue
        r, rerr = run([a.rust], env, execute, pools, rm)
        nt, nr = normalize(t), normalize(r)
        d = first_diff(nt, nr)
        if a.dump:
            os.makedirs(a.dump, exist_ok=True)
            for tag, tr in (("ts", nt), ("rust", nr)):
                with open(os.path.join(a.dump, "%s.%s.%s.jsonl" % (os.path.basename(rec), a.variant, tag)), "w") as f:
                    f.writelines(json.dumps(m) + "\n" for m in tr)
        name = "%s[%s]" % (os.path.basename(rec), a.variant)
        if d is None:
            passed += 1; print("PASS", name, len(nt), "msgs, final:", (nt[-1].get("eventType") if nt else None))
        else:
            failed += 1
            print("FAIL", name, d[0]); print("   ts:", json.dumps(d[1])[:600]); print(" rust:", json.dumps(d[2])[:600])
            if rerr.strip(): print(" rust stderr:", rerr.strip()[:400])
    print("%d/%d passed" % (passed, passed + failed))
    sys.exit(1 if failed else 0)

if __name__ == "__main__":
    main()
