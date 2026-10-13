//! Port of `handleSkillsCommand` / `printSkillMigrationReport` in src/cli/pilotdeck.ts.

use pilotdeck_session_search::paths;
use pilotdeck_skills::jsjson::stringify_pretty;
use pilotdeck_skills::migrate::{migrate, ConflictMode, Options, Report, SourceKind, Status};
use std::process::ExitCode;

fn read_string_flag<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    let i = argv.iter().position(|a| a == flag)?;
    argv.get(i + 1).map(String::as_str)
}

fn read_repeated(argv: &[String], flag: &str) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0..argv.len() {
        if argv[i] != flag {
            continue;
        }
        if let Some(v) = argv.get(i + 1) {
            if !v.is_empty() && !v.starts_with("--") {
                out.push(v.clone());
            }
        }
    }
    out
}

fn parse_sources(value: Option<&str>) -> Result<Option<Vec<SourceKind>>, String> {
    let Some(value) = value.filter(|v| !v.is_empty()) else { return Ok(None) };
    let mut out: Vec<SourceKind> = Vec::new();
    for raw in value.split(',') {
        let n = pilotdeck_session_search::js::js_trim(raw).to_lowercase();
        match n.as_str() {
            "" => continue,
            "cc" | "claude" | "claude-code" => out.push(SourceKind::ClaudeCode),
            "openclaw" => out.push(SourceKind::OpenClaw),
            "hermes" => out.push(SourceKind::Hermes),
            "all" => out.extend([SourceKind::ClaudeCode, SourceKind::OpenClaw, SourceKind::Hermes]),
            _ => return Err(format!("Unknown skills source \"{raw}\". Use cc, openclaw, hermes, or all.")),
        }
    }
    let mut dedup = Vec::new();
    for k in out {
        if !dedup.contains(&k) {
            dedup.push(k);
        }
    }
    Ok(if dedup.is_empty() { None } else { Some(dedup) })
}

fn status_mark(st: Status) -> &'static str {
    match st {
        Status::Migrated => "+",
        Status::WouldMigrate => "?",
        Status::Conflict => "!",
        Status::Error => "x",
        Status::Skipped => "-",
    }
}

fn print_report(r: &Report) {
    println!("PilotDeck skills migration ({})", if r.execute { "EXECUTED" } else { "DRY RUN" });
    println!("Target: {}", r.target_root);
    println!(
        "Summary: migrated={} would_migrate={} conflict={} skipped={} error={}",
        r.count(Status::Migrated),
        r.count(Status::WouldMigrate),
        r.count(Status::Conflict),
        r.count(Status::Skipped),
        r.count(Status::Error)
    );
    let actionable: Vec<_> = r.items.iter().filter(|i| i.status != Status::Skipped).collect();
    if !actionable.is_empty() {
        println!();
        for i in actionable {
            println!("{} {}: {}", status_mark(i.status), i.source_label, if i.slug.is_empty() { "(n/a)" } else { &i.slug });
            println!("  {}", i.source_path);
            if let Some(d) = i.destination_path.as_deref().filter(|d| !d.is_empty()) {
                println!("  -> {d}");
            }
            if !i.reason.is_empty() {
                println!("  {}", i.reason);
            }
        }
    }
    if !r.execute {
        println!();
        println!("This was a dry run. Add --execute to copy skills.");
    }
}

pub fn run_skills_cli(argv: &[String]) -> ExitCode {
    if argv.first().map(String::as_str) != Some("migrate") {
        eprintln!("Usage: pilotdeck skills migrate [--execute] [--from cc,openclaw,hermes] [--source <dir>] [--overwrite|--rename]");
        return ExitCode::from(1);
    }
    let has = |f: &str| argv.iter().any(|a| a == f);
    let include = match parse_sources(read_string_flag(argv, "--from")) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(1);
        }
    };
    let conflict_mode = if has("--overwrite") {
        ConflictMode::Overwrite
    } else if has("--rename") {
        ConflictMode::Rename
    } else {
        ConflictMode::Skip
    };
    let project_root = read_string_flag(argv, "--project").map(str::to_string).unwrap_or_else(paths::cwd);
    let pilot_home = read_string_flag(argv, "--pilot-home")
        .map(str::to_string)
        .unwrap_or_else(paths::resolve_pilot_home_from_env);
    let opts = Options {
        pilot_home,
        project_root: Some(project_root),
        include,
        custom_sources: read_repeated(argv, "--source"),
        execute: has("--execute"),
        conflict_mode,
    };
    let report = match migrate(&opts) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}", e.0);
            return ExitCode::from(1);
        }
    };
    if has("--json") {
        println!("{}", stringify_pretty(&report.to_js()));
        return ExitCode::SUCCESS;
    }
    print_report(&report);
    if report.count(Status::Error) > 0 {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
