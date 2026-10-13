//! Interactive TUI state machine: a Rust port of `TuiApp.tsx` (input handling, commands,
//! sidebar, permission prompt, help/settings, tool-output viewer) on top of `pilotdeck-tui-core`.
//!
//! Verified by a differential oracle that drives the real Ink app (see
//! `rust/parity/tui/gen_tui_app_fixture.mjs`). Rendering is not part of this crate's contract.
pub mod settings;
pub mod text_input;

use pilotdeck_tui_core::reducer::{self, ActivityItem, PendingPermission, ReducerState, TuiMessage};
use pilotdeck_tui_core::sidebar::{compute_smart_collapse, flatten_sidebar_rows, group_sessions, SidebarRow};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use settings::{js_trim, read_permission_settings, write_permission_settings, SettingsPatch};
use std::collections::BTreeSet;
use std::path::PathBuf;
use text_input::{utf16_len, TextInputOutcome, TextInputState};

pub const MIN_SIDEBAR_COLS: u16 = 70;

/// Ink's `Key` (only the flags TuiApp / TextInput / the viewer read).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Key {
    pub up_arrow: bool,
    pub down_arrow: bool,
    pub left_arrow: bool,
    pub right_arrow: bool,
    pub page_down: bool,
    pub page_up: bool,
    #[serde(rename = "return")]
    pub return_: bool,
    pub escape: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub tab: bool,
    pub backspace: bool,
    pub delete: bool,
    pub meta: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppState {
    pub connection: String,
    #[serde(rename = "activeSessionKey")]
    pub active_session_key: String,
    pub sessions: Vec<Value>,
    pub messages: Vec<TuiMessage>,
    pub activity: Vec<ActivityItem>,
    pub input: String,
    pub mode: String,
    #[serde(rename = "isRunning")]
    pub is_running: bool,
    #[serde(rename = "helpOpen")]
    pub help_open: bool,
    #[serde(rename = "helpTab")]
    pub help_tab: String,
    #[serde(rename = "settingsCursor")]
    pub settings_cursor: i64,
    #[serde(rename = "scrollOffset")]
    pub scroll_offset: i64,
    #[serde(rename = "focusedIndex")]
    pub focused_index: Option<i64>,
    #[serde(rename = "viewerContent")]
    pub viewer_content: Option<String>,
    #[serde(rename = "viewerTitle")]
    pub viewer_title: String,
    #[serde(rename = "pendingPermissions")]
    pub pending_permissions: Vec<PendingPermission>,
    #[serde(rename = "dashboardMode")]
    pub dashboard_mode: String,
    #[serde(rename = "sidebarCursorIndex")]
    pub sidebar_cursor_index: i64,
    #[serde(rename = "sidebarCollapsed")]
    pub sidebar_collapsed: BTreeSet<String>,
    #[serde(rename = "sidebarGroupBy")]
    pub sidebar_group_by: String,
}

/// The subset of the `Gateway` interface TuiApp uses. Params are the exact JS objects
/// (undefined fields omitted). Errors carry `error.message`.
pub trait TuiGateway {
    fn list_sessions(&mut self, params: Value) -> Result<Value, String>;
    fn new_session(&mut self, params: Value) -> Result<Value, String>;
    fn resume_session(&mut self, params: Value);
    fn read_session_messages(&mut self, params: Value) -> Result<Value, String>;
    fn submit_turn(&mut self, params: Value, on_event: &mut dyn FnMut(&Value)) -> Result<(), String>;
    fn permission_decide(&mut self, params: Value);
    fn abort_turn(&mut self, params: Value);
}

#[derive(Debug, Clone)]
pub struct AppProps {
    pub connection: String,
    pub project_key: Option<String>,
    pub session_key: Option<String>,
    /// Used for `defaultTuiSessionKey(projectKey = process.cwd())`.
    pub cwd: String,
    pub pilot_home: PathBuf,
    pub columns: u16,
    pub rows: u16,
}

pub struct App {
    pub props: AppProps,
    pub state: AppState,
    /// `None` while the ToolOutputViewer replaces the tree (PromptInput unmounted).
    pub text_input: Option<TextInputState>,
    /// Ink listener order: true while TextInput's `useInput` is registered before TuiApp's.
    text_input_first: bool,
    pub viewer_offset: Option<i64>,
    pub exited: bool,
    /// Text TuiApp wrote straight to stdout (short tool outputs).
    pub scrollback: Vec<String>,
    batch: Option<AppState>,
    /// Bumped whenever PromptInput remounts.
    ti_generation: u64,
}

fn obj(pairs: Vec<(&str, Option<Value>)>) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        if let Some(v) = v {
            m.insert(k.to_string(), v);
        }
    }
    Value::Object(m)
}

