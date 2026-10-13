//! `pilotdeck-agent-loop-sidecar` (Rust): native port of the AgentLoop sidecar.
//! NOT wired into production: the shim delegates only when PILOTDECK_AGENT_LOOP_SIDECAR_EXEC is
//! set; production keeps the TS sidecar until the cutover is approved (see docs coverage).
use pilotdeck_agent_loop_sidecar::rt::{Host, Incoming};
use std::io::BufRead;

fn main() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(Incoming::Line(line)).is_err() {
                return;
            }
        }
        let _ = tx.send(Incoming::Eof);
    });
    let code = Host::new(rx, Box::new(std::io::stdout())).serve();
    std::process::exit(code);
}
