//! Port of `ink-text-input@6` (value editing, cursor bookkeeping) with JS UTF-16 semantics,
//! including its quirks: the clamp after an edit tests the *old* cursor, so the cursor can go
//! to -1 / len+1 and a later insert uses `String.prototype.slice` negative-index rules.
use crate::Key;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextInputState {
    pub cursor_offset: i64,
    pub cursor_width: i64,
}

pub fn utf16_len(s: &str) -> i64 {
    s.encode_utf16().count() as i64
}

/// JS `s.slice(start, end)` on UTF-16 code units.
pub fn js_slice(s: &str, start: i64, end: Option<i64>) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    let len = u.len() as i64;
    let norm = |i: i64| if i < 0 { (len + i).max(0) } else { i.min(len) };
    let a = norm(start);
    let b = end.map(norm).unwrap_or(len);
    if a >= b {
        return String::new();
    }
    String::from_utf16_lossy(&u[a as usize..b as usize])
}

pub enum TextInputOutcome {
    Ignored,
    Submit(String),
    Edited { changed: Option<String> },
}

impl TextInputState {
    pub fn mount(value: &str) -> Self {
        Self { cursor_offset: utf16_len(value), cursor_width: 0 }
    }

    /// `useEffect([value, focus, showCursor])` clamp.
    pub fn effect(&mut self, value: &str, focus: bool) {
        if !focus {
            return;
        }
        let len = utf16_len(value);
        if self.cursor_offset > len - 1 {
            *self = Self { cursor_offset: len, cursor_width: 0 };
        }
    }

    pub fn handle(&mut self, value: &str, input: &str, key: &Key) -> TextInputOutcome {
        if key.up_arrow || key.down_arrow || (key.ctrl && input == "c") || key.tab {
            return TextInputOutcome::Ignored;
        }
        if key.return_ {
            return TextInputOutcome::Submit(value.to_string());
        }
        let cur = self.cursor_offset;
        let mut next_off = cur;
        let mut next_val = value.to_string();
        let mut next_w = 0;
        if key.left_arrow {
            next_off -= 1;
        } else if key.right_arrow {
            next_off += 1;
        } else if key.backspace || key.delete {
            if cur > 0 {
                next_val = js_slice(value, 0, Some(cur - 1)) + &js_slice(value, cur, Some(utf16_len(value)));
                next_off -= 1;
            }
        } else {
            next_val = js_slice(value, 0, Some(cur)) + input + &js_slice(value, cur, Some(utf16_len(value)));
            let il = utf16_len(input);
            next_off += il;
            if il > 1 {
                next_w = il;
            }
        }
        if cur < 0 {
            next_off = 0;
        }
        if cur > utf16_len(value) {
            next_off = utf16_len(value);
        }
        *self = Self { cursor_offset: next_off, cursor_width: next_w };
        TextInputOutcome::Edited { changed: (next_val != value).then_some(next_val) }
    }
}
