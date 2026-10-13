//! `pilotdeck-gateway-server`: Rust Gateway server edge. NOT wired into production (`pilotdeck server`
//! keeps the TS GatewayServer); the Gateway itself is served by a host process (see backend.rs).
//! Usage: pilotdeck-gateway-server [--host H] [--port N] [--token T] [--allow-remote-host]
//!        [--static-assets DIR] [--server-version V] -- <gateway host argv...>
use pilotdeck_gateway_server::backend::ProcessGateway;
use pilotdeck_gateway_server::http::{start_gateway_server, ServerOptions};
use pilotdeck_gateway_server::{obj, s, stringify_compact, JsValue};
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut o = ServerOptions { host: "127.0.0.1".into(), port: 18789, allow_remote_host: false, token: None, static_assets_path: None, server_version: None };
    let mut i = 0;
    let mut host_argv = Vec::new();
    while i < args.len() {
        let next = |i: usize| args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--host" => { o.host = next(i); i += 1; }
            "--port" => { o.port = next(i).parse().unwrap_or(18789); i += 1; }
            "--token" => { o.token = Some(next(i)); i += 1; }
            "--allow-remote-host" => o.allow_remote_host = true,
            "--static-assets" => { o.static_assets_path = Some(next(i)); i += 1; }
            "--server-version" => { o.server_version = Some(next(i)); i += 1; }
            "--" => { host_argv = args[i + 1..].to_vec(); break; }
            other => { eprintln!("unknown argument: {other}"); std::process::exit(2); }
        }
        i += 1;
    }
    let gateway = match ProcessGateway::spawn(&host_argv) {
        Ok(g) => Arc::new(g),
        Err(e) => { eprintln!("{e}"); std::process::exit(1); }
    };
    match start_gateway_server(o, gateway) {
        Ok(srv) => {
            let mut fields = vec![("url", s(&srv.url)), ("wsUrl", s(&srv.ws_url)), ("token", s(&srv.token))];
            if let Some(p) = &srv.token_path { fields.push(("tokenPath", s(p))); }
            println!("{}", stringify_compact(&obj(fields)));
            let _: JsValue = JsValue::Undefined;
            loop { std::thread::park(); }
        }
        Err(e) => { eprintln!("{e}"); std::process::exit(1); }
    }
}
