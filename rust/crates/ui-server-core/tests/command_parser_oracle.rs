//! Oracle test against `rust/parity/ui_server/gen_command_parser.mjs` (TS commandParser.js + shell-quote).
use pilotdeck_ui_server_core::command_parser::*;
use serde_json::{json, Value};
use std::path::Path;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/command_parser.json")).unwrap()
}
fn args_of(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a.iter().map(|x| x.as_str().unwrap().to_string()).collect(),
        Value::String(s) if !s.is_empty() => vec![s.clone()],
        _ => vec![],
    }
}

#[test]
fn replace_arguments_matches_fixed_reference() {
    let (mut n, mut intended) = (0, 0);
    for c in fixture()["replace"].as_array().unwrap() {
        let args = args_of(&c["args"]);
        let got = replace_arguments(c["content"].as_str().unwrap(), &args);
        assert_eq!(got, c["ref"].as_str().unwrap(), "{c}");
        if c["ts"] != c["ref"] {
            // SRC-DEFECT-010 needs a '$' inside an argument value.
            assert!(args.iter().any(|a| a.contains('$')), "unexpected TS difference {c}");
            intended += 1;
        } else {
            n += 1;
        }
    }
    assert!(n > 1000 && intended > 100, "{n} {intended}");
}

#[test]
fn path_safe_sanitize_and_validate_match_ts() {
    let f = fixture();
    for c in f["pathSafe"].as_array().unwrap() {
        assert_eq!(json!(is_path_safe(c["f"].as_str().unwrap(), c["b"].as_str().unwrap())), c["out"], "{c}");
    }
    for c in f["sanitize"].as_array().unwrap() {
        assert_eq!(json!(sanitize_output(c["s"].as_str().unwrap())), c["out"], "{c}");
    }
    let mut n = 0;
    for c in f["validate"].as_array().unwrap() {
        let got = match validate_command(c["s"].as_str().unwrap()) {
            Ok(v) => json!({ "ok": v }),
            Err(e) => json!({ "err": e }),
        };
        assert_eq!(got, c["out"], "{}", c["s"]);
        n += 1;
    }
    assert_eq!(n, 3000);
}

#[test]
fn file_includes_match_ts_except_fixed_defects() {
    let f = fixture();
    let inc = &f["includes"];
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base");
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(base.join("sub")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    for (k, v) in inc["outside"].as_object().unwrap() {
        std::fs::write(outside.join(k), v.as_str().unwrap()).unwrap();
    }
    for (k, v) in inc["files"].as_object().unwrap() {
        std::fs::write(base.join(k), v.as_str().unwrap()).unwrap();
    }
    for l in inc["links"].as_array().unwrap() {
        std::os::unix::fs::symlink(l[1].as_str().unwrap(), base.join(l[0].as_str().unwrap())).unwrap();
    }
    let trav = |f: &str| json!({ "err": format!("Invalid file path (directory traversal detected): {f}") });
    // Fixed behaviour where TS differs (SRC-DEFECT-010/011/012).
    let intended: Vec<(&str, Value)> = vec![
        ("@dollar.md", json!({"ok": "cost $& $' $` $$ end"})),
        ("@link_out", trav("link_out")),
        ("@dirlink/secret.txt", trav("dirlink/secret.txt")),
        ("x\n@a.md", json!({"ok": "x\nA"})),
        ("x\t@a.md", json!({"ok": "x\tA"})),
        ("@nl.md", json!({"ok": "line1\nA\nline3"})),
        ("x\r\n@a.md", json!({"ok": "x\r\nA"})),
        ("@a.md\n@b.md", json!({"ok": "A\nB A"})),
    ];
    let base_s = base.to_str().unwrap();
    let mut seen = 0;
    for c in inc["cases"].as_array().unwrap() {
        let content = c["c"].as_str().unwrap();
        let got = match process_file_includes(content, base_s, 0) {
            Ok(s) => json!({ "ok": s }),
            Err(e) => json!({ "err": e.replace(dir.path().to_str().unwrap(), "<dir>") }),
        };
        if let Some((_, want)) = intended.iter().find(|(k, _)| *k == content) {
            assert_ne!(&c["ts"], want, "TS no longer shows the defect for {content:?}");
            assert_eq!(&got, want, "{content:?}");
            seen += 1;
        } else {
            assert_eq!(got, c["ts"], "{content:?}");
        }
    }
    assert_eq!(seen, intended.len());
    assert!(Path::new(&outside.join("secret.txt")).exists());
}
