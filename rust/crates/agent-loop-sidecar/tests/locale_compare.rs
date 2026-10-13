//! `locale_compare` vs node's `localeCompare` (fixture from rust/parity/gen_locale_compare_fixture.mjs).
use pilotdeck_agent_loop_sidecar::cacheplan::locale_compare;
use pilotdeck_agent_loop_sidecar::json::*;
use std::cmp::Ordering;

#[test]
fn matches_node_locale_compare_on_ascii() {
    let fx = pilotdeck_skills::v8json::parse(include_str!("fixtures/locale_compare.json")).unwrap();
    let mut bad = Vec::new();
    let all = arr(&fx).unwrap();
    for p in all {
        let p = arr(p).unwrap();
        let (a, b, want) = (str_of(&p[0]).unwrap(), str_of(&p[1]).unwrap(), match &p[2] { JsValue::Num(n) => *n, _ => panic!() });
        let got = match locale_compare(a, b) { Ordering::Less => -1.0, Ordering::Equal => 0.0, Ordering::Greater => 1.0 };
        if got != want { bad.push(format!("{a:?} vs {b:?}: node {want} rust {got}")); }
    }
    assert!(bad.is_empty(), "{} / {} mismatches, e.g. {:?}", bad.len(), all.len(), &bad[..bad.len().min(15)]);
}
