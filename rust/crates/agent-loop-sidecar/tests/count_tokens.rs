//! `count_tokens` vs js-tiktoken (fixture from rust/parity/gen_count_tokens_fixture.mjs).
use pilotdeck_agent_loop_sidecar::json::*;
use pilotdeck_agent_loop_sidecar::tokenizer::count_tokens;

#[test]
fn matches_js_tiktoken_o200k() {
    let fx = pilotdeck_skills::v8json::parse(include_str!("fixtures/count_tokens.json")).unwrap();
    let all = arr(&fx).unwrap();
    let mut bad = Vec::new();
    for c in all {
        let c = arr(c).unwrap();
        let text = str_of(&c[0]).unwrap();
        let want = match &c[1] { JsValue::Num(n) => *n as usize, _ => panic!() };
        let got = count_tokens(text);
        if got != want { bad.push(format!("{text:?}: js {want} rust {got}")); }
    }
    assert!(bad.is_empty(), "{} / {} mismatches: {:?}", bad.len(), all.len(), &bad[..bad.len().min(10)]);
}
