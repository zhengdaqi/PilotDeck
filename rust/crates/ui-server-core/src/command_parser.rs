//! Port of `ui/server/utils/commandParser.js`. `parseCommand` (gray-matter frontmatter) and
//! `processBashCommands` (process execution) are not ported yet.
//!
//! Intended differences (fixed source defects, ADR-0004):
//! - SRC-DEFECT-010: values are inserted literally. TS feeds argument values and included file contents to
//!   `String.prototype.replace` as replacement strings, so `$&`, `` $` ``, `$'` and `$$` inside them are expanded.
//!   An argument containing `$2` is also substituted again by the positional pass.
//! - SRC-DEFECT-011: `@file` includes are checked against the real path, so a symlink cannot read files outside the
//!   base. TS checks only the path text.
//! - SRC-DEFECT-012: the whitespace before `@file` (newline or tab) is kept. TS keeps only a leading space.
use crate::node_path::{basename, is_absolute, relative, resolve};
use crate::shell_quote::{parse, Token};
use pilotdeck_web_ui_core::js::trim;
use serde_json::{json, Value};

pub const MAX_INCLUDE_DEPTH: usize = 3;
pub const BASH_COMMAND_ALLOWLIST: [&str; 12] = ["echo", "ls", "pwd", "date", "whoami", "git", "npm", "node", "cat", "grep", "find", "task-master"];

