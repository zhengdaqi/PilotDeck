//! `parse_plan_todo_snapshot` vs TS parsePlanTodoSnapshot (fixture from rust/parity/gen_plan_todo_fixture.mjs).
use pilotdeck_agent_loop_sidecar::json::*;
use pilotdeck_agent_loop_sidecar::obj;
use pilotdeck_agent_loop_sidecar::plantodo::parse_plan_todo_snapshot;

#[test]
fn matches_ts_parse_plan_todo_snapshot() {
    let fx = pilotdeck_skills::v8json::parse(include_str!("fixtures/plan_todo.json")).unwrap();
    let all = arr(&fx).unwrap();
    let mut bad = Vec::new();
    for (i, c) in all.iter().enumerate() {
        let input = match get(c, "input") { JsValue::Str(s) if s == "__undefined__" => JsValue::Undefined, v => v.clone() };
        let got = match parse_plan_todo_snapshot(&input) { Ok(v) => obj! {"ok" => v}, Err(e) => obj! {"err" => e} };
        let want = match get(c, "err") { JsValue::Undefined => obj! {"ok" => get(c, "ok")}, e => obj! {"err" => e} };
        if to_json(&got) != to_json(&want) { bad.push(format!("#{i}: want {} got {}", to_json(&want), to_json(&got))); }
    }
    assert!(bad.is_empty(), "{} / {} mismatches: {:?}", bad.len(), all.len(), &bad[..bad.len().min(5)]);
}
