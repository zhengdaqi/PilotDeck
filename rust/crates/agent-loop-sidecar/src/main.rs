//! `pilotdeck-agent-loop-sidecar` (Rust). Control plane only: AgentLoop execution is not
//! migrated yet, so an admitted `execute` is answered with a terminal `result_unknown`-free
//! failure and the process exits non-zero. Not wired into production (CAP-AGENT-LOOP open).
use std::io::{BufRead, Write};

fn main() {
    let mut server = pilotdeck_agent_loop_sidecar::Server::new();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as f64;
        let admitted = server.handle_line(&line, now);
        for l in server.out.drain(..) {
            let _ = writeln!(out, "{l}");
        }
        let _ = out.flush();
        if let Some((_, pilotdeck_agent_loop_sidecar::Admission::Accepted { .. })) = admitted {
            eprintln!("pilotdeck-agent-loop-sidecar (rust): AgentLoop execution is not migrated yet.");
            std::process::exit(70);
        }
    }
}
