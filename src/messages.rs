//! Every message ymux shows as a toast is also kept in
//! `~/.local/state/ymux/errors.log`, so it can be read and copied later with
//! `ymux errors` (user request 2026-11-16: a toast cannot be copied).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// How many messages the log keeps.
pub const KEEP: usize = 200;

pub fn log_path() -> PathBuf {
    crate::store::state_path().with_file_name("errors.log")
}

/// `YYYY-MM-DD HH:MM:SS UTC` for a unix time (no date library needed).
pub fn timestamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // civil-from-days (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem / 60 % 60, rem % 60)
}

/// Appends a message, keeping only the latest `KEEP`.
pub fn record(path: &Path, msg: &str, now: u64) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let line = format!("{}  {}", timestamp(now), msg.replace('\n', " "));
    let mut lines: Vec<String> = std::fs::read_to_string(path).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default();
    lines.push(line);
    let start = lines.len().saturating_sub(KEEP);
    if start > 0 {
        let _ = std::fs::write(path, lines[start..].join("\n") + "\n");
    } else if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{}", lines.last().expect("pushed"));
    }
}

/// The last `n` messages, oldest first.
pub fn latest(path: &Path, n: usize) -> Vec<String> {
    let lines: Vec<String> = std::fs::read_to_string(path).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

pub fn toast_body(msg: &str) -> String {
    format!("{msg}\n(to copy it: run ymux errors)")
}

/// The one way ymux shows a message: kept in the log, then a toast.
pub fn notify(msg: &str) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    record(&log_path(), msg, now);
    let _ = std::process::Command::new("notify-send").args(["ymux", &toast_body(msg)]).status();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ymux-msg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("errors.log")
    }

    #[test]
    fn messages_are_appended_with_a_time() {
        let p = tmp("append");
        record(&p, "ymux: no marked tile", 1_794_800_000);
        record(&p, "ymux: daemon not running", 1_794_800_060);
        let text = std::fs::read_to_string(&p).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("ymux: no marked tile"));
        assert!(lines[1].starts_with("2026-"), "a readable date: {}", lines[1]);
    }

    #[test]
    fn the_log_keeps_only_the_latest_entries() {
        let p = tmp("trim");
        for i in 0..(KEEP + 20) {
            record(&p, &format!("message {i}"), 1_794_800_000 + i as u64);
        }
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text.lines().count(), KEEP);
        assert!(text.lines().last().unwrap().ends_with(&format!("message {}", KEEP + 19)));
    }

    #[test]
    fn latest_returns_the_last_n_newest_last() {
        let p = tmp("latest");
        for i in 0..5 {
            record(&p, &format!("m{i}"), 1_794_800_000 + i);
        }
        let l = latest(&p, 2);
        assert_eq!(l.len(), 2);
        assert!(l[0].ends_with("m3") && l[1].ends_with("m4"));
        assert!(latest(&tmp("none"), 5).is_empty());
    }

    #[test]
    fn the_toast_says_where_to_copy_it_from() {
        assert!(toast_body("ymux: no marked tile").contains("ymux errors"));
        assert!(toast_body("ymux: no marked tile").starts_with("ymux: no marked tile"));
    }

    #[test]
    fn dates_are_utc_and_readable() {
        assert_eq!(timestamp(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(timestamp(1_794_800_000), "2026-11-16 03:33:20 UTC");
    }
}
