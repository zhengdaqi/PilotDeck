//! Port of `ui/src/utils/dateUtils.ts` `formatTimeAgo`. The host parses the date (`Date.parse`) and formats the
//! locale fallback (`toLocaleDateString`). The core does the same floor/threshold cascade on the millisecond values.
#[derive(Debug, Clone, PartialEq)]
pub enum TimeAgo {
    /// i18n key plus an optional `{ count }`.
    Key(&'static str, Option<i64>),
    /// More than 7 days: `new Date(ms).toLocaleDateString()` (host-formatted).
    LocaleDate(f64),
}

pub fn format_time_ago(parsed_ms: f64, now_ms: f64) -> TimeAgo {
    if parsed_ms.is_nan() {
        return TimeAgo::Key("status.unknown", None);
    }
    let diff = now_ms - parsed_ms;
    let s = (diff / 1000.0).floor();
    let m = (diff / 60_000.0).floor();
    let h = (diff / 3_600_000.0).floor();
    let d = (diff / 86_400_000.0).floor();
    if s < 60.0 {
        TimeAgo::Key("time.justNow", None)
    } else if m == 1.0 {
        TimeAgo::Key("time.oneMinuteAgo", None)
    } else if m < 60.0 {
        TimeAgo::Key("time.minutesAgo", Some(m as i64))
    } else if h == 1.0 {
        TimeAgo::Key("time.oneHourAgo", None)
    } else if h < 24.0 {
        TimeAgo::Key("time.hoursAgo", Some(h as i64))
    } else if d == 1.0 {
        TimeAgo::Key("time.oneDayAgo", None)
    } else if d < 7.0 {
        TimeAgo::Key("time.daysAgo", Some(d as i64))
    } else {
        TimeAgo::LocaleDate(parsed_ms)
    }
}

/// The English text used when no translator is passed (`t` falsy).
pub fn english(t: &TimeAgo) -> Option<String> {
    Some(match t {
        TimeAgo::Key("status.unknown", _) => "Unknown".into(),
        TimeAgo::Key("time.justNow", _) => "Just now".into(),
        TimeAgo::Key("time.oneMinuteAgo", _) => "1 min ago".into(),
        TimeAgo::Key("time.minutesAgo", Some(n)) => format!("{n} mins ago"),
        TimeAgo::Key("time.oneHourAgo", _) => "1 hour ago".into(),
        TimeAgo::Key("time.hoursAgo", Some(n)) => format!("{n} hours ago"),
        TimeAgo::Key("time.oneDayAgo", _) => "1 day ago".into(),
        TimeAgo::Key("time.daysAgo", Some(n)) => format!("{n} days ago"),
        _ => return None,
    })
}
