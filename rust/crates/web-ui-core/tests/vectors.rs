#[test]
fn workspace_file_mention_matches_ts() {
    let fx = include_str!("fixtures/workspace_file_mention.json");
    let (n, bad) = pilotdeck_web_ui_core::vectors::run_workspace_file_mention(fx);
    eprintln!("web-ui-core workspaceFileMention: {n} vectors, {} mismatches", bad.len());
    for b in bad.iter().take(8) {
        eprintln!("{b}");
    }
    assert!(bad.is_empty());
}

#[test]
fn batch2_matches_ts() {
    let (n, bad) = pilotdeck_web_ui_core::vectors::run_batch2(include_str!("fixtures/batch2.json"), None);
    eprintln!("web-ui-core batch2: {n} checks, {} mismatches", bad.len());
    for b in bad.iter().take(8) {
        eprintln!("{b}");
    }
    assert!(bad.is_empty());
}

#[test]
fn batch3_matches_ts() {
    let (n, bad) = pilotdeck_web_ui_core::vectors::run_batch3(include_str!("fixtures/batch3.json"));
    eprintln!("web-ui-core batch3: {n} checks, {} mismatches", bad.len());
    for b in bad.iter().take(8) {
        eprintln!("{b}");
    }
    assert!(bad.is_empty());
}
