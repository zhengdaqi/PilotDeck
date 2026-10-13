#!/usr/bin/env python3
"""Black-box parity runner for `pilotdeck skills migrate` (slice 2, file I/O + side effects).

For every case a fresh fixture is built at the SAME absolute path for the TS
oracle and for the Rust binary; we compare exit code, stdout, stderr (after
removing Node runtime warning banners) and a snapshot of the resulting
pilot-home tree (paths, types, sizes, content hashes, symlink targets, modes).
"""
import argparse, hashlib, json, os, shutil, stat, sys, tempfile
sys.path.insert(0, os.path.dirname(__file__))
from chat_search_parity import normalize_stderr, oracle_broken  # noqa: E402

def w(path, text="", data=None, mode=None):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(data if data is not None else text.encode())
    if mode:
        os.chmod(path, mode)

def skill(root, slug, fm, body="# body\n", extra=()):
    w(os.path.join(root, slug, "SKILL.md"), fm + body)
    for rel, data in extra:
        w(os.path.join(root, slug, rel), data=data if isinstance(data, bytes) else data.encode())

GOOD = "---\nname: {n}\ndescription: A sufficiently long description for {n}.\n---\n"

def build(base, variant):
    home = os.path.join(base, "home"); ph = os.path.join(base, "pilot-home"); proj = os.path.join(base, "proj")
    cc = os.path.join(home, ".claude", "skills")
    skill(cc, "alpha", GOOD.format(n="alpha"), extra=[("scripts/run.sh", "echo hi\n"), ("README.md", "x"), ("big.bin", b"\0" * 1310720)])
    skill(cc, "Beta", "---\nname: Beta\ndescription: short\nversion: 1.0\ncount: 42\nratio: 0.5\ntags: [a, b]\nnested:\n  x: null\n  '10': ten\n  '2': two\nenabled: true\n---\n")
    skill(cc, "no-desc", "---\nname: no-desc\n---\n")
    skill(cc, "no-fm", "# just markdown\n")
    skill(cc, "compat", "---\nname: compat: colon value\ndescription: |\n  Block description that is\n  long enough to pass.\nother: 'quoted'\n---\n")
    skill(cc, "empty-md", "")
    skill(cc, "_under", GOOD.format(n="under"))
    skill(cc, "a.b-c_d", GOOD.format(n="dots"))
    skill(cc, "existing", GOOD.format(n="existing"))
    skill(cc, "bad slug", GOOD.format(n="bad"))
    os.makedirs(os.path.join(cc, "not-a-skill"))
    skill(cc, "linked-inside", GOOD.format(n="linked"))
    os.symlink("../alpha/README.md", os.path.join(cc, "linked-inside", "link.md"))
    os.symlink(os.path.join(cc, "alpha"), os.path.join(cc, "zz-symlinked-skill"))
    w(os.path.join(cc, "stray.txt"), "x")
    # project-level claude skills
    skill(os.path.join(proj, ".claude", "skills"), "proj-skill", GOOD.format(n="proj"))
    # openclaw duplicate slug + hermes
    skill(os.path.join(home, ".openclaw", "workspace", "skills"), "alpha", GOOD.format(n="alpha-oc"))
    skill(os.path.join(home, ".openclaw", "skills"), "oc-only", GOOD.format(n="oc"), extra=[("tool.EXE", "MZ")])
    os.makedirs(os.path.join(home, ".hermes", "skills"))  # empty -> skipped
    if variant == "hermes-file":
        shutil.rmtree(os.path.join(home, ".hermes", "skills")); w(os.path.join(home, ".hermes", "skills"), "not a dir")
    # custom single-skill root and parent dir
    skill(os.path.join(base, "custom"), "single", GOOD.format(n="single"))
    # pilot home with pre-existing skills for conflict/rename
    skill(os.path.join(ph, "skills"), "existing", GOOD.format(n="old"))
    skill(os.path.join(ph, "skills"), "existing-imported", GOOD.format(n="old2"))
    return {"home": home, "ph": ph, "proj": proj, "base": base}

def snapshot(root):
    out = []
    for dp, dn, fn in os.walk(root):
        dn.sort()
        for name in sorted(dn + fn):
            p = os.path.join(dp, name); st = os.lstat(p); rel = os.path.relpath(p, root)
            if stat.S_ISLNK(st.st_mode):
                out.append((rel, "link", os.readlink(p)))
            elif stat.S_ISDIR(st.st_mode):
                out.append((rel, "dir", oct(st.st_mode & 0o777)))
            else:
                out.append((rel, "file", oct(st.st_mode & 0o777), hashlib.sha256(open(p, "rb").read()).hexdigest()))
    return sorted(set(out))

