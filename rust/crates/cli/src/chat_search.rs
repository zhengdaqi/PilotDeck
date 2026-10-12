//! Port of `runChatSearchCli` in `src/cli/commands/chatSearch.ts`.

use pilotdeck_session_search::format::{format_results, Locale};
use pilotdeck_session_search::js::{js_number, js_trim};
use pilotdeck_session_search::paths;
use pilotdeck_session_search::search::{search, Role, RoleFilter, SearchInput};
use std::io::Write;
use std::process::ExitCode;

const USAGE: &str = "Usage: pilotdeck chat search <keyword> [--project <path>] [--all-projects] [--limit N] [--json] [--regex] [--case-sensitive] [--role user|assistant|all] [--session <id>]";

/// `argv[argv.indexOf(flag) + 1]` (first occurrence; may be missing).
fn read_string_flag<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    let index = argv.iter().position(|a| a == flag)?;
    argv.get(index + 1).map(String::as_str)
}

fn read_number_flag(argv: &[String], flag: &str) -> Option<f64> {
    let value = read_string_flag(argv, flag)?;
    if value.is_empty() {
        return None;
    }
    let n = js_number(value);
    n.is_finite().then_some(n)
}

pub fn run_chat_search_cli(argv: &[String]) -> ExitCode {
    if argv.first().map(String::as_str) != Some("search") {
        eprintln!("{USAGE}");
        return ExitCode::from(1);
    }
    let has = |f: &str| argv.iter().any(|a| a == f);
    let json = has("--json");
    let all_projects = has("--all-projects");
    let project_root = read_string_flag(argv, "--project").map(str::to_string).unwrap_or_else(paths::cwd);
    let limit = read_number_flag(argv, "--limit");
    let regex = has("--regex");
    let case_sensitive = has("--case-sensitive");
    let role = match read_string_flag(argv, "--role") {
        Some("user") => Some(RoleFilter::Only(Role::User)),
        Some("assistant") => Some(RoleFilter::Only(Role::Assistant)),
        Some("all") => Some(RoleFilter::All),
        _ => None,
    };
    let session_id = read_string_flag(argv, "--session").map(str::to_string);

    let rest = &argv[1..];
    let query_parts: Vec<&str> = rest
        .iter()
        .enumerate()
        .filter(|(i, token)| {
            if token.starts_with("--") {
                return false;
            }
            let prev = if *i == 0 { None } else { rest.get(i - 1).map(String::as_str) };
            !matches!(prev, Some("--project" | "--limit" | "--role" | "--session"))
        })
        .map(|(_, t)| t.as_str())
        .collect();
    let query = js_trim(&query_parts.join(" ")).to_string();
    if query.is_empty() {
        eprintln!("Error: search keyword is required.");
        return ExitCode::from(1);
    }

    let pilot_home = read_string_flag(argv, "--pilot-home")
        .map(str::to_string)
        .unwrap_or_else(paths::resolve_pilot_home_from_env);
    let input = SearchInput {
        pilot_home,
        project_root: if all_projects { None } else { Some(project_root) },
        query,
        limit,
        case_sensitive,
        regex,
        role,
        session_id,
        include_internal: false,
    };
    let result = match search(&input) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("{}", err.message);
            return ExitCode::from(1);
        }
    };
    let text = if json {
        let mut buf = Vec::new();
        let fmt = serde_json::ser::PrettyFormatter::with_indent(b"  ");
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
        serde::Serialize::serialize(&result, &mut ser).expect("serialize");
        String::from_utf8(buf).expect("utf8")
    } else {
        format_results(&result, Locale::En, all_projects)
    };
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{text}");
    ExitCode::SUCCESS
}
