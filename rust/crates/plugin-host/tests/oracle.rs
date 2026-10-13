//! Replays rust/parity/plugin_host/gen_fixture.mjs (TS dist `patchProjectScopedMcpSpec`).
//! The fixture is host-bound (absolute paths, temp dirs): regenerate it with
//! `node rust/parity/plugin_host/gen_fixture.mjs` before running on a new machine.
use pilotdeck_plugin_host::{install_command, patch_project_scoped_mcp_spec, HostContext};
use serde_json::{json, Value};

#[test]
fn project_mcp_spec_matches_ts() {
    let fx: Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/project_mcp_spec.json")).unwrap()).unwrap();
    let tmp = fx["tmp"].as_str().unwrap();
    assert!(std::path::Path::new(tmp).exists(), "fixture temp dir {tmp} is gone: regenerate with node rust/parity/plugin_host/gen_fixture.mjs");
    let (mut n, mut bad) = (0, vec![]);
    for c in fx["cases"].as_array().unwrap() {
        let ctx = HostContext {
            node_executable: fx["nodeExecutable"].as_str().unwrap().into(),
            module_dir: fx["moduleDir"].as_str().unwrap().into(),
            runtime_root: c["runtimeRoot"].as_str().map(str::to_string),
            cwd: fx["cwd"].as_str().unwrap().into(),
        };
        n += 1;
        let (want, got) = if let Some(ic) = c.get("installCommand") {
            (ic.clone(), Value::from(install_command(&ctx)))
        } else {
            let got = match patch_project_scoped_mcp_spec(&ctx, &c["spec"], c["projectRoot"].as_str().unwrap(), c["pilotHome"].as_str().unwrap()) {
                Ok(v) => v,
                Err(_) => json!({"__error": "TypeError"}),
            };
            (c["out"].clone(), got)
        };
        if want != got {
            bad.push(format!("{c}\n  got {got}"));
        }
    }
    eprintln!("plugin-host oracle: {n} cases, {} mismatches", bad.len());
    for b in bad.iter().take(5) {
        eprintln!("{b}");
    }
    assert!(bad.is_empty());
}
