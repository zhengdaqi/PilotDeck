//! Oracle test against `rust/parity/ui_server/gen_frontmatter.mjs` (gray-matter 4.0.3 + js-yaml 3.14).
use pilotdeck_ui_server_core::command_parser::parse_command;
use pilotdeck_ui_server_core::frontmatter::{parse_frontmatter, Y};
use regex::Regex;
use serde_json::{json, Value};

/// The expected Rust result for the fixed defects, derived from the TS result. The only date key atom in the
/// generator is 2020-01-01.
/// - SRC-DEFECT-013: a `String(Date)` key becomes the key's source text.
/// - SRC-DEFECT-014: a `---toString` engine is reported as not registered instead of running
///   Object.prototype.toString.
fn fixed(input: &str, ts: &Value, date_str: &Regex, prefix: &str) -> Value {
    let mut f: Value = serde_json::from_str(&date_str.replace_all(&ts.to_string(), "2020-01-01")).unwrap();
    if input.contains("---toString") && ts.get("ok").is_some_and(|o| o["data"] != json!({})) {
        f = json!({ "err": format!("{prefix}gray-matter engine \"toString\" is not registered") });
    }
    f
}

/// gray-matter's own errors are compared exactly. YAML errors compare js-yaml's reason; the
/// " at line L, column C:\n<snippet>" mark that js-yaml appends is not reproduced.
fn same(ts: &Value, got: &Value, err_kind: &mut usize) -> bool {
    match (ts.get("err").and_then(Value::as_str), got.get("err").and_then(Value::as_str)) {
        (Some(t), Some(g)) if t.contains("gray-matter engine") || g.contains("gray-matter engine") => t == g,
        (Some(t), Some(g)) => {
            *err_kind += 1;
            t.split(" at line ").next().unwrap() == g
        }
        _ => ts.to_string() == got.to_string(), // string compare: key order matters
    }
}

#[test]
fn frontmatter_and_parse_command_match_ts() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/frontmatter.json")).unwrap();
    let date_str = Regex::new(r"[A-Z][a-z]{2} [A-Z][a-z]{2} \d\d \d{4} \d\d:\d\d:\d\d GMT[+-]\d{4} \([^)]*\)").unwrap();
    let (mut exact, mut err_kind, mut intended, mut bad) = (0, 0, 0, vec![]);
    for c in &cases {
        let input = c["input"].as_str().unwrap();
        let got = match parse_frontmatter(input) {
            Ok(m) => json!({"ok": {"data": m.data.as_ref().map_or(json!("__undefined"), Y::to_json), "content": m.content}}),
            Err(e) => json!({ "err": e }),
        };
        let cmd = match parse_command(input) {
            Ok(v) => json!({ "ok": v }),
            Err(e) => json!({ "err": e }),
        };
        let ts = fixed(input, &c["out"], &date_str, "");
        let tsc = fixed(input, &c["cmd"], &date_str, "Failed to parse command: ");
        if ts != c["out"] {
            intended += 1;
        }
        if same(&ts, &got, &mut err_kind) && same(&tsc, &cmd, &mut err_kind) {
            exact += 1;
        } else {
            let cut = |v: &Value| v.to_string().chars().take(220).collect::<String>();
            bad.push(format!("{input:?}\n ts {}\n rs {}\n tsc {}\n rsc {}", cut(&ts), cut(&got), cut(&tsc), cut(&cmd)));
        }
    }
    eprintln!("exact {exact} (yaml error reason only: {err_kind}) intended {intended} bad {}", bad.len());
    assert!(bad.is_empty(), "{} mismatches:\n{}", bad.len(), bad.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
    assert!(exact > 3900 && intended > 300, "{exact} {intended}");
}
