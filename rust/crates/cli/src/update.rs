//! Port of `handleUpdateCommand` + `src/runtime/updateCommand.ts#runUpdateProcess`.
//! The updater itself stays the bundled `scripts/update-web.mjs` asset (run by Node), exactly as TS spawns it.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const NO_UPDATER: &str =
    "This installation does not include the Git Web updater. Use application settings or update manually.";

pub fn find_update_root(start: &Path) -> Result<PathBuf, String> {
    let mut current = start.to_path_buf();
    loop {
        if current.join("scripts/update-web.mjs").exists() {
            return Ok(current);
        }
        match current.parent() {
            Some(p) => current = p.to_path_buf(),
            None => return Err(NO_UPDATER.to_string()),
        }
    }
}

fn start_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("PILOTDECK_INSTALL_ROOT") {
        return PathBuf::from(dir);
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn run_update_cli(args: &[String]) -> ExitCode {
    match run(args) {
        Ok(code) if code == 0 || code == 2 => ExitCode::SUCCESS,
        Ok(_) => ExitCode::from(1),
        Err(message) => {
            eprintln!("Update failed: {message}");
            ExitCode::from(1)
        }
    }
}

fn run(args: &[String]) -> Result<i32, String> {
    let root = find_update_root(&start_dir())?;
    let node = std::env::var("PILOTDECK_NODE").unwrap_or_else(|_| "node".to_string());
    let mut child = Command::new(&node)
        .arg(root.join("scripts/update-web.mjs"))
        .args(args)
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn {node} {}", if e.kind() == std::io::ErrorKind::NotFound { "ENOENT".to_string() } else { e.to_string() }))?;
    // TS forwards both child streams to process.stdout as chunks arrive.
    let mut err = child.stderr.take().unwrap();
    let errthread = std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = err.read(&mut buf) {
            if n == 0 { break; }
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(&buf[..n]);
            let _ = out.flush();
        }
    });
    let mut out = child.stdout.take().unwrap();
    let mut buf = [0u8; 8192];
    while let Ok(n) = out.read(&mut buf) {
        if n == 0 { break; }
        let mut o = std::io::stdout().lock();
        let _ = o.write_all(&buf[..n]);
        let _ = o.flush();
    }
    let _ = errthread.join();
    let status = child.wait().map_err(|e| e.to_string())?;
    Ok(status.code().unwrap_or(1))
}