/// `replaceArguments(content, args)`: `$ARGUMENTS` is all args joined with a space, `$1`..`$9` are positional
/// (`$10` is `$1` then `0`, as in TS and bash). Single pass, literal values (SRC-DEFECT-010).
pub fn replace_arguments(content: &str, args: &[String]) -> String {
    let all = args.join(" ");
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(p) = rest.find('$') {
        out.push_str(&rest[..p]);
        let after = &rest[p + 1..];
        if let Some(r) = after.strip_prefix("ARGUMENTS") {
            out.push_str(&all);
            rest = r;
        } else if let Some(d) = after.chars().next().filter(|c| ('1'..='9').contains(c)) {
            out.push_str(args.get(d as usize - '1' as usize).map(String::as_str).unwrap_or(""));
            rest = &after[1..];
        } else {
            out.push('$');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// `isPathSafe(filePath, basePath)` (absolute `basePath`).
pub fn is_path_safe(file_path: &str, base_path: &str) -> bool {
    let resolved = resolve(&[base_path, file_path]);
    let base = resolve(&[base_path]);
    let rel = relative(&base, &resolved);
    !rel.is_empty() && !rel.starts_with("..") && !is_absolute(&rel)
}

/// `validateCommand(commandString)` -> `{ allowed, command, args, error? }`. `Err` is a thrown shell-quote error.
pub fn validate_command(command_string: &str) -> Result<Value, String> {
    let trimmed = trim(command_string);
    let deny = |command: &str, args: &[String], error: String| json!({"allowed": false, "command": command, "args": args, "error": error});
    if trimmed.is_empty() {
        return Ok(deny("", &[], "Empty command".into()));
    }
    let parsed = parse(trimmed)?;
    if parsed.iter().any(|t| matches!(t, Token::Op(_) | Token::Glob(_))) {
        return Ok(deny("", &[], "Shell operators (&&, ||, |, ;, etc.) are not allowed".into()));
    }
    let tokens: Vec<String> = parsed.into_iter().filter_map(|t| if let Token::Str(s) = t { Some(s) } else { None }).collect();
    let Some((command, args)) = tokens.split_first() else {
        return Ok(deny("", &[], "No valid command found".into()));
    };
    let name = basename(command);
    if !BASH_COMMAND_ALLOWLIST.contains(&name.as_str()) {
        return Ok(deny(&name, args, format!("Command '{name}' is not in the allowlist")));
    }
    let dangerous = |a: &str| a.chars().any(|c| ";&|`$()<>{}[]\\".contains(c));
    if let Some(a) = args.iter().find(|a| dangerous(a)) {
        return Ok(deny(&name, args, format!("Argument contains dangerous characters: {a}")));
    }
    Ok(json!({"allowed": true, "command": name, "args": args}))
}

/// `sanitizeOutput(output)`: drops control characters except tab, LF and CR (astral characters are kept, since TS
/// tests their high surrogate).
pub fn sanitize_output(output: &str) -> String {
    output.chars().filter(|&c| {
        let code = c as u32;
        code > 0xffff || code == 9 || code == 10 || code == 13 || (code >= 32 && code != 127)
    }).collect()
}

/// `(?:^|\s)@([^\s]+)` with the `gm` flags (JS `\s`), giving (match start, match end, whitespace prefix, filename).
fn include_matches(content: &str) -> Vec<(usize, usize, String, String)> {
    let is_ws = pilotdeck_web_ui_core::js::is_js_ws;
    let chars: Vec<(usize, char)> = content.char_indices().collect();
    let mut out = vec![];
    let mut k = 0;
    while k < chars.len() {
        let (b, c) = chars[k];
        // Leftmost-first: at a line start the `^` branch wins (the match begins at '@'); otherwise one `\s` char.
        let line_start = b == 0 || matches!(content[..b].chars().last(), Some('\n' | '\r' | '\u{2028}' | '\u{2029}'));
        let (prefix, at_k) = if c == '@' && line_start {
            (String::new(), k)
        } else if is_ws(c) && chars.get(k + 1).is_some_and(|x| x.1 == '@') {
            (c.to_string(), k + 1)
        } else {
            k += 1;
            continue;
        };
        let mut e = at_k + 1;
        while e < chars.len() && !is_ws(chars[e].1) {
            e += 1;
        }
        if e == at_k + 1 {
            k += 1;
            continue;
        }
        let name_start = chars[at_k + 1].0;
        let end = chars.get(e).map_or(content.len(), |x| x.0);
        out.push((b, end, prefix, content[name_start..end].to_string()));
        k = e;
    }
    out
}

/// `processFileIncludes(content, basePath, depth)` with the SRC-DEFECT-010/011/012 fixes. Errors are TS's messages.
pub fn process_file_includes(content: &str, base_path: &str, depth: usize) -> Result<String, String> {
    if content.is_empty() {
        return Ok(String::new());
    }
    if depth >= MAX_INCLUDE_DEPTH {
        return Err(format!("Maximum include depth ({MAX_INCLUDE_DEPTH}) exceeded"));
    }
    let matches = include_matches(content);
    if matches.is_empty() {
        return Ok(content.to_string());
    }
    let real_base = std::fs::canonicalize(resolve(&[base_path])).ok();
    let mut result = content.to_string();
    for (start, end, prefix, filename) in matches {
        let full = &content[start..end];
        if !is_path_safe(&filename, base_path) {
            return Err(format!("Invalid file path (directory traversal detected): {filename}"));
        }
        let file_path = resolve(&[base_path, &filename]);
        let bytes = match std::fs::canonicalize(&file_path) {
            Ok(real) => {
                // SRC-DEFECT-011: the real path must stay below the real base.
                let inside = real_base.as_ref().is_some_and(|b| real.starts_with(b) && real != *b);
                if !inside {
                    return Err(format!("Invalid file path (directory traversal detected): {filename}"));
                }
                match std::fs::read(&real) {
                    Ok(b) => b,
                    Err(e) if e.raw_os_error() == Some(21) => return Err("EISDIR: illegal operation on a directory, read".into()),
                    Err(e) => return Err(e.to_string()),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(format!("File not found: {filename}")),
            Err(e) => return Err(e.to_string()),
        };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let processed = process_file_includes(&text, base_path, depth + 1)?;
        if let Some(p) = result.find(full) {
            // Literal replacement (SRC-DEFECT-010), keeping the matched whitespace (SRC-DEFECT-012).
            result.replace_range(p..p + full.len(), &format!("{prefix}{processed}"));
        }
    }
    Ok(result)
}
pub use crate::frontmatter::parse_command;

/// `processBashCommands(content, { cwd, timeout })`: runs each allowlisted `!command` line without a shell and
/// substitutes its sanitized output (stdout, or stderr when stdout is empty).
/// SRC-DEFECT-010 also covers this function: TS inserts the output through `String.prototype.replace`, so `$&` and
/// the other patterns in command output are expanded. Rust inserts the output literally.
pub fn process_bash_commands(content: &str, cwd: &std::path::Path, timeout: std::time::Duration) -> Result<String, String> {
    if content.is_empty() {
        return Ok(content.to_string());
    }
    // /(?:^|\n)!(.+?)(?=\n|$)/g without the m flag: `.` stops at line terminators, `$` is the end of input.
    let chars: Vec<(usize, char)> = content.char_indices().collect();
    let mut matches: Vec<(usize, usize, String)> = vec![];
    let mut k = 0;
    while k < chars.len() {
        let (b, c) = chars[k];
        let bang = if b == 0 && c == '!' { Some(k) } else if c == '\n' && chars.get(k + 1).is_some_and(|x| x.1 == '!') { Some(k + 1) } else { None };
        let Some(bk) = bang else {
            k += 1;
            continue;
        };
        let mut e = bk + 1;
        while e < chars.len() && !matches!(chars[e].1, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
            e += 1;
        }
        let end_ok = e == chars.len() || chars[e].1 == '\n';
        if e > bk + 1 && end_ok {
            let end = chars.get(e).map_or(content.len(), |x| x.0);
            matches.push((b, end, content[chars[bk + 1].0..end].to_string()));
            k = e;
        } else {
            k += 1;
        }
    }
    let mut result = content.to_string();
    for (start, end, cmd) in matches {
        let full = &content[start..end];
        let command_string = pilotdeck_web_ui_core::js::trim(&cmd).to_string();
        let v = validate_command(&command_string).map_err(|e| e.to_string())?;
        if v["allowed"] != true {
            return Err(format!("Command not allowed: {command_string} - {}", v["error"].as_str().unwrap_or("")));
        }
        let program = v["command"].as_str().unwrap().to_string();
        let args: Vec<String> = v["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect();
        let output = run_exec_file(&program, &args, cwd, timeout).map_err(|e| match e {
            ExecError::Timeout => format!("Command timeout: {command_string}"),
            ExecError::Other(m) => format!("Command failed: {command_string} - {m}"),
        })?;
        let output = sanitize_output(&output);
        if let Some(p) = result.find(full) {
            let rep = if full.starts_with('\n') { format!("\n{output}") } else { output };
            result.replace_range(p..p + full.len(), &rep);
        }
    }
    Ok(result)
}

enum ExecError {
    Timeout,
    Other(String),
}

/// Node `execFile(file, args, { cwd, timeout, maxBuffer: 1 MiB })`: output is `stdout || stderr`; errors carry
/// Node's messages (`spawn <file> ENOENT`, `Command failed: <file> <args>\n<stderr>`).
fn run_exec_file(file: &str, args: &[String], cwd: &std::path::Path, timeout: std::time::Duration) -> Result<String, ExecError> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut child = Command::new(file).args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| {
        let code = match e.kind() {
            std::io::ErrorKind::NotFound => "ENOENT",
            std::io::ErrorKind::PermissionDenied => "EACCES",
            _ => "EIO",
        };
        ExecError::Other(format!("spawn {file} {code}"))
    })?;
    let mut out_pipe = child.stdout.take().unwrap();
    let mut err_pipe = child.stderr.take().unwrap();
    let t_out = std::thread::spawn(move || {
        let mut v = vec![];
        let _ = out_pipe.read_to_end(&mut v);
        v
    });
    let t_err = std::thread::spawn(move || {
        let mut v = vec![];
        let _ = err_pipe.read_to_end(&mut v);
        v
    });
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| ExecError::Other(e.to_string()))? {
            break s;
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ExecError::Timeout);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    let stdout = String::from_utf8_lossy(&t_out.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&t_err.join().unwrap_or_default()).into_owned();
    if stdout.len() > 1024 * 1024 || stderr.len() > 1024 * 1024 {
        return Err(ExecError::Other("stdout maxBuffer length exceeded".into()));
    }
    if !status.success() {
        let cmd = std::iter::once(file.to_string()).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ");
        return Err(ExecError::Other(format!("Command failed: {cmd}\n{stderr}")));
    }
    Ok(if stdout.is_empty() { stderr } else { stdout })
}
