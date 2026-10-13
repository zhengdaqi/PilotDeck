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
