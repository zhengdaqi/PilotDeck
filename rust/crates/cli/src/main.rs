//! Rust entry point for `pilotdeck`.
//!
//! Migrated commands run natively. Commands that have not been migrated yet are
//! forwarded verbatim to the TypeScript CLI named by `PILOTDECK_TS_CLI`
//! (a transitional, logic-free process shim tracked in
//! docs/rust-migration/coverage.csv); without it they fail closed.

mod chat_search;
mod skills;
mod cron;
mod update;
mod sdk_cli;

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if !is_forwarded(&argv) {
        announce_env_proxy();
    }
    match argv.first().map(String::as_str) {
        Some("chat") => chat_search::run_chat_search_cli(&argv[1..]),
        Some("cron") => cron::run_cron_cli(&argv[1..]),
        Some("help" | "--help" | "-h") => {
            println!("{}", SDK_HELP);
            ExitCode::SUCCESS
        }
        Some(c @ ("run" | "resume")) => sdk_cli::run_turn_cli(c, &argv[1..]),
        Some(c @ ("sessions" | "settings")) => sdk_cli::run(c, &argv[1..]),
        Some("update") => update::run_update_cli(&argv[1..]),
        Some("gateway") if argv.get(1).map(String::as_str) == Some("setup") => {
            let mut ctx = pilotdeck_setup::gateway_setup::Ctx::from_env();
            match pilotdeck_setup::gateway_setup::run_gateway_setup(&mut ctx, &argv[2..]) {
                Ok(()) | Err(pilotdeck_setup::gateway_setup::Stop::Eof) => ExitCode::SUCCESS,
                Err(pilotdeck_setup::gateway_setup::Stop::Error(m)) => {
                    eprintln!("{m}");
                    ExitCode::from(1)
                }
            }
        }
        Some("gateway") => {
            eprintln!("Usage: pilotdeck gateway setup [feishu|weixin|wecom]");
            ExitCode::from(1)
        }
        Some("tui") if !std::io::IsTerminal::is_terminal(&std::io::stdin()) => {
            eprintln!("pilotdeck tui requires an interactive terminal.");
            ExitCode::from(1)
        }
        Some("skills") => skills::run_skills_cli(&argv[1..]),
        _ => forward_to_typescript(&argv),
    }
}

fn forward_to_typescript(argv: &[String]) -> ExitCode {
    let Ok(entry) = std::env::var("PILOTDECK_TS_CLI") else {
        eprintln!(
            "pilotdeck (rust): command {:?} is not migrated yet; set PILOTDECK_TS_CLI to the TypeScript CLI entry to forward it.",
            argv.first().map(String::as_str).unwrap_or("")
        );
        return ExitCode::from(2);
    };
    let node = std::env::var("PILOTDECK_NODE").unwrap_or_else(|_| "node".to_string());
    match Command::new(node).arg(entry).args(argv).status() {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(err) => {
            eprintln!("pilotdeck (rust): failed to start TypeScript CLI: {err}");
            ExitCode::from(1)
        }
    }
}

/// `src/cli/sdkCli.ts#printHelp` (verbatim).
const SDK_HELP: &str = "PilotDeck SDK CLI

  pilotdeck run <prompt> [options]
  pilotdeck resume <session-id> <prompt> [options]
  pilotdeck sessions list [options]
  pilotdeck sessions messages <session-id> [options]
  pilotdeck settings get [options]
  pilotdeck settings set '<json>' [options]

Options:
  --gateway-url <url>       Gateway WebSocket URL
  --auth-token <token>      Gateway auth token
  --project <path>          Explicit Gateway project key (defaults to Gateway workspace)
  --channel <name>          Session channel (defaults to cli)
  --model <provider/model>  Session model override
  --permission-mode <mode>  Session permission mode
  --json                    Print the final run/result as JSON
  --stream-json             Print each run event as JSONL";

/// Mirrors the top-level `await installGlobalProxy()` in src/cli/pilotdeck.ts:
/// when an env proxy is set, TS logs the dispatcher install to stdout before
/// dispatching any command.
fn announce_env_proxy() {
    let get = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let Some(url) = ["PILOTDECK_PROXY", "https_proxy", "HTTPS_PROXY", "http_proxy", "HTTP_PROXY"]
        .iter()
        .find_map(|k| get(k))
    else {
        return;
    };
    let user = get("no_proxy").or_else(|| get("NO_PROXY"));
    let no_proxy: Vec<String> = user.into_iter().chain(["127.0.0.1".into(), "localhost".into()]).collect();
    println!("[proxy] Global fetch proxy → {url} (noProxy: {})", no_proxy.join(","));
}

fn is_forwarded(argv: &[String]) -> bool {
    match argv.first().map(String::as_str) {
        Some(
            "chat" | "cron" | "help" | "--help" | "-h" | "run" | "resume" | "sessions" | "settings"
            | "update" | "gateway" | "skills",
        ) => false,
        Some("tui") => std::io::IsTerminal::is_terminal(&std::io::stdin()),
        _ => true,
    }
}
