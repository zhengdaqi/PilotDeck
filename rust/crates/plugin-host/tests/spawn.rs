//! The builtin funasr MCP server starts as a Node child process from the resolved spec and answers
//! MCP `initialize` + `tools/list` over stdio (ADR-0004 option 1).
use pilotdeck_plugin_host::{patch_project_scoped_mcp_spec, spawn_stdio, HostContext};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};

#[test]
fn funasr_mcp_runs_as_node_subprocess() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().unwrap();
    let module_dir = root.join("dist/src/mcp/runtime");
    if !module_dir.exists() {
        panic!("dist/ missing: run the TS build first");
    }
    let ctx = HostContext {
        node_executable: HostContext::node_from_env(),
        module_dir: module_dir.display().to_string(),
        runtime_root: None,
        cwd: root.display().to_string(),
    };
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(root.join("src/extension/plugins/builtin/funasr/plugin.json")).unwrap()).unwrap();
    let mut spec = manifest["mcpServers"]["funasr"].clone();
    spec["id"] = "funasr".into();
    spec["transport"] = "stdio".into();
    let home = std::env::temp_dir().join(format!("pd-plughost-home-{}", std::process::id()));
    let proj = std::env::temp_dir().join(format!("pd-plughost-proj-{}", std::process::id()));
    std::fs::create_dir_all(&proj).unwrap();
    let resolved = patch_project_scoped_mcp_spec(&ctx, &spec, &proj.display().to_string(), &home.display().to_string()).unwrap();
    let mut child = spawn_stdio(&resolved).expect("spawn node");
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut rpc = |id: i64, method: &str, params: Value| -> Value {
        writeln!(stdin, "{}", json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        loop {
            line.clear();
            assert!(out.read_line(&mut line).unwrap() > 0, "server closed");
            let v: Value = match serde_json::from_str(&line) { Ok(v) => v, Err(_) => continue };
            if v["id"] == json!(id) {
                return v;
            }
        }
    };
    let init = rpc(1, "initialize", json!({"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "rust-plugin-host", "version": "0"}}));
    assert!(init["result"].is_object(), "initialize: {init}");
    let tools = rpc(2, "tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    eprintln!("funasr tools via Node subprocess: {names:?}");
    assert!(!names.is_empty());
    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&proj);
}
