//! Hyprland access: the `Hypr` trait, a `hyprctl`-backed implementation, and the Lua
//! dispatcher expressions ymux sends (forms as Omarchy uses them in
//! monitor-workspaces.lua and Workspaces.qml).

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::process::Command;

#[derive(Clone, Debug, PartialEq)]
pub struct HMonitor {
    pub name: String,
    pub description: String,
    pub focused: bool,
    pub active_workspace: i64,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HWorkspace {
    pub id: i64,
    /// Identifies special workspaces such as `special:minimized` (dynamic ids).
    pub name: String,
    pub monitor: String,
    pub windows: u32,
    pub tiled_layout: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HClient {
    pub address: String,
    pub pid: u32,
    pub class: String,
    pub title: String,
    pub workspace: i64,
    pub workspace_name: String,
    pub floating: bool,
    pub at: (i32, i32),
    pub size: (i32, i32),
    pub tags: Vec<String>,
    pub hidden: bool,
}

/// What `/proc` says about a process: its argv and working directory (spec 7.2).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProcInfo {
    pub cmdline: Vec<String>,
    pub cwd: Option<String>,
}

/// Reads `/proc/<pid>/cmdline` (NUL-separated) and the `/proc/<pid>/cwd` link; empty
/// when the process is gone or not readable.
pub fn read_proc(pid: u32) -> ProcInfo {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|b| b.split(|c| *c == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect())
        .unwrap_or_default();
    let cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).ok().map(|p| p.display().to_string());
    ProcInfo { cmdline, cwd }
}

pub trait Hypr {
    fn monitors(&self) -> Result<Vec<HMonitor>>;
    fn workspaces(&self) -> Result<Vec<HWorkspace>>;
    fn clients(&self) -> Result<Vec<HClient>>;
    fn active_address(&self) -> Result<Option<String>>;
    fn dispatch(&self, lua: &str) -> Result<()>;
    fn eval(&self, lua: &str) -> Result<()>;
    /// Starts a detached process; returns its pid.
    fn spawn(&self, argv: &[String], cwd: Option<&std::path::Path>) -> Result<u32>;
    fn notify(&self, msg: &str);
    fn proc_info(&self, pid: u32) -> ProcInfo {
        read_proc(pid)
    }
}

// ---- Lua dispatcher expressions -------------------------------------------------

pub fn focus_workspace(id: i64) -> String {
    format!("hl.dsp.focus({{ workspace = \"{id}\" }})")
}

pub fn focus_monitor(name: &str) -> String {
    format!("hl.dsp.focus({{ monitor = \"{name}\" }})")
}

pub fn move_window(address: &str, id: i64, follow: bool) -> String {
    format!("hl.dsp.window.move({{ workspace = \"{id}\", follow = {follow}, window = \"address:{address}\" }})")
}

/// Moves a window to a workspace by name (e.g. `special:ymux-stash`), not following.
pub fn move_to(address: &str, workspace: &str) -> String {
    format!("hl.dsp.window.move({{ workspace = \"{workspace}\", follow = false, window = \"address:{address}\" }})")
}

pub fn move_workspace(id: i64, monitor: &str) -> String {
    format!("hl.dsp.workspace.move({{ workspace = \"{id}\", monitor = \"{monitor}\" }})")
}

pub fn set_mode(id: i64, mode: &str) -> String {
    format!("o.workspace_mode({{ workspace = \"{id}\", mode = \"{mode}\" }})")
}

pub fn swap_windows(target: &str, window: &str) -> String {
    format!("hl.dsp.window.swap({{ target = \"address:{target}\", window = \"address:{window}\" }})")
}

pub fn focus_window(address: &str) -> String {
    format!("hl.dsp.focus({{ window = \"address:{address}\" }})")
}

pub fn close_window(address: &str) -> String {
    format!("hl.dsp.window.close({{ window = \"address:{address}\" }})")
}

/// A layout message to the focused window's layout, e.g. dwindle's `swapsplit`.
pub fn layout_msg(msg: &str) -> String {
    format!("hl.dsp.layout(\"{msg}\")")
}

/// Absolute position of a floating window (spikes.md "parking").
pub fn move_exact(address: &str, x: i32, y: i32) -> String {
    format!("hl.dsp.window.move({{ x = {x}, y = {y}, relative = false, window = \"address:{address}\" }})")
}

