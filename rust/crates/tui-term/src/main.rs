//! EXPERIMENTAL Rust TUI: `pilotdeck-tui-app` state machine + a plain crossterm renderer,
//! talking to a running Gateway server over WS. Visual parity with the Ink UI is NOT claimed;
//! behaviour parity of the state machine is covered by the tui-app oracle test.
//!
//! Usage: pilotdeck-tui-rs [--url ws://127.0.0.1:18789/ws] [--project KEY] [--session KEY]
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::{cursor, execute, queue, style, terminal};
use pilotdeck_gateway_client::transport::{Options, StreamItem, Transport};
use pilotdeck_tui_app::{App, AppProps, Key, TuiGateway};
use pilotdeck_tui_core::reducer::TuiMessage;
use pilotdeck_tui_core::sidebar::{flatten_sidebar_rows, group_sessions, session_display_title, SidebarRow};
use pilotdeck_tui_core::width::{slice_columns, string_width};
use serde_json::Value;
use std::io::{stdout, Write};
use std::time::{Duration, Instant};

struct Remote {
    t: Transport,
    stream: Option<String>,
}

impl Remote {
    fn req(&mut self, m: &str, p: Value) -> Result<Value, String> {
        self.t.request_raw(m, &p.to_string())
    }
}

impl TuiGateway for Remote {
    fn list_sessions(&mut self, p: Value) -> Result<Value, String> {
        self.req("list_sessions", p)
    }
    fn new_session(&mut self, p: Value) -> Result<Value, String> {
        self.req("new_session", p)
    }
    fn resume_session(&mut self, p: Value) {
        let _ = self.req("resume_session", p);
    }
    fn read_session_messages(&mut self, p: Value) -> Result<Value, String> {
        self.req("read_session_messages", p)
    }
    /// Starts the stream only; events are pumped from the main loop so keys stay live.
    fn submit_turn(&mut self, p: Value, _on: &mut dyn FnMut(&Value)) -> Result<(), String> {
        self.stream = Some(self.t.stream_start("submit_turn", &p.to_string())?);
        Ok(())
    }
    fn permission_decide(&mut self, p: Value) {
        let _ = self.req("permission_decide", p);
    }
    fn abort_turn(&mut self, p: Value) {
        let _ = self.req("abort_turn", p);
    }
}

fn flag(key: &mut Key, f: fn(&mut Key)) -> String {
    f(key);
    String::new()
}

fn to_key(k: KeyEvent) -> Option<(String, Key)> {
    if k.kind == KeyEventKind::Release {
        return None;
    }
    let mut key = Key {
        ctrl: k.modifiers.contains(KeyModifiers::CONTROL),
        shift: k.modifiers.contains(KeyModifiers::SHIFT),
        meta: k.modifiers.contains(KeyModifiers::ALT),
        ..Default::default()
    };
    let input = match k.code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => {
            key.return_ = true;
            "\r".into()
        }
        KeyCode::Esc => flag(&mut key, |k| k.escape = true),
        KeyCode::Tab => flag(&mut key, |k| k.tab = true),
        KeyCode::BackTab => flag(&mut key, |k| {
            k.tab = true;
            k.shift = true
        }),
        KeyCode::Backspace => flag(&mut key, |k| k.backspace = true),
        KeyCode::Delete => flag(&mut key, |k| k.delete = true),
        KeyCode::Up => flag(&mut key, |k| k.up_arrow = true),
        KeyCode::Down => flag(&mut key, |k| k.down_arrow = true),
        KeyCode::Left => flag(&mut key, |k| k.left_arrow = true),
        KeyCode::Right => flag(&mut key, |k| k.right_arrow = true),
        KeyCode::PageUp => flag(&mut key, |k| k.page_up = true),
        KeyCode::PageDown => flag(&mut key, |k| k.page_down = true),
        _ => return None,
    };
    if input.chars().count() == 1 && input.chars().all(|c| c.is_ascii_uppercase()) {
        key.shift = true;
    }
    Some((input, key))
}

fn fit(s: &str, w: usize) -> String {
    if string_width(s) <= w { s.to_string() } else { slice_columns(s, 0, Some(w)) }
}

fn message_lines(app: &App, width: usize) -> Vec<String> {
    let mut out = vec![];
    for (i, m) in app.state.messages.iter().enumerate() {
        let focused = app.state.focused_index == Some(i as i64);
        let (prefix, text) = match m {
            TuiMessage::User { text } => ("> ", text.clone()),
            TuiMessage::Assistant { text, .. } => ("● ", text.clone()),
            TuiMessage::System { text } => ("· ", text.clone()),
            TuiMessage::Error { text } => ("✗ ", text.clone()),
            TuiMessage::Tool { text, tool_name, expanded, ok, .. } => {
                let mark = if *ok == Some(false) { "✗" } else { "✓" };
                let body = if expanded.unwrap_or(false) { text.clone() } else { text.lines().next().unwrap_or("").to_string() };
                (if focused { "▶ " } else { "  " }, format!("{mark} {} {body}", tool_name.as_deref().unwrap_or("tool")))
            }
        };
        for (j, l) in text.split('\n').enumerate() {
            out.push(fit(&format!("{}{l}", if j == 0 { prefix } else { "  " }), width));
        }
    }
    out
}

