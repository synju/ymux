//! Omarchy per-workspace layout modes (spec 3.2.2, 3.2.11). Files look like
//! `o.workspace_mode({ workspace = "1", mode = "scrolling" })` (verified against
//! ~/.local/state/omarchy/workspace-layouts/1.lua).

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn layouts_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state/omarchy/workspace-layouts")
}

fn file(dir: &Path, id: i64) -> PathBuf {
    dir.join(format!("{id}.lua"))
}

fn mode_in(text: &str) -> Option<String> {
    let rest = &text[text.find("mode = \"")? + "mode = \"".len()..];
    Some(rest[..rest.find('"')?].to_string())
}

/// The mode exactly as `omarchy-hyprland-workspace-layout-current` resolves it.
pub fn read_mode(dir: &Path, id: i64, live_tiled_layout: Option<&str>, default_mode: impl Fn() -> Option<String>) -> String {
    if let Some(m) = fs::read_to_string(file(dir, id)).ok().as_deref().and_then(mode_in) {
        return m;
    }
    if dir.join("default.lua").exists()
        && let Some(m) = default_mode()
    {
        return m;
    }
    live_tiled_layout.map(str::to_string).unwrap_or_else(|| "dwindle".into())
}

/// Runs Omarchy's default-layout script (used as `read_mode`'s `default_mode`).
pub fn omarchy_default_mode() -> Option<String> {
    let out = Command::new("omarchy-hyprland-workspace-layout-default").output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !s.is_empty()).then_some(s)
}

pub fn move_mode_file(dir: &Path, old: i64, new: i64) -> Result<()> {
    let from = file(dir, old);
    let Ok(text) = fs::read_to_string(&from) else { return Ok(()) };
    let text = text.replace(&format!("workspace = \"{old}\""), &format!("workspace = \"{new}\""));
    fs::write(file(dir, new), text).with_context(|| format!("writing {new}.lua"))?;
    fs::remove_file(&from).with_context(|| format!("removing {old}.lua"))?;
    Ok(())
}

/// Writes a workspace's mode file the way Omarchy's toggle does.
pub fn write_mode_file(dir: &Path, id: i64, mode: &str) -> Result<()> {
    fs::create_dir_all(dir).context("creating the layouts folder")?;
    fs::write(file(dir, id), format!("o.workspace_mode({{ workspace = \"{id}\", mode = \"{mode}\" }})\n"))
        .with_context(|| format!("writing {id}.lua"))
}

pub fn delete_mode_file(dir: &Path, id: i64) -> Result<()> {
    match fs::remove_file(file(dir, id)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).context("removing mode file"),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ymux-layout-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn scrolling_file_moves_with_renumber() {
        let d = tmp("move");
        fs::write(d.join("1.lua"), "o.workspace_mode({ workspace = \"1\", mode = \"scrolling\" })\n").unwrap();
        move_mode_file(&d, 1, 100001).unwrap();
        assert!(!d.join("1.lua").exists());
        assert_eq!(fs::read_to_string(d.join("100001.lua")).unwrap(), "o.workspace_mode({ workspace = \"100001\", mode = \"scrolling\" })\n");
        assert_eq!(read_mode(&d, 100001, None, || None), "scrolling");
    }

    #[test]
    fn move_without_file_is_noop() {
        let d = tmp("nofile");
        move_mode_file(&d, 3, 100003).unwrap();
        assert!(!d.join("100003.lua").exists());
    }

    #[test]
    fn no_file_no_default_falls_to_live_then_dwindle() {
        let d = tmp("chain");
        assert_eq!(read_mode(&d, 5, Some("master"), || None), "master");
        assert_eq!(read_mode(&d, 5, None, || None), "dwindle");
    }

    #[test]
    fn default_lua_present_uses_default_script() {
        let d = tmp("default");
        fs::write(d.join("default.lua"), "").unwrap();
        assert_eq!(read_mode(&d, 5, Some("dwindle"), || Some("scrolling".into())), "scrolling");
    }

    #[test]
    fn delete_removes_file_and_missing_is_ok() {
        let d = tmp("del");
        fs::write(d.join("100001.lua"), "x").unwrap();
        delete_mode_file(&d, 100001).unwrap();
        assert!(!d.join("100001.lua").exists());
        delete_mode_file(&d, 100001).unwrap();
    }
}