/// Absolute size (spike: `relative = false` works on a window by address).
pub fn resize_exact(address: &str, w: i32, h: i32) -> String {
    format!("hl.dsp.window.resize({{ x = {w}, y = {h}, relative = false, window = \"address:{address}\" }})")
}

pub fn change_id(old: i64, new: i64) -> String {
    format!("hl.dsp.workspace.change_id({{ workspace = \"{old}\", id = {new} }})")
}

// ---- hyprctl JSON (camelCase, nested workspace objects) --------------------------

#[derive(Deserialize)]
struct RawRef {
    id: i64,
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMonitor {
    name: String,
    description: String,
    focused: bool,
    active_workspace: RawRef,
    x: i32,
    y: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawWorkspace {
    id: i64,
    name: String,
    monitor: String,
    windows: u32,
    #[serde(default)]
    tiled_layout: String,
}

#[derive(Deserialize)]
struct RawClient {
    address: String,
    pid: i64,
    class: String,
    title: String,
    workspace: RawRef,
    floating: bool,
    at: (i32, i32),
    size: (i32, i32),
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    hidden: bool,
}

pub fn parse_monitors(json: &str) -> Result<Vec<HMonitor>> {
    let raw: Vec<RawMonitor> = serde_json::from_str(json).context("hyprctl monitors json")?;
    Ok(raw
        .into_iter()
        .map(|m| HMonitor {
            name: m.name,
            description: m.description,
            focused: m.focused,
            active_workspace: m.active_workspace.id,
            x: m.x,
            y: m.y,
        })
        .collect())
}

pub fn parse_workspaces(json: &str) -> Result<Vec<HWorkspace>> {
    let raw: Vec<RawWorkspace> = serde_json::from_str(json).context("hyprctl workspaces json")?;
    Ok(raw
        .into_iter()
        .map(|w| HWorkspace { id: w.id, name: w.name, monitor: w.monitor, windows: w.windows, tiled_layout: w.tiled_layout })
        .collect())
}

fn to_client(c: RawClient) -> HClient {
    HClient {
        address: crate::events::normalise_address(&c.address),
        pid: c.pid.max(0) as u32,
        class: c.class,
        title: c.title,
        workspace: c.workspace.id,
        workspace_name: c.workspace.name,
        floating: c.floating,
        at: c.at,
        size: c.size,
        tags: c.tags,
        hidden: c.hidden,
    }
}

pub fn parse_clients(json: &str) -> Result<Vec<HClient>> {
    let raw: Vec<RawClient> = serde_json::from_str(json).context("hyprctl clients json")?;
    Ok(raw.into_iter().map(to_client).collect())
}

/// `hyprctl -j activewindow` prints `{}` when nothing is focused.
/// `hyprctl -j instances`: the Wayland socket of the instance with `signature`.
pub fn parse_wl_socket(json: &str, signature: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct I {
        instance: String,
        wl_socket: String,
    }
    let list: Vec<I> = serde_json::from_str(json).ok()?;
    list.into_iter().find(|i| i.instance == signature).map(|i| i.wl_socket)
}

/// The running Hyprland's Wayland socket, asked of `hyprctl instances`.
pub fn wayland_socket(signature: &str) -> Option<String> {
    let out = Command::new("hyprctl").args(["-j", "instances"]).output().ok()?;
    parse_wl_socket(&String::from_utf8_lossy(&out.stdout), signature)
}

pub fn parse_active(json: &str) -> Result<Option<String>> {
    let v: serde_json::Value = serde_json::from_str(json).context("hyprctl activewindow json")?;
    Ok(v.get("address").and_then(|a| a.as_str()).map(crate::events::normalise_address))
}

// ---- hyprctl-backed implementation -----------------------------------------------

pub struct Hyprctl;

fn hyprctl(args: &[&str]) -> Result<String> {
    let out = Command::new("hyprctl").args(args).output().context("running hyprctl")?;
    if !out.status.success() {
        bail!("hyprctl {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn hyprctl_ok(args: &[&str]) -> Result<()> {
    let out = hyprctl(args)?;
    if out.trim() != "ok" {
        bail!("hyprctl {:?}: {}", args, out.trim());
    }
    Ok(())
}

impl Hypr for Hyprctl {
    fn monitors(&self) -> Result<Vec<HMonitor>> {
        parse_monitors(&hyprctl(&["-j", "monitors"])?)
    }
    fn workspaces(&self) -> Result<Vec<HWorkspace>> {
        parse_workspaces(&hyprctl(&["-j", "workspaces"])?)
    }
    fn clients(&self) -> Result<Vec<HClient>> {
        parse_clients(&hyprctl(&["-j", "clients"])?)
    }
    fn active_address(&self) -> Result<Option<String>> {
        parse_active(&hyprctl(&["-j", "activewindow"])?)
    }
    fn dispatch(&self, lua: &str) -> Result<()> {
        hyprctl_ok(&["dispatch", lua])
    }
    fn eval(&self, lua: &str) -> Result<()> {
        hyprctl(&["eval", lua]).map(|_| ())
    }
    fn spawn(&self, argv: &[String], cwd: Option<&std::path::Path>) -> Result<u32> {
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;
        let (prog, args) = argv.split_first().context("empty command")?;
        let mut cmd = Command::new(prog);
        cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0);
        // a saved directory that is gone is not a reason to fail the launch
        if let Some(d) = cwd.filter(|d| d.is_dir()) {
            cmd.current_dir(d);
        }
        let mut child = cmd.spawn().with_context(|| format!("starting {prog}"))?;
        let pid = child.id();
        // reap it so no zombie is left behind
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(pid)
    }
    fn notify(&self, msg: &str) {
        let _ = Command::new("notify-send").args(["ymux", msg]).status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_builders_match_omarchy_forms() {
        assert_eq!(focus_workspace(100003), "hl.dsp.focus({ workspace = \"100003\" })");
        assert_eq!(focus_monitor("DP-1"), "hl.dsp.focus({ monitor = \"DP-1\" })");
        assert_eq!(
            move_window("0xab", 200001, false),
            "hl.dsp.window.move({ workspace = \"200001\", follow = false, window = \"address:0xab\" })"
        );
        assert_eq!(move_workspace(100002, "DP-1"), "hl.dsp.workspace.move({ workspace = \"100002\", monitor = \"DP-1\" })");
        assert_eq!(change_id(3, 100003), "hl.dsp.workspace.change_id({ workspace = \"3\", id = 100003 })");
    }

    #[test]
    fn hyprctl_json_fixtures_deserialize() {
        let clients = parse_clients(include_str!("../tests/fixtures/clients.json")).unwrap();
        assert!(!clients.is_empty());
        assert!(clients.iter().all(|c| c.address.starts_with("0x")));
        let ws = parse_workspaces(include_str!("../tests/fixtures/workspaces.json")).unwrap();
        assert!(ws.iter().all(|w| !w.tiled_layout.is_empty()));
        let mons = parse_monitors(include_str!("../tests/fixtures/monitors.json")).unwrap();
        assert_eq!(mons.len(), 2);
        assert!(mons.iter().any(|m| m.description == "BOE 0x0A3B" && m.x == 1920));
    }

    #[test]
    fn special_workspace_keeps_negative_id_and_name() {
        let c = parse_clients(r#"[{"address":"0xab","pid":5,"class":"foot","title":"t","workspace":{"id":-98,"name":"special:qconsole"},"floating":false,"at":[0,0],"size":[1,1],"tags":[]}]"#).unwrap();
        assert_eq!((c[0].workspace, c[0].workspace_name.as_str()), (-98, "special:qconsole"));
        let w = parse_workspaces(r#"[{"id":-98,"name":"special:qconsole","monitor":"DP-1","windows":1,"tiledLayout":"dwindle"}]"#).unwrap();
        assert_eq!(w[0].name, "special:qconsole");
    }

    #[test]
    fn activewindow_fixture_gives_address() {
        let a = parse_active(include_str!("../tests/fixtures/activewindow.json")).unwrap();
        assert!(a.unwrap().starts_with("0x"));
        assert_eq!(parse_active("{}").unwrap(), None);
    }

    #[test]
    fn wayland_socket_comes_from_our_instance() {
        let json = r#"[{"instance":"other_1","time":1,"pid":9,"wl_socket":"wayland-0"},
                      {"instance":"sig_2","time":2,"pid":10,"wl_socket":"wayland-1"}]"#;
        assert_eq!(super::parse_wl_socket(json, "sig_2").as_deref(), Some("wayland-1"));
        assert_eq!(super::parse_wl_socket(json, "gone"), None);
    }
}
