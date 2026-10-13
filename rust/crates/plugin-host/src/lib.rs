//! JS plugin host (ADR-0004): plugins that ship MCP servers run as **Node child processes**.
//! `patch_project_scoped_mcp_spec` ports `src/mcp/runtime/projectMcpSpec.ts`; `spawn_stdio`
//! starts the resolved spec. The Node executable replaces TS `process.execPath`.
use pilotdeck_session_search::paths::{join, resolve};
use serde_json::{Map, Value};
use std::path::Path;
use std::process::{Child, Command, Stdio};

pub const PROJECT_ROOT_MARKER: &str = "__PILOTDECK_PROJECT_ROOT__";
pub const NODE_EXECUTABLE_MARKER: &str = "__PILOTDECK_NODE_EXECUTABLE__";
pub const FUNASR_MCP_ENTRYPOINT_MARKER: &str = "__PILOTDECK_FUNASR_MCP_ENTRYPOINT__";
pub const FUNASR_RUNTIME_ROOT_MARKER: &str = "__PILOTDECK_FUNASR_RUNTIME_ROOT__";
pub const FUNASR_INSTALL_COMMAND_MARKER: &str = "__PILOTDECK_FUNASR_INSTALL_COMMAND__";
pub const PLAYWRIGHT_MCP_ENTRYPOINT_MARKER: &str = "__PILOTDECK_PLAYWRIGHT_MCP_ENTRYPOINT__";

/// Host facts the TS module reads from its process.
#[derive(Debug, Clone)]
pub struct HostContext {
    /// TS `process.execPath`: the Node used for plugin subprocesses.
    pub node_executable: String,
    /// TS `MODULE_DIR` (directory of the compiled `projectMcpSpec.js`, i.e. `<root>/dist/src/mcp/runtime`).
    pub module_dir: String,
    /// `process.env.PILOTDECK_RUNTIME_ROOT`.
    pub runtime_root: Option<String>,
    /// `process.cwd()`.
    pub cwd: String,
}

impl HostContext {
    /// Node executable resolution for the Rust runtime: `PILOTDECK_NODE_EXECUTABLE`, else `node` on PATH.
    pub fn node_from_env() -> String {
        if let Ok(p) = std::env::var("PILOTDECK_NODE_EXECUTABLE") {
            if !p.is_empty() {
                return p;
            }
        }
        if let Ok(path) = std::env::var("PATH") {
            for d in path.split(':').filter(|d| !d.is_empty()) {
                let c = format!("{d}/node");
                if Path::new(&c).is_file() {
                    return c;
                }
            }
        }
        "node".into()
    }
    fn res(&self, segs: &[&str]) -> String {
        let mut v = vec![self.cwd.as_str()];
        v.extend_from_slice(segs);
        resolve(&v)
    }
}

/// `path.dirname` for normalized absolute POSIX paths.
fn dirname_of(p: &str) -> String {
    match p.rfind('/') {
        Some(0) | None => "/".into(),
        Some(i) => p[..i].to_string(),
    }
}

fn exists(p: &str) -> bool {
    Path::new(p).exists()
}

fn find_package_root(ctx: &HostContext, start: &str) -> String {
    let mut cur = ctx.res(&[start]);
    loop {
        if exists(&join(&[&cur, "package.json"])) && exists(&join(&[&cur, "scripts", "install-asr.mjs"])) {
            return cur;
        }
        let parent = dirname_of(&cur);
        if parent == cur {
            return ctx.res(&[start]);
        }
        cur = parent;
    }
}

fn realpath(p: &str) -> Option<std::path::PathBuf> {
    std::fs::canonicalize(p).ok()
}

pub fn install_command(ctx: &HostContext) -> String {
    let root = find_package_root(ctx, &ctx.module_dir);
    if let Some(d) = ctx.runtime_root.as_deref().filter(|d| !d.is_empty()) {
        if exists(d) && realpath(d).is_some() && realpath(d) == realpath(&root) {
            let q = |v: &str| format!("'{}'", v.replace('\'', "'\"'\"'"));
            return format!("{} {}", q(&ctx.node_executable), q(&join(&[&root, "scripts", "install-asr.mjs"])));
        }
    }
    format!("npm --prefix \"{root}\" run install:asr")
}

