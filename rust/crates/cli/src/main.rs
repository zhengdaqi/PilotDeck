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
        Some("gateway") if argv.get(1).map(String::as_str) != Some("setup") => {
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