fn frame(app: &App, w: usize, h: usize) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    if let Some(c) = &app.state.viewer_content {
        let off = app.viewer_offset.unwrap_or(0).max(0) as usize;
        lines.push(fit(&format!(" {}  j/k PgUp/Dn g/G q:close", app.state.viewer_title), w));
        lines.extend(c.split('\n').skip(off).take(h.saturating_sub(2)).map(|l| fit(l, w)));
        lines.truncate(h);
        return lines;
    }
    let s = &app.state;
    lines.push(fit(&format!("PilotDeck (rust, experimental) · {} · {} · {}", s.mode, s.connection, s.active_session_key), w));
    let side_w = if s.dashboard_mode == "sidebar" { 30.min(w / 3) } else { 0 };
    let body_h = h.saturating_sub(5);
    let body = message_lines(app, w.saturating_sub(side_w + 1));
    let end = body.len().saturating_sub(s.scroll_offset.max(0) as usize);
    let body = &body[end.saturating_sub(body_h)..end];
    let mut side: Vec<String> = vec![];
    if side_w > 0 {
        let rows = flatten_sidebar_rows(&group_sessions(&s.sessions, &s.sidebar_group_by), &s.sidebar_collapsed);
        for (i, r) in rows.iter().enumerate() {
            let cur = if i as i64 == s.sidebar_cursor_index { "›" } else { " " };
            let l = match r {
                SidebarRow::Header { label, count, collapsed, .. } => format!("{cur}{} {label} ({count})", if *collapsed { "▸" } else { "▾" }),
                SidebarRow::Session { session, .. } => format!("{cur}  {}", session_display_title(session, side_w.saturating_sub(4))),
            };
            side.push(fit(&l, side_w));
        }
    }
    for i in 0..body_h {
        let l = side.get(i).cloned().unwrap_or_default();
        let sep = if side_w > 0 { format!("{l}{}│", " ".repeat(side_w.saturating_sub(string_width(&l)))) } else { String::new() };
        lines.push(format!("{sep}{}", body.get(i).cloned().unwrap_or_default()));
    }
    lines.push(if s.help_open {
        fit(&format!("[help] tab={} (1 shortcuts · 2 settings · 3 about · q close) settings cursor={}", s.help_tab, s.settings_cursor), w)
    } else if let Some(p) = s.pending_permissions.first() {
        fit(&format!("Permission: {} {} — y allow · a always · n deny · esc abort ({} queued)", p.tool_name, p.payload, s.pending_permissions.len()), w)
    } else {
        String::new()
    });
    let act: Vec<&str> = s.activity.iter().map(|a| a.text.as_str()).collect();
    lines.push(fit(&format!("{}{}", if s.is_running { "… " } else { "" }, act.join(" · ")), w));
    lines.push(fit(&format!("> {}", s.input), w));
    lines.truncate(h);
    lines
}

fn draw(lines: &[String]) -> std::io::Result<()> {
    let mut o = stdout();
    queue!(o, terminal::Clear(terminal::ClearType::All))?;
    for (i, l) in lines.iter().enumerate() {
        queue!(o, cursor::MoveTo(0, i as u16), style::Print(l))?;
    }
    o.flush()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).cloned();
    let url = arg("--url").unwrap_or_else(|| format!("ws://{}/ws", pilotdeck_gateway_client::DEFAULT_URL_HOST));
    let token = pilotdeck_gateway_client::read_token().unwrap_or_default();
    let opts = Options { url: &url, token: &token, client_name: "tui-rs", client_version: "0.1.0", connect_timeout: Duration::from_secs(5), request_timeout: Duration::from_secs(30) };
    let t = match Transport::connect(&opts) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("pilotdeck-tui-rs: {e}");
            std::process::exit(1);
        }
    };
    let mut gw = Remote { t, stream: None };
    let (w, h) = terminal::size().ok().filter(|(c, r)| *c > 0 && *r > 0).unwrap_or((100, 30));
    let props = AppProps {
        connection: "remote".into(),
        project_key: arg("--project"),
        session_key: arg("--session"),
        cwd: std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default(),
        pilot_home: pilotdeck_session_search::paths::resolve_pilot_home_from_env().into(),
        columns: w,
        rows: h,
    };
    let mut app = App::new(props, &mut gw);
    let _ = terminal::enable_raw_mode();
    let _ = execute!(stdout(), terminal::EnterAlternateScreen, cursor::Hide);
    let mut last: Vec<String> = vec![];
    while !app.exited {
        let f = frame(&app, app.props.columns as usize, app.props.rows as usize);
        if f != last {
            let _ = draw(&f);
            last = f;
        }
        if let Some(id) = gw.stream.clone() {
            match gw.t.stream_poll(&id, Instant::now() + Duration::from_millis(30)) {
                Ok(StreamItem::Event(e)) => app.apply_event(&e),
                Ok(StreamItem::End) => gw.stream = None,
                Ok(StreamItem::Interrupted) => {}
                Err(e) => {
                    gw.stream = None;
                    app.turn_failed(e);
                }
            }
        }
        let wait = if gw.stream.is_some() { 0 } else { 50 };
        if !event::poll(Duration::from_millis(wait)).unwrap_or(false) {
            continue;
        }
        match event::read() {
            // Ink's exitOnCtrlC: exit before any handler runs.
            Ok(Event::Key(k)) if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) => app.ctrl_c(),
            Ok(Event::Key(k)) => {
                if let Some((input, key)) = to_key(k) {
                    app.handle(&mut gw, &input, &key);
                }
            }
            Ok(Event::Resize(c, r)) => {
                app.props.columns = c;
                app.props.rows = r;
            }
            Ok(Event::Paste(p)) => app.handle(&mut gw, &p, &Key::default()),
            _ => {}
        }
    }
    let _ = execute!(stdout(), cursor::Show, terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
    for s in &app.scrollback {
        print!("{s}");
    }
}
