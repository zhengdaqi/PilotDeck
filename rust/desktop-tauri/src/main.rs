//! PilotDeck desktop (Tauri v2). Serves the Leptos dist (rust/web-ui/dist).
//! With PILOTDECK_DESKTOP_SMOKE=1 it waits for the web UI self-test, reports it through a
//! blocked navigation to https://smoke.invalid/?<json>, prints `SMOKE <json>` and exits
//! (headless smoke under xvfb).
use tauri::{WebviewUrl, WebviewWindowBuilder};

fn main() {
    let smoke = std::env::var("PILOTDECK_DESKTOP_SMOKE").is_ok();
    tauri::Builder::default()
        .setup(move |app| {
            let w = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("PilotDeck")
                .inner_size(1200.0, 800.0)
                .on_navigation(|url| {
                    if url.host_str() == Some("smoke.invalid") {
                        let q = url.query().unwrap_or("");
                        let decoded = percent_decode(q);
                        println!("SMOKE {decoded}");
                        std::process::exit(0);
                    }
                    true
                })
                .build()?;
            if smoke {
                std::thread::spawn(move || loop {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    let _ = w.eval("(()=>{const s=document.getElementById('selftest');if(s){location.href='https://smoke.invalid/?'+encodeURIComponent(JSON.stringify({selftest:JSON.parse(s.innerText),ua:navigator.userAgent}))}})()");
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri run");
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