fn funasr_entrypoint(ctx: &HostContext) -> String {
    ctx.res(&[&ctx.module_dir, "../../extension/plugins/builtin/funasr/funasr-local-mcp.mjs"])
}

fn playwright_entrypoint(ctx: &HostContext) -> String {
    let mut c = vec![];
    if let Some(r) = ctx.runtime_root.as_deref().filter(|d| !d.is_empty()) {
        c.push(ctx.res(&[r, "node_modules", "@playwright", "mcp", "cli.js"]));
    }
    c.push(ctx.res(&[&ctx.module_dir, "../../../node_modules/@playwright/mcp/cli.js"]));
    c.push(ctx.res(&[&ctx.module_dir, "../../../../node_modules/@playwright/mcp/cli.js"]));
    c.push(ctx.res(&["node_modules/@playwright/mcp/cli.js"]));
    c.iter().find(|p| exists(p)).cloned().unwrap_or_else(|| c[0].clone())
}

/// TS calls `String.prototype.replaceAll` on command and every arg; a non-string throws a TypeError.
fn patch(spec: &Map<String, Value>, cwd: String, f: &dyn Fn(&str) -> String) -> Result<Value, String> {
    let mut out = spec.clone();
    let cmd = spec.get("command").and_then(Value::as_str).ok_or("TypeError: command is not a string")?;
    out.insert("command".into(), Value::from(f(cmd)));
    out.insert("cwd".into(), Value::from(cwd));
    match spec.get("args") {
        Some(Value::Array(a)) => {
            let mut mapped = vec![];
            for x in a {
                mapped.push(Value::from(f(x.as_str().ok_or("TypeError: arg is not a string")?)));
            }
            out.insert("args".into(), Value::Array(mapped));
        }
        _ => {
            out.remove("args");
        }
    }
    Ok(Value::Object(out))
}

/// `String(x)` for the values that can appear in args.
fn js_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// `patchProjectScopedMcpSpec(spec, projectRoot, pilotHome)`.
pub fn patch_project_scoped_mcp_spec(ctx: &HostContext, spec: &Value, project_root: &str, pilot_home: &str) -> Result<Value, String> {
    let Some(o) = spec.as_object() else { return Ok(spec.clone()) };
    if o.get("transport").and_then(Value::as_str) != Some("stdio") {
        return Ok(spec.clone());
    }
    let id = o.get("id").and_then(Value::as_str).unwrap_or("");
    if id == "browser-use" {
        let pw = playwright_entrypoint(ctx);
        let node = ctx.node_executable.clone();
        return patch(o, ctx.res(&[project_root]), &|v| v.replace(NODE_EXECUTABLE_MARKER, &node).replace(PLAYWRIGHT_MCP_ENTRYPOINT_MARKER, &pw));
    }
    if id != "funasr" {
        return Ok(spec.clone());
    }
    let reps = [
        (PROJECT_ROOT_MARKER, ctx.res(&[project_root])),
        (NODE_EXECUTABLE_MARKER, ctx.node_executable.clone()),
        (FUNASR_MCP_ENTRYPOINT_MARKER, funasr_entrypoint(ctx)),
        (FUNASR_RUNTIME_ROOT_MARKER, join(&[&ctx.res(&[pilot_home]), "funasr"])),
        (FUNASR_INSTALL_COMMAND_MARKER, install_command(ctx)),
    ];
    patch(o, ctx.res(&[project_root]), &|v| reps.iter().fold(v.to_string(), |acc, (m, r)| acc.replace(m, r)))
}

/// Start a resolved stdio MCP spec as a child process (stdin/stdout piped, stderr inherited).
pub fn spawn_stdio(spec: &Value) -> std::io::Result<Child> {
    let cmd = spec.get("command").and_then(Value::as_str).unwrap_or("");
    let mut c = Command::new(cmd);
    if let Some(a) = spec.get("args").and_then(Value::as_array) {
        c.args(a.iter().map(js_string));
    }
    if let Some(d) = spec.get("cwd").and_then(Value::as_str) {
        c.current_dir(d);
    }
    if let Some(env) = spec.get("env").and_then(Value::as_object) {
        for (k, v) in env {
            c.env(k, js_string(v));
        }
    }
    c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
}