fn split_ws(s: &str) -> Vec<&str> {
    // JS `str.split(/\s+/)` (keeps leading/trailing empty fields).
    let mut out = vec![];
    let mut start = 0;
    let mut in_ws = false;
    let mut ws_start = 0;
    for (i, c) in s.char_indices() {
        if settings::is_js_ws(c) {
            if !in_ws {
                in_ws = true;
                ws_start = i;
            }
        } else if in_ws {
            out.push(&s[start..ws_start]);
            start = i;
            in_ws = false;
        }
    }
    if in_ws {
        out.push(&s[start..ws_start]);
        out.push("");
    } else {
        out.push(&s[start..]);
    }
    out
}

/// JS `parseInt(s, 10)`; `None` for NaN.
pub fn js_parse_int(s: &str) -> Option<f64> {
    let t = s.trim_start_matches(settings::is_js_ws);
    let (neg, rest) = match t.chars().next() {
        Some('-') => (true, &t[1..]),
        Some('+') => (false, &t[1..]),
        _ => (false, t),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let v: f64 = digits.parse().ok()?;
    Some(if neg { -v } else { v })
}

pub fn build_permission_entry(tool_name: &str, payload: &Value) -> String {
    if tool_name != "bash" {
        return tool_name.to_string();
    }
    let command = payload.get("command").and_then(Value::as_str).map(js_trim).unwrap_or("");
    if command.is_empty() {
        return "bash".into();
    }
    let tokens = split_ws(command);
    if tokens[0] == "git" && tokens.get(1).is_some_and(|t| !t.is_empty()) {
        return format!("bash:{} {}:*", tokens[0], tokens[1]);
    }
    format!("bash:{}:*", tokens[0])
}

fn session_key_of(s: &Value) -> String {
    s.get("sessionKey").and_then(Value::as_str).or_else(|| s.get("sessionId").and_then(Value::as_str)).unwrap_or("").to_string()
}
fn summary_of(s: &Value) -> Option<String> {
    s.get("summary").and_then(Value::as_str).map(str::to_string)
}
fn line_count(m: &TuiMessage) -> f64 {
    match m {
        TuiMessage::Tool { line_count: Some(v), .. } => v.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}
fn system(t: impl Into<String>) -> TuiMessage {
    TuiMessage::System { text: t.into() }
}
fn error(t: impl Into<String>) -> TuiMessage {
    TuiMessage::Error { text: t.into() }
}

/// Did `applyGatewayEventToTuiState` produce a new `messages` array (reference inequality)?
fn event_replaces_messages(e: &Value) -> bool {
    let nonempty = |k: &str| e.get(k).and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    match e.get("type").and_then(Value::as_str).unwrap_or("") {
        "assistant_text_delta" | "assistant_thinking_delta" => nonempty("text"),
        "tool_call_finished" | "structured_output" | "error" | "tool_result_detail_available" => true,
        _ => false,
    }
}

impl App {
    /// `useState` initialiser + the mount `listSessions` effect.
    pub fn new(props: AppProps, gw: &mut dyn TuiGateway) -> Self {
        let perm = read_permission_settings(&props.pilot_home);
        let pk = props.project_key.clone().unwrap_or_else(|| props.cwd.clone());
        let initial = props.session_key.clone().unwrap_or_else(|| pilotdeck_tui_core::default_tui_session_key(&pk));
        let state = AppState {
            connection: props.connection.clone(),
            active_session_key: initial,
            sessions: vec![],
            messages: vec![],
            activity: vec![],
            input: String::new(),
            mode: if perm.skip_permissions { "bypassPermissions" } else { "default" }.into(),
            is_running: false,
            help_open: false,
            help_tab: "shortcuts".into(),
            settings_cursor: 0,
            scroll_offset: 0,
            focused_index: None,
            viewer_content: None,
            viewer_title: String::new(),
            pending_permissions: vec![],
            dashboard_mode: "closed".into(),
            sidebar_cursor_index: 0,
            sidebar_collapsed: BTreeSet::new(),
            sidebar_group_by: "project".into(),
        };
        let mut app = App { props, state, text_input: None, text_input_first: true, viewer_offset: None, exited: false, scrollback: vec![], batch: None, ti_generation: 0 };
        app.text_input = Some(TextInputState::mount(""));
        if let Ok(r) = gw.list_sessions(obj(vec![("projectKey", app.pk()), ("limit", Some(json!(8)))])) {
            app.state.sessions = r.get("sessions").and_then(Value::as_array).cloned().unwrap_or_default();
        }
        app
    }

    fn pk(&self) -> Option<Value> {
        self.props.project_key.clone().map(Value::from)
    }
    fn rows(&self) -> i64 {
        (self.props.rows as i64 - 7).max(10)
    }
    fn scroll_page(&self) -> i64 {
        (self.rows() / 2).max(1)
    }
    fn focus(s: &AppState) -> bool {
        !s.help_open && s.pending_permissions.is_empty()
    }

    /// Commit bookkeeping: TextInput mount/unmount, focus-driven re-registration and its clamp effect.
    fn commit(&mut self, prev: &AppState) {
        let viewer_now = self.state.viewer_content.is_some();
        let viewer_before = prev.viewer_content.is_some();
        if viewer_now && !viewer_before {
            self.text_input = None;
            self.viewer_offset = Some(0);
            return;
        }
        if !viewer_now && viewer_before {
            self.viewer_offset = None;
            let mut ti = TextInputState::mount(&self.state.input);
            ti.effect(&self.state.input, Self::focus(&self.state));
            self.text_input = Some(ti);
            self.text_input_first = false;
            self.ti_generation += 1;
            return;
        }
        if viewer_now {
            if prev.viewer_content != self.state.viewer_content {
                self.viewer_offset = Some(0);
            }
            return;
        }
        // Opening/closing the sidebar swaps the root element (row Box vs chat Box), so React
        // remounts PromptInput: TextInput state resets and its listener re-registers last.
        if (prev.dashboard_mode == "sidebar") != (self.state.dashboard_mode == "sidebar") {
            let mut ti = TextInputState::mount(&self.state.input);
            ti.effect(&self.state.input, Self::focus(&self.state));
            self.text_input = Some(ti);
            self.text_input_first = false;
            self.ti_generation += 1;
            return;
        }
        let (f0, f1) = (Self::focus(prev), Self::focus(&self.state));
        if !f0 && f1 {
            self.text_input_first = false;
        }
        if let Some(ti) = self.text_input.as_mut() {
            if prev.input != self.state.input || f0 != f1 {
                ti.effect(&self.state.input, f1);
            }
        }
    }

    fn set(&mut self, f: impl FnOnce(&mut AppState)) {
        if self.batch.is_some() {
            f(&mut self.state);
            return;
        }
        let prev = self.state.clone();
        f(&mut self.state);
        self.commit(&prev);
    }

    /// Updates queued synchronously inside one Ink listener commit together.
    fn begin_batch(&mut self) {
        self.batch = Some(self.state.clone());
    }
    fn end_batch(&mut self) {
        if let Some(prev) = self.batch.take() {
            self.commit(&prev);
        }
    }

    /// Ink exits on Ctrl+C itself (`exitOnCtrlC` default) before any `useInput` handler runs.
    pub fn ctrl_c(&mut self) {
        self.exited = true;
    }

    /// One Ink `input` event, run to quiescence.
    pub fn handle(&mut self, gw: &mut dyn TuiGateway, input: &str, key: &Key) {
        if self.exited {
            return;
        }
        if self.state.viewer_content.is_some() {
            self.viewer_key(input, key);
            return;
        }
        let snap = self.state.clone();
        let ti_active = Self::focus(&snap) && self.text_input.is_some();
        let mut on_change: Option<String> = None;
        let mut submit: Option<String> = None;
        let mut suppress = false;
        let mut deferred: Vec<Deferred> = vec![];
        // Ink snapshots its listener list before emitting, so a TextInput that gets remounted by an
        // earlier listener still handles this event on its old (discarded) instance.
        let gen0 = self.ti_generation;
        let ti0 = self.text_input;
        let order = if self.text_input_first { [true, false] } else { [false, true] };
        for is_ti in order {
            self.begin_batch();
            if is_ti {
                if ti_active {
                    let mut ti = ti0.unwrap();
                    match ti.handle(&snap.input, input, key) {
                        TextInputOutcome::Submit(v) => submit = Some(v),
                        TextInputOutcome::Edited { changed } => on_change = changed,
                        TextInputOutcome::Ignored => {}
                    }
                    if self.ti_generation == gen0 {
                        self.text_input = Some(ti);
                    }
                    if let Some(v) = submit.take() {
                        self.submit(gw, &snap, &v, &mut deferred);
                    }
                }
            } else {
                suppress = self.app_key(gw, &snap, input, key, &mut deferred);
            }
            self.end_batch();
        }
        // Microtask: handleInputChange.
        if let Some(next) = on_change {
            if !suppress {
                self.set(|s| {
                    s.input = next;
                    s.focused_index = None;
                });
            }
        }
        for d in deferred {
            self.run_deferred(gw, &snap, d);
        }
    }

    fn viewer_key(&mut self, input: &str, key: &Key) {
        let content = self.state.viewer_content.clone().unwrap_or_default();
        let viewport = (self.props.rows as i64 - 4).max(5);
        let n = content.split('\n').count() as i64;
        let max = (n - viewport).max(0);
        let o = self.viewer_offset.unwrap_or(0);
        let next = if input == "q" || key.escape {
            self.set(|s| {
                s.viewer_content = None;
                s.viewer_title = String::new();
            });
            return;
        } else if input == "j" || key.down_arrow {
            max.min(o + 1)
        } else if input == "k" || key.up_arrow {
            (o - 1).max(0)
        } else if key.page_down {
            max.min(o + viewport)
        } else if key.page_up {
            (o - viewport).max(0)
        } else if input == "G" {
            max
        } else if input == "g" {
            0
        } else {
            o
        };
        self.viewer_offset = Some(next);
    }

    fn decide(&self, gw: &mut dyn TuiGateway, snap: &AppState, extra: Vec<(&str, Option<Value>)>) {
        let mut p = vec![("sessionKey", Some(Value::from(snap.active_session_key.clone())))];
        p.extend(extra);
        gw.permission_decide(obj(p));
    }

    /// TuiApp's `useInput` handler. Returns whether the TextInput change is suppressed.
    fn app_key(&mut self, gw: &mut dyn TuiGateway, snap: &AppState, input: &str, key: &Key, deferred: &mut Vec<Deferred>) -> bool {
        let sk = || obj(vec![("sessionKey", Some(Value::from(snap.active_session_key.clone())))]);
        if let Some(front) = snap.pending_permissions.first() {
            let rid = Some(Value::from(front.request_id.clone()));
            let dequeue = |s: &mut AppState| {
                if !s.pending_permissions.is_empty() {
                    s.pending_permissions.remove(0);
                }
            };
            match input {
                "y" => {
                    self.decide(gw, snap, vec![("requestId", rid), ("decision", Some("allow".into())), ("remember", Some(false.into()))]);
                    self.set(dequeue);
                }
                "a" => {
                    self.decide(gw, snap, vec![("requestId", rid), ("decision", Some("allow".into())), ("remember", Some(true.into()))]);
                    let entry = build_permission_entry(&front.tool_name, &front.payload);
                    let cur = read_permission_settings(&self.props.pilot_home);
                    if !cur.allowed_tools.contains(&entry) {
                        let mut a = cur.allowed_tools.clone();
                        a.push(entry);
                        write_permission_settings(&self.props.pilot_home, SettingsPatch { allowed_tools: Some(a), ..Default::default() });
                    }
                    self.set(dequeue);
                }
                "n" => {
                    self.decide(gw, snap, vec![("requestId", rid), ("decision", Some("deny".into())), ("reason", Some("User denied in TUI".into()))]);
                    self.set(dequeue);
                }
                _ if key.escape => {
                    gw.abort_turn(sk());
                    self.set(|s| s.pending_permissions.clear());
                }
                _ => {}
            }
            return false;
        }

        if snap.help_open {
            const TABS: [&str; 3] = ["shortcuts", "settings", "about"];
            let idx = |t: &str| TABS.iter().position(|x| *x == t).map(|i| i as i64).unwrap_or(-1);
            if key.right_arrow || (key.tab && !key.shift) {
                if snap.help_tab != "settings" {
                    self.set(|s| s.help_tab = TABS[((idx(&s.help_tab) + 1).rem_euclid(3)) as usize].into());
                }
                return false;
            }
            if key.left_arrow || (key.shift && key.tab) {
                if snap.help_tab != "settings" {
                    self.set(|s| s.help_tab = TABS[((idx(&s.help_tab) - 1 + 3).rem_euclid(3)) as usize].into());
                }
                return false;
            }
            if snap.help_tab == "settings" {
                if key.up_arrow {
                    self.set(|s| s.settings_cursor = (s.settings_cursor - 1).max(0));
                    return false;
                }
                if key.down_arrow {
                    self.set(|s| s.settings_cursor = (s.settings_cursor + 1).min(2));
                    return false;
                }
                if key.return_ || input == " " {
                    if self.state.settings_cursor == 0 {
                        const MODES: [&str; 3] = ["default", "plan", "bypassPermissions"];
                        let i = MODES.iter().position(|m| *m == self.state.mode).map(|i| i as i64).unwrap_or(-1);
                        let next = MODES[((i + 1).rem_euclid(3)) as usize];
                        write_permission_settings(&self.props.pilot_home, SettingsPatch { skip_permissions: Some(next == "bypassPermissions"), ..Default::default() });
                        self.set(|s| s.mode = next.into());
                    }
                    return false;
                }
            }
            match input {
                "1" => self.set(|s| s.help_tab = "shortcuts".into()),
                "2" => self.set(|s| {
                    s.help_tab = "settings".into();
                    s.settings_cursor = 0;
                }),
                "3" => self.set(|s| s.help_tab = "about".into()),
                _ if key.escape || input == "q" => self.set(|s| s.help_open = false),
                _ => {}
            }
            return false;
        }

        if snap.dashboard_mode == "sidebar" {
            let groups = group_sessions(&snap.sessions, &snap.sidebar_group_by);
            let rows = flatten_sidebar_rows(&groups, &snap.sidebar_collapsed);
            let row = usize::try_from(snap.sidebar_cursor_index).ok().and_then(|i| rows.get(i)).cloned();
            if key.escape || (key.ctrl && input == "c") {
                self.set(|s| s.dashboard_mode = "closed".into());
                return false;
            }
            if key.ctrl && input == "e" {
                self.set(|s| s.dashboard_mode = "closed".into());
                return true;
            }
            if key.ctrl && input == "s" {
                self.set(|s| {
                    let next = if s.sidebar_group_by == "project" { "status" } else { "project" };
                    let g = group_sessions(&s.sessions, next);
                    s.sidebar_group_by = next.into();
                    s.sidebar_cursor_index = 0;
                    s.sidebar_collapsed = compute_smart_collapse(&g, &s.active_session_key);
                });
                return true;
            }
            let empty = snap.input.is_empty();
            if key.up_arrow && empty {
                self.set(|s| s.sidebar_cursor_index = (s.sidebar_cursor_index - 1).max(0));
                return false;
            }
            if key.down_arrow && empty {
                let n = rows.len() as i64;
                self.set(|s| s.sidebar_cursor_index = (n - 1).min(s.sidebar_cursor_index + 1));
                return false;
            }
            if key.return_ && js_trim(&snap.input).is_empty() {
                match row {
                    Some(SidebarRow::Header { group_key, .. }) => self.set(|s| {
                        if !s.sidebar_collapsed.remove(&group_key) {
                            s.sidebar_collapsed.insert(group_key);
                        }
                    }),
                    Some(SidebarRow::Session { session, .. }) => {
                        self.switch_start(gw, &session_key_of(&session), summary_of(&session), deferred)
                    }
                    None => {}
                }
                return false;
            }
            if key.right_arrow && empty {
                match row {
                    Some(SidebarRow::Header { group_key, collapsed: true, .. }) => self.set(|s| {
                        s.sidebar_collapsed.remove(&group_key);
                    }),
                    Some(SidebarRow::Session { session, .. }) => {
                        self.switch_start(gw, &session_key_of(&session), summary_of(&session), deferred)
                    }
                    _ => {}
                }
                return false;
            }
            if key.left_arrow && empty {
                match row {
                    Some(SidebarRow::Header { group_key, collapsed: false, .. }) => self.set(|s| {
                        s.sidebar_collapsed.insert(group_key);
                    }),
                    Some(SidebarRow::Session { group_key, .. }) => {
                        if let Some(h) = rows.iter().position(|r| matches!(r, SidebarRow::Header { group_key: g, .. } if *g == group_key)) {
                            self.set(|s| s.sidebar_cursor_index = h as i64);
                        }
                    }
                    _ => {}
                }
                return false;
            }
        }

        if key.ctrl && input == "c" {
            if snap.is_running {
                gw.abort_turn(sk());
            } else {
                self.exited = true;
            }
            return false;
        }
        if key.ctrl && input == "e" {
            deferred.push(Deferred::OpenSidebar);
            return true;
        }
        if key.escape {
            if snap.is_running {
                gw.abort_turn(sk());
            } else if snap.help_open {
                self.set(|s| s.help_open = false);
            } else if snap.scroll_offset > 0 || snap.focused_index.is_some() {
                self.set(|s| {
                    s.scroll_offset = 0;
                    s.focused_index = None;
                });
            }
            return false;
        }
        if input == "?" && snap.input.is_empty() {
            self.set(|s| s.help_open = !s.help_open);
            return false;
        }
        if key.tab && snap.input.is_empty() {
            let shift = key.shift;
            self.set(|s| {
                let ti: Vec<i64> = s.messages.iter().enumerate().filter(|(_, m)| matches!(m, TuiMessage::Tool { .. })).map(|(i, _)| i as i64).collect();
                if ti.is_empty() {
                    return;
                }
                let last = *ti.last().unwrap();
                s.focused_index = Some(match s.focused_index {
                    None => if shift { last } else { ti[0] },
                    Some(f) => {
                        let pos = ti.iter().position(|x| *x == f).map(|p| p as i64).unwrap_or(-1);
                        if shift {
                            if pos <= 0 { last } else { ti[(pos - 1) as usize] }
                        } else if pos >= ti.len() as i64 - 1 {
                            ti[0]
                        } else {
                            ti[(pos + 1) as usize]
                        }
                    }
                });
            });
            return false;
        }
        let page = self.scroll_page();
        if key.page_up || (key.shift && key.up_arrow) {
            self.set(|s| {
                let max = (s.messages.len() as i64 - 1).max(0);
                s.scroll_offset = max.min(s.scroll_offset + page);
            });
            return false;
        }
        if key.page_down || (key.shift && key.down_arrow) {
            self.set(|s| s.scroll_offset = (s.scroll_offset - page).max(0));
        }
        false
    }

    /// `switchToSession` synchronous prefix; the message load is deferred.
    fn switch_start(&mut self, gw: &mut dyn TuiGateway, key: &str, summary: Option<String>, deferred: &mut Vec<Deferred>) {
        gw.resume_session(json!({ "sessionKey": key }));
        let label = summary.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| key.to_string());
        let k = key.to_string();
        self.set(|s| {
            s.active_session_key = k;
            s.dashboard_mode = "closed".into();
            s.messages = vec![system(format!("Loading session: {label}\u{2026}"))];
            s.scroll_offset = 0;
            s.focused_index = None;
        });
        deferred.push(Deferred::LoadSession { key: key.to_string(), label });
    }

    fn load_session(&mut self, gw: &mut dyn TuiGateway, key: &str, label: &str) {
        let r = gw.read_session_messages(obj(vec![
            ("sessionKey", Some(key.into())),
            ("projectKey", self.pk()),
            ("limit", Some(json!(50))),
            ("direction", Some("backward".into())),
        ]));
        match r {
            Ok(v) => {
                let mut out: Vec<TuiMessage> = vec![];
                for m in v.get("messages").and_then(Value::as_array).cloned().unwrap_or_default() {
                    let g = |k: &str| m.get(k).and_then(Value::as_str);
                    let text = g("text").filter(|t| !t.is_empty());
                    let (role, kind) = (g("role").unwrap_or(""), g("kind").unwrap_or(""));
                    if let (true, Some(t)) = (role == "user" && kind == "text", text) {
                        out.push(TuiMessage::User { text: t.into() });
                    } else if let (true, Some(t)) = (role == "assistant" && kind == "text", text) {
                        out.push(TuiMessage::Assistant { text: t.into(), thinking: None });
                    } else if role == "assistant" && kind == "thinking" && text.is_some() {
                        if let Some(TuiMessage::Assistant { thinking, .. }) = out.last_mut() {
                            *thinking = Some(thinking.clone().unwrap_or_default() + text.unwrap());
                        }
                    } else if role == "tool" && kind == "tool_result" {
                        let ok = m.get("ok").and_then(Value::as_bool);
                        let t = match m.get("text") {
                            Some(Value::String(s)) => s.clone(),
                            Some(v) if !v.is_null() => v.to_string(),
                            _ => if settings::js_truthy(m.get("ok")) { "ok".into() } else { "error".into() },
                        };
                        out.push(TuiMessage::Tool {
                            text: t,
                            ok,
                            tool_call_id: g("toolCallId").map(str::to_string),
                            tool_name: g("toolName").map(str::to_string),
                            line_count: None,
                            result_path: None,
                            full_text: None,
                            expanded: None,
                        });
                    } else if role == "error" {
                        out.push(error(match m.get("text") {
                            Some(Value::String(s)) => s.clone(),
                            _ => "error".into(),
                        }));
                    }
                }
                let msgs = if out.is_empty() { vec![system(format!("Session: {label}"))] } else { out };
                self.set(|s| s.messages = msgs);
            }
            Err(_) => self.set(|s| s.messages = vec![system(format!("Switched to session: {label}"))]),
        }
    }

    fn open_sidebar(&mut self, gw: &mut dyn TuiGateway, snap: &AppState) {
        let mode = if self.props.columns >= MIN_SIDEBAR_COLS { "sidebar" } else { "overlay" };
        match gw.list_sessions(obj(vec![("projectKey", self.pk()), ("limit", Some(json!(20)))])) {
            Ok(r) => {
                let sessions = r.get("sessions").and_then(Value::as_array).cloned().unwrap_or_default();
                self.set(|s| {
                    let g = group_sessions(&sessions, &s.sidebar_group_by);
                    s.sidebar_collapsed = compute_smart_collapse(&g, &s.active_session_key);
                    s.sessions = sessions;
                    s.dashboard_mode = mode.into();
                    s.sidebar_cursor_index = 0;
                });
            }
            Err(_) => {
                let g = group_sessions(&snap.sessions, &snap.sidebar_group_by);
                self.set(|s| {
                    s.sidebar_collapsed = compute_smart_collapse(&g, &s.active_session_key);
                    s.dashboard_mode = mode.into();
                    s.sidebar_cursor_index = 0;
                });
            }
        }
    }

    fn open_tool_output(&mut self, msg: &TuiMessage) {
        let TuiMessage::Tool { text, tool_name, result_path, full_text, .. } = msg else { return };
        let mut t = full_text.clone().unwrap_or_else(|| text.clone());
        if let Some(p) = result_path {
            if let Ok(c) = std::fs::read(p) {
                t = String::from_utf8_lossy(&c).into_owned();
            }
        }
        let n = t.split('\n').count();
        let name = tool_name.clone();
        if n < 50 {
            self.scrollback.push(format!("\n--- {} output ---\n{t}\n---\n", name.as_deref().unwrap_or("tool")));
        } else {
            let title = name.unwrap_or_else(|| "tool output".into());
            self.set(|s| {
                s.viewer_content = Some(t);
                s.viewer_title = title;
            });
        }
    }

    /// `handleSubmit` synchronous prefix.
    fn submit(&mut self, gw: &mut dyn TuiGateway, snap: &AppState, raw: &str, deferred: &mut Vec<Deferred>) {
        let trimmed = js_trim(raw).to_string();
        self.set(|s| s.input = String::new());
        if trimmed.is_empty() {
            if !snap.is_running {
                if let Some(fi) = snap.focused_index {
                    if matches!(snap.messages.get(fi as usize), Some(TuiMessage::Tool { .. })) {
                        self.set(|s| {
                            if let Some(TuiMessage::Tool { expanded, .. }) = s.messages.get_mut(fi as usize) {
                                *expanded = Some(!expanded.unwrap_or(false));
                            }
                        });
                        return;
                    }
                }
                let last = snap.messages.iter().rev().find(|m| matches!(m, TuiMessage::Tool { result_path, .. } if line_count(m) > 4.0 || result_path.is_some()));
                if let Some(m) = last {
                    deferred.push(Deferred::OpenToolOutput(m.clone()));
                }
            }
            return;
        }
        if snap.is_running || !snap.pending_permissions.is_empty() {
            return;
        }
        match self.command(gw, snap, &trimmed, deferred) {
            Ok(true) => return,
            Ok(false) => {}
            Err(e) => {
                self.set(|s| s.messages.push(error(format!("Command failed: {e}"))));
                return;
            }
        }
        deferred.push(Deferred::Turn(trimmed));
    }

    fn run_turn(&mut self, gw: &mut dyn TuiGateway, snap: &AppState, trimmed: String) {
        self.set(|s| {
            s.messages.push(TuiMessage::User { text: trimmed.clone() });
            s.is_running = true;
            s.scroll_offset = 0;
            s.focused_index = None;
        });
        let params = obj(vec![
            ("sessionKey", Some(snap.active_session_key.clone().into())),
            ("channelKey", Some("tui".into())),
            ("projectKey", self.pk()),
            ("message", Some(trimmed.into())),
            ("mode", Some(snap.mode.clone().into())),
        ]);
        let mut events: Vec<Value> = vec![];
        // Events are applied as they arrive; collecting first is equivalent for the state machine
        // because nothing else can run between them in this synchronous model.
        let r = gw.submit_turn(params, &mut |e| events.push(e.clone()));
        for e in &events {
            self.apply_event(e);
        }
        if let Err(msg) = r {
            self.set(|s| {
                s.is_running = false;
                s.messages.push(error(msg));
            });
        }
    }

    /// A streamed turn failed (`for await` threw): same as the `catch` in `handleSubmit`.
    pub fn turn_failed(&mut self, msg: String) {
        self.set(|s| {
            s.is_running = false;
            s.messages.push(error(msg));
        });
    }

    pub fn apply_event(&mut self, e: &Value) {
        self.set(|s| {
            let rs = ReducerState {
                messages: s.messages.clone(),
                activity: s.activity.clone(),
                mode: s.mode.clone(),
                is_running: s.is_running,
                pending_permissions: s.pending_permissions.clone(),
            };
            let n = reducer::apply(&rs, e);
            s.messages = n.messages;
            s.activity = n.activity;
            s.mode = n.mode;
            s.is_running = n.is_running;
            s.pending_permissions = n.pending_permissions;
            if event_replaces_messages(e) {
                s.scroll_offset = 0;
            }
        });
    }

    fn run_deferred(&mut self, gw: &mut dyn TuiGateway, snap: &AppState, d: Deferred) {
        match d {
            Deferred::OpenSidebar => self.open_sidebar(gw, snap),
            Deferred::LoadSession { key, label } => self.load_session(gw, &key, &label),
            Deferred::OpenToolOutput(m) => self.open_tool_output(&m),
            Deferred::Turn(t) => self.run_turn(gw, snap, t),
            Deferred::NewSession => match gw.new_session(obj(vec![("channelKey", Some("tui".into())), ("projectKey", self.pk())])) {
                Ok(r) => {
                    let k = r.get("sessionKey").and_then(Value::as_str).unwrap_or("").to_string();
                    self.set(|s| {
                        s.messages = vec![system(format!("New session: {k}"))];
                        s.active_session_key = k;
                    });
                }
                Err(e) => self.set(|s| s.messages.push(error(format!("Command failed: {e}")))),
            },
            Deferred::Switch(n) => {
                match gw.list_sessions(obj(vec![("projectKey", self.pk()), ("limit", Some(json!(20)))])) {
                    Ok(r) => {
                        let sessions = r.get("sessions").and_then(Value::as_array).cloned().unwrap_or_default();
                        if let Some(t) = sessions.get(n - 1).cloned() {
                            self.set(|s| s.sessions = sessions);
                            let mut more = vec![];
                            self.switch_start(gw, &session_key_of(&t), summary_of(&t), &mut more);
                            for d in more {
                                self.run_deferred(gw, snap, d);
                            }
                        } else {
                            self.open_sidebar(gw, snap);
                        }
                    }
                    Err(e) => self.set(|s| s.messages.push(error(format!("Command failed: {e}")))),
                }
            }
        }
    }

    /// `handleCommand`. `Ok(false)` = not a command.
    fn command(&mut self, _gw: &mut dyn TuiGateway, snap: &AppState, command: &str, deferred: &mut Vec<Deferred>) -> Result<bool, String> {
        if !command.starts_with('/') {
            return Ok(false);
        }
        let parts = split_ws(command);
        let name = parts[0];
        let args = &parts[1..];
        let home = self.props.pilot_home.clone();
        match name {
            "/new" => deferred.push(Deferred::NewSession),
            "/sessions" | "/switch" | "/dashboard" => {
                if name == "/switch" && !args.is_empty() {
                    if let Some(n) = js_parse_int(args[0]).filter(|n| *n >= 1.0) {
                        deferred.push(Deferred::Switch(n as usize));
                        return Ok(true);
                    }
                }
                deferred.push(Deferred::OpenSidebar);
            }
            "/permissions" => {
                let cur = read_permission_settings(&home);
                let Some(sub) = args.first().copied() else {
                    let list = |v: &Vec<String>| if v.is_empty() { "(none)".to_string() } else { v.join(", ") };
                    let text = format!("skipPermissions: {}\nallow: {}\ndeny: {}", cur.skip_permissions, list(&cur.allowed_tools), list(&cur.disallowed_tools));
                    self.set(|s| s.messages.push(system(text)));
                    return Ok(true);
                };
                let entry = js_trim(&args[1..].join(" ")).to_string();
                let msg = if entry.is_empty() && sub != "bypass" {
                    error("Usage: /permissions [allow|deny|clear <entry>|bypass]")
                } else if sub == "allow" && !entry.is_empty() {
                    let mut a = cur.allowed_tools.clone();
                    a.push(entry.clone());
                    write_permission_settings(&home, SettingsPatch { allowed_tools: Some(a), ..Default::default() });
                    system(format!("Added allow: {entry}"))
                } else if sub == "deny" && !entry.is_empty() {
                    let mut a = cur.disallowed_tools.clone();
                    a.push(entry.clone());
                    write_permission_settings(&home, SettingsPatch { disallowed_tools: Some(a), ..Default::default() });
                    system(format!("Added deny: {entry}"))
                } else if sub == "clear" && !entry.is_empty() {
                    write_permission_settings(&home, SettingsPatch {
                        allowed_tools: Some(cur.allowed_tools.iter().filter(|e| **e != entry).cloned().collect()),
                        disallowed_tools: Some(cur.disallowed_tools.iter().filter(|e| **e != entry).cloned().collect()),
                        ..Default::default()
                    });
                    system(format!("Cleared: {entry}"))
                } else if sub == "bypass" {
                    write_permission_settings(&home, SettingsPatch { skip_permissions: Some(true), ..Default::default() });
                    self.set(|s| s.mode = "bypassPermissions".into());
                    system("Permissions bypassed globally (skipPermissions=true).")
                } else {
                    error(format!("Unknown /permissions subcommand: {sub}"))
                };
                self.set(|s| s.messages.push(msg));
            }
            "/mode" => {
                let req = args.first().copied().unwrap_or("");
                let mode = if req == "plan" || req == "bypassPermissions" { req } else { "default" };
                if mode == "bypassPermissions" {
                    write_permission_settings(&home, SettingsPatch { skip_permissions: Some(true), ..Default::default() });
                } else if read_permission_settings(&home).skip_permissions {
                    write_permission_settings(&home, SettingsPatch { skip_permissions: Some(false), ..Default::default() });
                }
                self.set(|s| {
                    s.mode = mode.into();
                    s.messages.push(system(format!("Mode: {mode}")));
                });
            }
            "/view" => {
                let n = js_parse_int(args.first().copied().unwrap_or(""));
                let tools: Vec<&TuiMessage> = snap
                    .messages
                    .iter()
                    .filter(|m| matches!(m, TuiMessage::Tool { result_path, full_text, .. } if line_count(m) > 4.0 || result_path.is_some() || full_text.is_some()))
                    .collect();
                if tools.is_empty() {
                    self.set(|s| s.messages.push(system("No tool output to view.")));
                    return Ok(true);
                }
                let t = match n {
                    Some(n) if n >= 1.0 && n <= tools.len() as f64 => tools[n as usize - 1],
                    _ => tools[tools.len() - 1],
                };
                deferred.push(Deferred::OpenToolOutput(t.clone()));
            }
            "/clear" => self.set(|s| {
                s.messages.clear();
                s.focused_index = None;
            }),
            "/help" => self.set(|s| s.help_open = !s.help_open),
            "/exit" => self.exited = true,
            _ => self.set(|s| s.messages.push(error(format!("Unknown command {name}")))),
        }
        Ok(true)
    }

    pub fn text_input_order_first(&self) -> bool {
        self.text_input_first
    }
    pub fn input_len_utf16(&self) -> i64 {
        utf16_len(&self.state.input)
    }
}

enum Deferred {
    OpenSidebar,
    LoadSession { key: String, label: String },
    OpenToolOutput(TuiMessage),
    Turn(String),
    NewSession,
    Switch(usize),
}
