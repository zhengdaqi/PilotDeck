//! Oracle test against `rust/parity/ui_server/gen_bash_commands.mjs` (TS processBashCommands with real processes).
use pilotdeck_ui_server_core::command_parser::process_bash_commands;
use serde_json::{json, Value};
use std::time::Duration;

#[test]
fn bash_commands_match_ts_except_src_defect_010() {
    let f: Value = serde_json::from_str(include_str!("fixtures/bash_commands.json")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let dir_path = std::fs::canonicalize(dir.path()).unwrap();
    for (k, v) in f["files"].as_object().unwrap() {
        std::fs::write(dir_path.join(k), v.as_str().unwrap()).unwrap();
    }
    // SRC-DEFECT-010: command output is inserted literally (TS expands `$&` into the matched `!command`).
    let intended = [("!cat a.txt", "alpha\nbeta $& gamma\n"), ("!grep beta a.txt", "beta $& gamma\n")];
    let mut seen = 0;
    for c in f["cases"].as_array().unwrap() {
        let content = c["c"].as_str().unwrap();
        let got = match process_bash_commands(content, &dir_path, Duration::from_secs(30)) {
            Ok(s) => json!({ "ok": s.replace(dir_path.to_str().unwrap(), "<dir>") }),
            Err(e) => json!({ "err": e.replace(dir_path.to_str().unwrap(), "<dir>") }),
        };
        if let Some((_, want)) = intended.iter().find(|(k, _)| *k == content) {
            assert_ne!(c["ts"], json!({ "ok": want }), "TS no longer shows the defect");
            assert_eq!(got, json!({ "ok": want }));
            seen += 1;
        } else {
            assert_eq!(got, c["ts"], "{content:?}");
        }
    }
    assert_eq!(seen, intended.len());
}

#[test]
fn bash_command_timeout_reports_like_ts() {
    let dir = tempfile::tempdir().unwrap();
    // `find /` with a tiny timeout: TS maps a killed child to "Command timeout: <command>".
    let r = process_bash_commands("!find / -name nothing-here", dir.path(), Duration::from_millis(20));
    assert_eq!(r.unwrap_err(), "Command timeout: find / -name nothing-here");
}