def cases():
    c = []
    def add(i, argv, variant="std", env=None, cwd="proj"):
        c.append({"id": i, "argv": ["skills", *argv], "variant": variant, "env": env or {}, "cwd": cwd})
    for j in ([], ["--json"]):
        x = "-json" if j else "-text"
        add("dry-default" + x, ["migrate", *j])
        add("dry-from-cc" + x, ["migrate", "--from", "cc", *j])
        add("dry-from-openclaw-hermes" + x, ["migrate", "--from", "openclaw,HERMES, ,claude", *j])
        add("dry-rename" + x, ["migrate", "--from", "cc", "--rename", *j])
        add("dry-overwrite" + x, ["migrate", "--from", "cc", "--overwrite", *j])
        add("dry-custom" + x, ["migrate", "--from", "hermes", "--source", "../custom/single", "--source", "~/.openclaw/skills", "--source", "--json", *j])
        add("exec-skip" + x, ["migrate", "--execute", *j])
        add("exec-rename" + x, ["migrate", "--execute", "--rename", "--from", "all", *j])
        add("exec-overwrite" + x, ["migrate", "--execute", "--overwrite", "--from", "cc", *j])
        add("exec-self-import" + x, ["migrate", "--execute", "--overwrite", "--from", "hermes", "--source", "PH/skills", *j])
        add("hermes-not-dir" + x, ["migrate", "--from", "hermes", *j], variant="hermes-file")
    add("pilot-home-flag", ["migrate", "--from", "cc", "--pilot-home", "../alt-home", "--execute", "--json"])
    add("project-flag", ["migrate", "--from", "cc", "--project", "/nonexistent/project"])
    add("unknown-source", ["migrate", "--from", "cc,bogus"])
    add("empty-from", ["migrate", "--from", "", "--json"])
    add("wrong-subcommand", ["list"])
    add("no-subcommand", [])
    return c

def run(cmd, case, fx):
    env = {"PATH": os.environ.get("PATH", ""), "HOME": fx["home"], "PILOT_HOME": fx["ph"], "LANG": "C.UTF-8", "TZ": "UTC"}
    env.update(case["env"])
    argv = [a.replace("PH/", fx["ph"] + "/") for a in case["argv"]]
    import subprocess
    p = subprocess.run(cmd + argv, cwd=fx[case["cwd"]], env=env, capture_output=True, timeout=120)
    return {"code": p.returncode, "stdout": p.stdout, "stderr": normalize_stderr(p.stderr),
            "tree": snapshot(fx["base"])}

def one(cmd, case, base):
    if os.path.exists(base):
        shutil.rmtree(base)
    os.makedirs(base)
    return run(cmd, case, build(base, case["variant"]))

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ts-cli", required=True); ap.add_argument("--rust-bin", required=True)
    ap.add_argument("--node", default="node"); ap.add_argument("--report")
    a = ap.parse_args()
    root = tempfile.mkdtemp(prefix="pd-skills-"); base = os.path.join(root, "fx")
    ts, rs = [a.node, os.path.abspath(a.ts_cli)], [os.path.abspath(a.rust_bin)]
    results, failed = [], 0
    try:
        for case in cases():
            t = None
            for _ in range(3):
                t = one(ts, case, base)
                if not oracle_broken(t):
                    break
            if oracle_broken(t):
                failed += 1; results.append({"id": case["id"], "ok": False, "oracle_unavailable": True}); continue
            r = one(rs, case, base)
            ok = all(t[k] == r[k] for k in ("code", "stdout", "stderr", "tree"))
            failed += not ok
            results.append({"id": case["id"], "ok": ok, "exit": t["code"], "stdout_bytes": len(t["stdout"]),
                            "stdout_sha256": hashlib.sha256(t["stdout"]).hexdigest(), "tree_entries": len(t["tree"])})
            if not ok:
                print(f"FAIL {case['id']}")
                for k in ("code", "stdout", "stderr"):
                    if t[k] != r[k]:
                        print(f"  {k}: ts={t[k]!r:.1500}\n  {k}: rs={r[k]!r:.1500}")
                if t["tree"] != r["tree"]:
                    print("  tree ts-only:", sorted(set(t["tree"]) - set(r["tree"]))[:8])
                    print("  tree rs-only:", sorted(set(r["tree"]) - set(t["tree"]))[:8])
        # negative controls
        probe = cases()[0]
        t = one(ts, probe, base)
        nc = [{"id": "nc-stdout-mutated", "detected": t["stdout"] != t["stdout"] + b"x"},
              {"id": "nc-tree-mutated", "detected": t["tree"] != t["tree"][1:]},
              {"id": "nc-wrong-binary", "detected": one(["/bin/echo"], probe, base)["stdout"] != t["stdout"]}]
        nc_ok = all(x["detected"] for x in nc)
        n = len(cases())
        summary = {"cases_planned": n, "cases_executed": len(results), "passed": len(results) - failed, "failed": failed,
                   "skipped": 0, "negative_controls": nc, "results": results}
        if a.report:
            json.dump(summary, open(a.report, "w"), indent=2)
        print(f"parity: {summary['passed']}/{n} passed, negative controls {'ok' if nc_ok else 'FAILED'}")
        sys.exit(0 if failed == 0 and nc_ok and len(results) == n else 1)
    finally:
        shutil.rmtree(root, ignore_errors=True)

if __name__ == "__main__":
    main()
