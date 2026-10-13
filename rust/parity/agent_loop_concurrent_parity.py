#!/usr/bin/env python3
"""Concurrent-execute differential: two executes for different operations on ONE sidecar process.

Operation A's module calls are held until operation B has issued a module call and reached its
final event, so A can only finish if the sidecar interleaves executions (TS `handleExecute`
promises run concurrently). Each operation's transcript (its accepted response, module calls,
events) is projected out of the shared stream, normalized like agent_loop_replay_parity.py, and
compared TS vs Rust exactly; the global interleaving order is timing-dependent and not compared.
Usage: agent_loop_concurrent_parity.py --rust BIN RECA.in RECB.in [RECA2.in RECB2.in ...]
"""
import argparse, copy, json, os, queue, sys
sys.path.insert(0, os.path.dirname(__file__))
from agent_loop_replay_parity import TS, Proc, load, op_of, normalize, first_diff

def run(argv, env, execs, pools, timeout=20):
    pr = Proc(argv, env)
    pr.send({"kind": "request", "messageId": "hello-h", "method": "hello", "payload": {}})
    tag_of_msg = {}
    for tag, e in execs.items():
        tag_of_msg[e["messageId"]] = tag
        pr.send(e)
    per = {t: [] for t in execs}; used = {}; held = []; finals = set(); b_called = False; overlap = False
    def answer(tag, m):
        key = op_of(m); pool = pools[tag].get(key) or []
        i = used.get((tag, key), 0); used[(tag, key)] = i + 1
        r = copy.deepcopy(pool[min(i, len(pool) - 1)]) if pool else {"kind": "response", "ok": False, "final": True, "outcome": "failed", "code": "HARNESS_NO_RECORDING"}
        r["inReplyTo"] = m["messageId"]; r["requestId"] = m.get("requestId"); r["messageId"] = "host-%s-%s-%d" % (tag, m.get("module"), i)
        pr.send(r)
    while len(finals) < len(execs):
        try: l = pr.q.get(timeout=timeout)
        except queue.Empty: return per, "timeout", overlap
        if l is None: return per, "eof", overlap
        m = json.loads(l)
        tag = tag_of_msg.get(m.get("inReplyTo")) or next((t for t, e in execs.items() if m.get("operationId") == e["operationId"]), None)
        if tag is None: continue
        per[tag].append(m)
        if sum(len(v) for v in per.values()) > 4000: pr.p.kill(); return per, "message_limit", overlap
        if m.get("method") == "module_call":
            if tag == "A" and "B" not in finals: held.append(m)
            else:
                if tag == "B": b_called = True
                answer(tag, m)
        if m.get("kind") == "event" and m.get("final"):
            finals.add(tag)
            if tag == "B":
                overlap = b_called and "A" not in finals
                for h in held: answer("A", h)
                held = []
    try: pr.p.stdin.close(); pr.p.wait(timeout=10)
    except Exception: pr.p.kill()
    return per, "ok", overlap

def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--rust", required=True); ap.add_argument("--ts", default=TS); ap.add_argument("recs", nargs="+")
    a = ap.parse_args()
    env = {k: v for k, v in os.environ.items() if k != "PILOTDECK_AGENT_LOOP_SIDECAR_EXEC"}
    passed = failed = 0
    for ra, rb in zip(a.recs[::2], a.recs[1::2]):
        (ea, pa), (eb, pb) = load(ra), load(rb)
        ea, eb = copy.deepcopy(ea), copy.deepcopy(eb)
        for tag, e in (("A", ea), ("B", eb)):
            e["operationId"] = "op-conc%s" % tag.lower(); e["requestId"] = "req-conc%s" % tag.lower(); e["messageId"] = "exec-conc%s" % tag.lower()
        execs, pools = {"A": ea, "B": eb}, {"A": pa, "B": pb}
        name = "%s+%s" % (os.path.basename(ra), os.path.basename(rb))
        t, ts_status, ts_overlap = run(["node", a.ts], env, execs, pools)
        if ts_status != "ok":
            print("SKIP", name, "TS oracle did not settle:", ts_status); continue
        r, rs_status, rs_overlap = run([a.rust], env, execs, pools)
        diff = None if rs_status == "ok" else ("status", ts_status, rs_status)
        if not diff and ts_overlap != rs_overlap: diff = ("overlap", ts_overlap, rs_overlap)
        for tag in ("A", "B"):
            if diff: break
            d = first_diff(normalize(t[tag]), normalize(r[tag]))
            if d: diff = (tag,) + d
        if diff: failed += 1; print("FAIL", name, diff)
        else: passed += 1; print("PASS", name, "(overlap=%s)" % rs_overlap)
    print("%d passed, %d failed" % (passed, failed))
    sys.exit(1 if failed else 0)

if __name__ == "__main__": main()
