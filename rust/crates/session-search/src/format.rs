//! Port of `src/session/search/formatChatHistorySearch.ts` (CLI uses `en`).

use crate::search::{Role, SearchMatch, SearchResult};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike, Datelike};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    Zh,
    En,
}

pub fn format_results(result: &SearchResult, locale: Locale, include_project: bool) -> String {
    let zh = locale == Locale::Zh;
    if crate::js::js_trim(&result.query).is_empty() {
        return if zh {
            "用法：/search <关键词> [--all] [--limit N] [--role user|assistant]\n示例：/search docker 部署".into()
        } else {
            "Usage: /search <keyword> [--all] [--limit N] [--role user|assistant]\nExample: /search docker deploy".into()
        };
    }
    if result.matches.is_empty() {
        return if zh {
            format!("未找到包含「{}」的聊天记录。\n已扫描 {} 个会话。", result.query, result.sessions_scanned)
        } else {
            format!("No chat history matches for \"{}\".\nScanned {} session(s).", result.query, result.sessions_scanned)
        };
    }
    let header = if zh {
        format!(
            "🔍 找到 {} 条匹配「{}」{}",
            result.matches.len(),
            result.query,
            if result.truncated { "（结果已截断，可加大 --limit）" } else { "" }
        )
    } else {
        format!(
            "🔍 {} match(es) for \"{}\"{}",
            result.matches.len(),
            result.query,
            if result.truncated { " (truncated — try a higher --limit)" } else { "" }
        )
    };
    let mut lines = vec![header, String::new()];
    for (i, m) in result.matches.iter().enumerate() {
        lines.push(format_match_line(m, i + 1, locale, include_project));
        lines.push(String::new());
    }
    lines.push(if zh {
        "提示：在 Web UI 中打开对应会话后，可用 /search 结果中的片段定位消息。".into()
    } else {
        "Tip: open the session in the Web UI to jump to the matched message.".into()
    });
    lines.join("\n").trim_end_matches(crate::js::is_js_space).to_string()
}

fn format_match_line(m: &SearchMatch, index: usize, locale: Locale, include_project: bool) -> String {
    let zh = locale == Locale::Zh;
    let role = match (m.role, zh) {
        (Role::User, true) => "用户",
        (Role::User, false) => "user",
        (Role::Assistant, true) => "助手",
        (Role::Assistant, false) => "assistant",
    };
    let short_id = if crate::js::utf16_len(&m.session_id) <= 12 {
        m.session_id.clone()
    } else {
        format!("{}…", crate::js::utf16_slice(&m.session_id, 0, 8))
    };
    let when = format_when(&m.created_at, locale);
    let project_suffix = match (&m.project_key, include_project) {
        (Some(key), true) => {
            let last = key.rsplit(['/', '\\']).next().unwrap_or("");
            format!(" · {}", if last.is_empty() { key.as_str() } else { last })
        }
        _ => String::new(),
    };
    format!(
        "{index}. **{}** (`{short_id}`){project_suffix}\n   {role} · {when}\n   > {}",
        m.session_title, m.snippet
    )
}

/// Parse the ISO-8601 subset that `new Date(string)` accepts for transcripts.
fn parse_js_date(s: &str) -> Option<DateTime<Local>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Local));
    }
    // Date-only ISO forms are UTC in JS.
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(chrono::Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0)?).with_timezone(&Local));
    }
    // Date-time without offset is local time in JS.
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M"] {
        if let Ok(n) = NaiveDateTime::parse_from_str(s, fmt) {
            return Local.from_local_datetime(&n).earliest();
        }
    }
    None
}

fn format_when(created_at: &str, locale: Locale) -> String {
    if created_at.is_empty() {
        return if locale == Locale::Zh { "未知时间".into() } else { "unknown time".into() };
    }
    let Some(dt) = parse_js_date(created_at) else { return created_at.to_string() };
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    match locale {
        // en-US: "Sep 10, 08:05 AM" (Node 22.23 ICU emits a regular space before the day period).
        Locale::En => {
            let (pm, h12) = dt.hour12();
            format!(
                "{} {}, {:02}:{:02} {}",
                MONTHS[dt.month0() as usize],
                dt.day(),
                h12,
                dt.minute(),
                if pm { "PM" } else { "AM" }
            )
        }
        // zh-CN: "9月10日 08:05"
        Locale::Zh => format!("{}月{}日 {:02}:{:02}", dt.month(), dt.day(), dt.hour(), dt.minute()),
    }
}
