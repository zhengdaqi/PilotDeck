use pilotdeck_gateway_server::files::name_locale_compare;
#[test]
fn matches_node_locale_compare() {
    let cases: Vec<(String, String, i32)> = serde_json::from_str(include_str!("fixtures/locale_names.json")).unwrap();
    let mut bad = Vec::new();
    for (a, b, want) in &cases {
        let got = name_locale_compare(a, b) as i32;
        if got != *want { bad.push((a.clone(), b.clone(), *want, got)); }
    }
    assert!(bad.is_empty(), "{} / {} mismatches, first: {:?}", bad.len(), cases.len(), &bad[..bad.len().min(15)]);
}
