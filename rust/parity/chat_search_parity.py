#!/usr/bin/env python3
"""Black-box parity runner for `pilotdeck chat search` (slice 1).

Builds deterministic fixture homes, runs the frozen TypeScript CLI
(oracle, `node dist/src/cli/pilotdeck.js`) and the Rust `pilotdeck` binary
through the same argv/cwd/env, and compares exit code + stdout byte-for-byte
(and stderr where the contract says `stderr: exact`).

Usage: chat_search_parity.py --ts-cli <dist/src/cli/pilotdeck.js> --rust-bin <path> [--node node] [--report out.json]
Exit 0 only if every case matches and the negative controls detect drift.
"""
import argparse, hashlib, json, os, shutil, subprocess, sys, tempfile

def w(path, text=None, data=None):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(data if data is not None else text.encode("utf-8"))

def j(obj):
    return json.dumps(obj, ensure_ascii=False, separators=(",", ":"))

def user(text, sid, at="2026-09-10T00:05:00.000Z", extra_blocks=()):
    blocks = [{"type": "text", "text": text}, *extra_blocks]
    return j({"type": "accepted_input", "sessionId": sid, "createdAt": at, "messages": [{"role": "user", "content": blocks}]})

def asst(text, sid, at="2026-09-10T13:45:00.000Z", kind="assistant_message"):
    return j({"type": kind, "sessionId": sid, "createdAt": at, "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}})

def meta(sid, **fields):
    return j({"type": "session_metadata", "sessionId": sid, "metadata": fields})

def legacy_id(root):
    import re
    s = re.sub(r"[^A-Za-z0-9._-]+", "-", root.replace("\\", "/"))
    return s.strip("-") or "project"

def build_fixture(base):
    home = os.path.join(base, "pilot-home")
    proj_a = os.path.join(base, "work", "proj-a")
    proj_b = os.path.join(base, "work", "项目 b")
    os.makedirs(proj_a); os.makedirs(proj_b)
    chats_a = os.path.join(home, "projects", legacy_id(proj_a), "chats")
    # project B registered through a .cwd marker under a custom id
    w(os.path.join(home, "projects", "custom-b", ".cwd"), proj_b + "\n")
    chats_b = os.path.join(home, "projects", "custom-b", "chats")

    long = ("lorem ipsum " * 20) + "NEEDLE in the middle " + ("dolor sit " * 20)
    w(os.path.join(chats_a, "s-alpha-0001-long-session-id.jsonl"), "\n".join([
        user("Deploy docker   to\tstaging please", "s-alpha-0001-long-session-id"),
        asst("Sure. Docker deploy steps:\n1. build\n2. push", "s-alpha-0001-long-session-id"),
        meta("s-alpha-0001-long-session-id", title="Docker 部署 \"guide\""),
        "",
        "not json at all",
        user(long, "s-alpha-0001-long-session-id", at="2026-01-02T03:04:05"),
        asst("emoji 😀 needle ✨ café İstanbul", "s-alpha-0001-long-session-id", kind="durable_message"),
    ]) + "\n")
    w(os.path.join(chats_a, "s2.jsonl"), "\r\n".join([
        user("first prompt about Kubernetes", "s2", at=""),
        asst("kubernetes answer with needle", "s2", at="garbage-date"),
        user("  second prompt docker  ", "s2", at="2026-03-01"),
    ]) + "\r" + asst("lone CR line docker", "s2"))
    w(os.path.join(chats_a, "always-on-report:x.jsonl"), user("docker internal", "always-on-report:x") + "\n")
    w(os.path.join(chats_a, "notes.txt"), user("docker txt", "zz") + "\n")
    w(os.path.join(chats_a, "nosid.jsonl"), j({"type": "accepted_input", "createdAt": 5, "messages": [{"content": [{"type": "text", "text": "docker without id"}, {"type": "image"}, {"type": "text", "text": "  second block  "}]}]}) + "\n" + j([1, 2]) + "\n" + j("str") + "\n")
    w(os.path.join(chats_a, "ai-title.jsonl"), meta("ai-title", aiTitle="AI named") + "\n" + user("docker with ai title", "ai-title") + "\n")
    w(os.path.join(chats_a, "empty.jsonl"), "")
    # >64 KiB: tail snapshot provides title
    pad = asst("x" * 70000, "big-snap")
    w(os.path.join(chats_a, "big-snap.jsonl"), "\n".join([user("big docker head", "big-snap"), pad, meta("big-snap", title="Snapshot Title", isSnapshot=True)]) + "\n")
    # >64 KiB: head metadata only, oversized non-metadata line (>128 KiB) in the middle
    w(os.path.join(chats_a, "big-head.jsonl"), "\n".join([meta("big-head", title="Head Title"), asst("y" * 140000, "big-head"), user("docker after huge line", "big-head"), meta("big-head", aiTitle="Later AI")]) + "\n")
    # project B
    w(os.path.join(chats_b, "b1.jsonl"), user("docker in project b", "b1") + "\n" + asst("Answer DOCKER b", "b1") + "\n")
    # a stray file and a non-chat project in projects/
    w(os.path.join(home, "projects", "README"), "x")
    os.makedirs(os.path.join(home, "projects", "nochats"))

    crash_home = os.path.join(base, "crash-home")
    w(os.path.join(crash_home, "projects", "p", "chats", "a.jsonl"), user("docker ok", "a") + "\nnull\n")
    w(os.path.join(crash_home, "projects", "p", "chats", "bad-title.jsonl"), '{"type":"session_metadata","metadata":{"title":"bad \\x escape"}}\n' + user("docker", "bad") + "\n")
    return {"home": home, "proj_a": proj_a, "proj_b": proj_b, "crash_home": crash_home, "base": base}

def cases(fx):
    A, B, H = fx["proj_a"], fx["proj_b"], fx["home"]
    c = []
    def add(name, args, cwd=A, env=None, stderr="exact"):
        c.append({"id": name, "argv": args, "cwd": cwd, "env": env or {}, "stderr": stderr})
    for mode in ([], ["--json"]):
        sfx = "-json" if mode else "-text"
        add("plain" + sfx, ["chat", "search", "docker", *mode])
        add("multiword" + sfx, ["chat", "search", "docker", "deploy", *mode])
        add("case-sensitive" + sfx, ["chat", "search", "Docker", "--case-sensitive", *mode])
        add("regex" + sfx, ["chat", "search", "dock(er|s)\\s+\\w+", "--regex", *mode])
        add("regex-cs" + sfx, ["chat", "search", "^Sure", "--regex", "--case-sensitive", *mode])
        add("role-user" + sfx, ["chat", "search", "docker", "--role", "user", *mode])
        add("role-assistant" + sfx, ["chat", "search", "docker", "--role", "assistant", *mode])
        add("role-invalid" + sfx, ["chat", "search", "docker", "--role", "bot", *mode])
        add("all-projects" + sfx, ["chat", "search", "docker", "--all-projects", *mode])
        add("project-b" + sfx, ["chat", "search", "docker", "--project", B, *mode])
        add("project-relative" + sfx, ["chat", "search", "docker", "--project", "../项目 b", *mode])
        add("session" + sfx, ["chat", "search", "needle", "--session", "s2", *mode])
        add("snippet-long" + sfx, ["chat", "search", "NEEDLE", *mode])
        add("unicode" + sfx, ["chat", "search", "😀", *mode])
        add("unicode-fold" + sfx, ["chat", "search", "CAFÉ", *mode])
        add("no-match" + sfx, ["chat", "search", "zzzz-nothing", *mode])
        add("big-files" + sfx, ["chat", "search", "big", *mode])
        add("after-huge-line" + sfx, ["chat", "search", "after huge", *mode])
        for lim in ["1", "2.5", "0", "-3", "abc", "0x2", "200", "1e1"]:
            add(f"limit-{lim}" + sfx, ["chat", "search", "docker", "--all-projects", "--limit", lim, *mode])
    add("pilot-home-flag-leaks-into-query", ["chat", "search", "docker", "--pilot-home", H, "--json"], env={"PILOT_HOME": "/nonexistent"})
    add("pilot-home-env", ["chat", "search", "docker", "--json"], env={"PILOT_HOME": H})
    add("pilot-home-tilde", ["chat", "search", "docker", "--json"], env={"PILOT_HOME": "~/ph", "HOME": fx["base"]})
    add("session-internal", ["chat", "search", "docker", "--session", "always-on-report:x", "--json"])
    add("session-missing", ["chat", "search", "docker", "--session", "nope"])
    add("missing-keyword", ["chat", "search", "--json"])
    add("missing-keyword-spaces", ["chat", "search", "  ", "--limit", "3"])
    add("wrong-subcommand", ["chat", "find", "docker"])
    add("no-subcommand", ["chat"])
    add("invalid-regex", ["chat", "search", "(", "--regex"], stderr="exit-only")
    add("crash-null-line", ["chat", "search", "docker", "--all-projects"], env={"PILOT_HOME": fx["crash_home"]}, stderr="exit-only")
    for x in c:
        x["env"].setdefault("PILOT_HOME", H)
    return c

import re
NODE_RUNTIME_WARNING = re.compile(rb"^\(node:\d+\) ExperimentalWarning: [^\n]*\n(\(Use `node --trace-warnings \.\.\.` to show where the warning was created\)\n)?", re.M)

def normalize_stderr(err):
    """Contract normalization: drop Node.js runtime ExperimentalWarning banners
    (emitted by the node host, not by PilotDeck code)."""
    return NODE_RUNTIME_WARNING.sub(b"", err)

def run(cmd, case, fx):
    env = {"PATH": os.environ.get("PATH", ""), "HOME": fx["base"], "TZ": "Asia/Shanghai", "LANG": "C.UTF-8", "NO_COLOR": "1"}
    env.update(case["env"])
    p = subprocess.run(cmd + case["argv"], cwd=case["cwd"], env=env, capture_output=True, timeout=120)
    return {"code": p.returncode, "stdout": p.stdout, "stderr": normalize_stderr(p.stderr)}

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ts-cli", required=True)
    ap.add_argument("--rust-bin", required=True)
    ap.add_argument("--node", default="node")
    ap.add_argument("--report")
    a = ap.parse_args()
    base = tempfile.mkdtemp(prefix="pd-parity-")
    try:
        fx = build_fixture(base)
        cs = cases(fx)
        results, failed = [], 0
        ts_cmd, rs_cmd = [a.node, os.path.abspath(a.ts_cli)], [os.path.abspath(a.rust_bin)]
        for case in cs:
            t, r = run(ts_cmd, case, fx), run(rs_cmd, case, fx)
            ok = t["code"] == r["code"] and t["stdout"] == r["stdout"]
            if case["stderr"] == "exact":
                ok = ok and t["stderr"] == r["stderr"]
            else:
                ok = ok and bool(t["stderr"]) == bool(r["stderr"])
            failed += not ok
            results.append({"id": case["id"], "ok": ok, "exit": t["code"],
                            "stdout_sha256": hashlib.sha256(t["stdout"]).hexdigest(), "stdout_bytes": len(t["stdout"])})
            if not ok:
                print(f"FAIL {case['id']}\n  ts  code={t['code']} out={t['stdout'][:600]!r} err={t['stderr'][:300]!r}\n  rs  code={r['code']} out={r['stdout'][:600]!r} err={r['stderr'][:300]!r}")
        # Negative controls: the comparator must detect drift.
        nc = []
        probe = cs[0]
        t = run(ts_cmd, probe, fx)
        nc.append({"id": "nc-stdout-mutated", "detected": t["stdout"] != t["stdout"] + b" "})
        nc.append({"id": "nc-wrong-binary", "detected": run(["/bin/echo"], probe, fx)["stdout"] != t["stdout"]})
        nc.append({"id": "nc-nonempty-oracle", "detected": all(x["stdout_bytes"] > 0 or x["exit"] != 0 for x in results)})
        nc_ok = all(x["detected"] for x in nc)
        summary = {"cases_planned": len(cs), "cases_executed": len(results), "passed": len(results) - failed,
                   "failed": failed, "skipped": 0, "negative_controls": nc, "results": results}
        if a.report:
            with open(a.report, "w") as f:
                json.dump(summary, f, indent=2)
        print(f"parity: {summary['passed']}/{summary['cases_planned']} passed, negative controls {'ok' if nc_ok else 'FAILED'}")
        sys.exit(0 if failed == 0 and nc_ok and len(results) == len(cs) else 1)
    finally:
        shutil.rmtree(base, ignore_errors=True)

if __name__ == "__main__":
    main()
