//! state.json persistence (spec 6.1): atomic write, missing file is "no state".

use crate::model::State;
use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn state_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state/ymux/state.json")
}

/// The restore snapshot (spec 6.1): state as it last stood with no window closing for
/// 5 s, so the window-closing burst of a shutdown never reaches it.
pub fn snapshot_path() -> PathBuf {
    state_path().with_file_name("relaunch.json")
}

/// How long closes must have stopped before the snapshot is written.
pub const SNAPSHOT_QUIET: std::time::Duration = std::time::Duration::from_secs(5);

pub fn snapshot_due(last_close: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    last_close.is_none_or(|t| now.duration_since(t) >= SNAPSHOT_QUIET)
}

/// The state a daemon starts from: `state.json` while the same Hyprland runs; on a new
/// Hyprland, the snapshot (state.json was stripped by the shutdown's closes), unless it
/// is missing or holds no sessions.
pub fn start_state(state: Option<State>, snapshot: Option<State>, signature: &str) -> Option<State> {
    let same = state.as_ref().is_some_and(|s| s.signature.as_deref() == Some(signature));
    match snapshot {
        Some(snap) if !same && !snap.sessions.is_empty() => Some(snap),
        _ => state,
    }
}

pub fn load(path: &Path) -> Result<Option<State>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Writes `text` to a hidden temp file in the same directory, fsyncs it, then renames
/// it over `path`, so a watcher (Hyprland, the shell) never reads a partial file.
pub fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.tmp"));
    let mut f = fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    fs::rename(&tmp, path).with_context(|| format!("renaming to {}", path.display()))?;
    Ok(())
}

pub fn save_atomic(state: &State, path: &Path) -> Result<()> {
    write_atomic(path, &serde_json::to_string_pretty(state)?)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    fn with(sig: &str, sessions: bool) -> State {
        let mut s = State { signature: Some(sig.into()), ..State::default() };
        if sessions {
            s.ensure_monitor("LG A", "DP-1");
            s.create_session("LG A", None);
        }
        s
    }

    #[test]
    fn snapshot_waits_until_closes_have_stopped_for_5_s() {
        let now = Instant::now();
        assert!(snapshot_due(None, now));
        assert!(!snapshot_due(Some(now - Duration::from_secs(2)), now));
        assert!(snapshot_due(Some(now - Duration::from_secs(5)), now));
    }

    #[test]
    fn new_hyprland_starts_from_the_snapshot() {
        let got = start_state(Some(with("old", false)), Some(with("old", true)), "new").unwrap();
        assert!(!got.sessions.is_empty(), "the stripped state.json is not used");
    }

    #[test]
    fn daemon_restart_keeps_state_json() {
        let got = start_state(Some(with("same", false)), Some(with("same", true)), "same").unwrap();
        assert!(got.sessions.is_empty());
    }

    #[test]
    fn no_or_empty_snapshot_falls_back_to_state_json() {
        assert!(start_state(Some(with("old", true)), None, "new").unwrap().sessions.len() == 1);
        assert!(start_state(Some(with("old", true)), Some(with("old", false)), "new").unwrap().sessions.len() == 1);
        assert!(start_state(None, None, "new").is_none());
    }

    #[test]
    fn snapshot_lives_next_to_state_json() {
        assert_eq!(snapshot_path().file_name().unwrap(), "relaunch.json");
        assert_eq!(snapshot_path().parent(), state_path().parent());
    }

    use super::*;
    use crate::model::State;

    #[test]
    fn save_is_atomic_and_round_trips() {
        let d = std::env::temp_dir().join(format!("ymux-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let path = d.join("state.json");
        let mut s = State::default();
        s.ensure_monitor("M", "DP-1");
        s.create_session("M", Some("main"));
        save_atomic(&s, &path).unwrap();
        assert_eq!(load(&path).unwrap(), Some(s));
        assert!(!d.join(".state.json.tmp").exists());
    }

    #[test]
    fn write_atomic_replaces_file_and_leaves_no_tmp() {
        let d = std::env::temp_dir().join(format!("ymux-wa-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("ymux.lua");
        write_atomic(&f, "a").unwrap();
        write_atomic(&f, "b").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "b");
        assert!(!d.join(".ymux.lua.tmp").exists());
    }

    #[test]
    fn load_missing_file_is_none() {
        assert_eq!(load(std::path::Path::new("/nonexistent/ymux/state.json")).unwrap(), None);
    }
}
