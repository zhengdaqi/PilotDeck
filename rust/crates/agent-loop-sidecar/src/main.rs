//! `pilotdeck-agent-loop-sidecar` (Rust): native port of the AgentLoop sidecar.
//! NOT wired into production: several AgentLoop features are still pending
//! (see docs coverage); production keeps the TS sidecar.
use pilotdeck_agent_loop_sidecar::rt::{Incoming, Rt};
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
    let code = Rt::new(rx, Box::new(std::io::stdout())).serve();
    std::process::exit(code);
}
