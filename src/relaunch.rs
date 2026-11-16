//! Relaunch on Hyprland start (spec 7): launch commands, the restore plan, and claiming
//! windows that are already open.

use crate::engine::Engine;
use crate::hypr::{move_exact, move_to, move_window, resize_exact, set_mode, HClient, Hypr, ProcInfo};
use crate::model::{reading_key, State, Tile};
use anyhow::Result;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::Path;
use std::time::{Duration, Instant};

const STASH: &str = "special:ymux-stash";
/// How long a launched app gets to show its window (spec 7.3).
const OPEN_WAIT: Duration = Duration::from_secs(15);

/// How to start a saved app again (Ruling 18): its own argv, or a `launch-override`
/// line through `sh -c`.
#[derive(Clone, Debug, PartialEq)]
pub enum LaunchCmd {
    Argv(Vec<String>),
    Shell(String),
}

pub fn launch_command(tile: &Tile, overrides: &BTreeMap<String, String>) -> Option<LaunchCmd> {
    if tile.is_placeholder() {
        return None;
    }
    if let Some(cmd) = overrides.get(&tile.class) {
        return Some(LaunchCmd::Shell(cmd.clone()));
    }
    match tile.cmdline.as_slice() {
        [] => None,
        // Electron apps rewrite their argv into one space-separated string
        [one] if one.contains(' ') && !Path::new(one).exists() => {
            Some(LaunchCmd::Argv(one.split_whitespace().map(str::to_string).collect()))
        }
        argv => Some(LaunchCmd::Argv(argv.to_vec())),
    }
}

/// One step of a restore (spec 7.3).
#[derive(Clone, Debug, PartialEq)]
pub enum RestoreStep {
    SetMode { workspace: i64, mode: String },
    Open { workspace: i64, tile: Tile },
    Layout { workspace: i64 },
}

/// Per workspace, in (session, workspace number) order: its mode, its tiles in reading
/// order, then its layout.
pub fn plan(state: &State) -> Vec<RestoreStep> {
    let mut ws: Vec<_> = state.workspaces.values().collect();
    ws.sort_by_key(|w| (w.session, w.number));
    let mut steps = Vec::new();
    for w in ws {
        steps.push(RestoreStep::SetMode { workspace: w.real_id, mode: w.mode.clone().unwrap_or_else(|| "dwindle".into()) });
        let mut tiles = w.tiles.clone();
        tiles.sort_by_key(|t| reading_key(t.at));
        steps.extend(tiles.into_iter().map(|tile| RestoreStep::Open { workspace: w.real_id, tile }));
        steps.push(RestoreStep::Layout { workspace: w.real_id });
    }
    steps
}

/// An open window no tile has claimed yet, of the same class and launch command, to
/// move into the saved spot instead of launching a second copy (spec 7.3).
pub fn claim<'a>(tile: &Tile, live: &'a [HClient], claimed: &HashSet<String>, procs: &dyn Fn(u32) -> ProcInfo) -> Option<&'a HClient> {
    live.iter()
        .find(|c| c.class == tile.class && !claimed.contains(&c.address) && procs(c.pid).cmdline == tile.cmdline)
}

/// A relaunch in progress: one step at a time; each window waits on the stash until
/// its workspace's layout step brings the whole workspace in (Ruling, phase 6).
pub(crate) struct Restore {
    steps: VecDeque<RestoreStep>,
    waiting: Option<Waiting>,
    claimed: HashSet<String>,
    /// Per workspace: the live window standing in for each saved tile.
    placed: BTreeMap<i64, Vec<(String, u32, Tile)>>,
    modes: BTreeMap<i64, String>,
    workspaces: Vec<i64>,
}

struct Waiting {
    workspace: i64,
    tile: Tile,
    pid: u32,
    known_before: HashSet<String>,
    deadline: Instant,
}

impl<H: Hypr> Engine<H> {
    /// Hyprland started with relaunch on: the saved steps run one at a time, and the
    /// restoring workspaces are kept from deletion.
    pub(crate) fn begin_restore(&mut self, steps: Vec<RestoreStep>) -> Result<()> {
        let mut workspaces: Vec<i64> = steps
            .iter()
            .map(|s| match s {
                RestoreStep::SetMode { workspace, .. } | RestoreStep::Open { workspace, .. } | RestoreStep::Layout { workspace } => *workspace,
            })
            .collect();
        workspaces.dedup();
        for id in &workspaces {
            self.busy.insert(*id);
        }
        self.restore = Some(Restore {
            steps: steps.into(),
            waiting: None,
            claimed: HashSet::new(),
            placed: BTreeMap::new(),
            modes: BTreeMap::new(),
            workspaces,
        });
        self.advance_restore()
    }

    fn advance_restore(&mut self) -> Result<()> {
        loop {
            let Some(r) = self.restore.as_mut() else { return Ok(()) };
            if r.waiting.is_some() {
                return Ok(());
            }
            let Some(step) = r.steps.pop_front() else { return self.finish_restore() };
            // one failing step (an app that died, a hyprctl hiccup) is skipped: the
            // restore must always reach its end, or every later window goes unrecorded
            let done = match step {
                RestoreStep::SetMode { workspace, mode } => self.restore_mode(workspace, &mode),
                RestoreStep::Open { workspace, tile } => self.restore_open(workspace, tile),
                RestoreStep::Layout { workspace } => self.restore_layout(workspace),
            };
            if let Err(e) = done {
                eprintln!("ymux: restore step skipped: {e:#}");
            }
        }
    }

    /// Sets the mode and writes or deletes the mode file exactly as Omarchy's toggle
    /// does (spec 3.2.2): no file for the default mode.
    fn restore_mode(&mut self, id: i64, mode: &str) -> Result<()> {
        if !self.state.workspaces.contains_key(&id) {
            return Ok(());
        }
        self.hypr.eval(&set_mode(id, mode))?;
        let default = crate::layout::omarchy_default_mode().unwrap_or_else(|| "dwindle".into());
        if mode == default {
            crate::layout::delete_mode_file(&self.layouts_dir, id)?;
        } else {
            crate::layout::write_mode_file(&self.layouts_dir, id, mode)?;
        }
        self.state.workspaces.get_mut(&id).expect("checked").mode = Some(mode.to_string());
        self.restore.as_mut().expect("restoring").modes.insert(id, mode.to_string());
        Ok(())
    }

    /// Claim an open copy, else launch the app (or a placeholder) and wait for it.
    fn restore_open(&mut self, id: i64, tile: Tile) -> Result<()> {
        if !self.state.workspaces.contains_key(&id) {
            return Ok(());
        }
        let live = self.hypr.clients()?;
        let argv: Vec<String> = if tile.is_placeholder() {
            vec![self.exe.clone(), "placeholder".into()]
        } else {
            let claimed = self.restore.as_ref().expect("restoring").claimed.clone();
            let procs = |pid: u32| self.hypr.proc_info(pid);
            if let Some(c) = claim(&tile, &live, &claimed, &procs) {
                let (addr, pid) = (c.address.clone(), c.pid);
                self.hypr.dispatch(&move_to(&addr, STASH))?;
                self.restored(id, addr, pid, tile);
                return Ok(());
            }
            match launch_command(&tile, &self.config.launch_overrides) {
                Some(LaunchCmd::Argv(a)) => a,
                Some(LaunchCmd::Shell(s)) => vec!["sh".into(), "-c".into(), s],
                None => {
                    eprintln!("ymux: no launch command saved for {}; skipped", tile.class);
                    return Ok(());
                }
            }
        };
        let pid = match self.hypr.spawn(&argv, tile.cwd.as_deref().map(Path::new)) {
            Ok(pid) => pid,
            Err(e) => {
                eprintln!("ymux: could not start {}: {e:#}", tile.class);
                self.hypr.notify(&format!("ymux: {} did not come back; a launch-override line may help", tile.class));
                return Ok(());
            }
        };
        if tile.is_placeholder() {
            self.spawned.insert(pid);
        }
        let known_before = live.into_iter().map(|c| c.address).collect();
        self.restore.as_mut().expect("restoring").waiting =
            Some(Waiting { workspace: id, tile, pid, known_before, deadline: Instant::now() + OPEN_WAIT });
        Ok(())
    }

    fn restored(&mut self, id: i64, addr: String, pid: u32, tile: Tile) {
        let r = self.restore.as_mut().expect("restoring");
        r.claimed.insert(addr.clone());
        r.placed.entry(id).or_default().push((addr, pid, tile));
    }

    /// A window opened during a restore: the one being waited for (by pid, or a new
    /// unclaimed window of the same class) goes to the stash; any other is recorded
    /// after the restore. True when the restore took the event.
    pub(crate) fn restore_window_opened(&mut self, address: &str) -> Result<bool> {
        let Some(r) = self.restore.as_ref() else { return Ok(false) };
        let Some(w) = r.waiting.as_ref() else { return Ok(true) };
        let Some(c) = self.hypr.clients()?.into_iter().find(|c| c.address == address) else { return Ok(true) };
        let ours = c.pid == w.pid
            || (c.class == w.tile.class && !r.claimed.contains(address) && !w.known_before.contains(address));
        if !ours {
            return Ok(true);
        }
        let w = self.restore.as_mut().expect("restoring").waiting.take().expect("waiting");
        if c.workspace_name != STASH
            && let Err(e) = self.hypr.dispatch(&move_to(address, STASH))
        {
            eprintln!("ymux: {e:#}");
        }
        self.restored(w.workspace, address.to_string(), c.pid, w.tile);
        self.advance_restore()?;
        Ok(true)
    }

    /// The pid a restore waits for, if any.
    pub(crate) fn restore_pid(&self) -> Option<u32> {
        self.restore.as_ref()?.waiting.as_ref().map(|w| w.pid)
    }

    pub(crate) fn restore_deadline(&self) -> Option<Instant> {
        self.restore.as_ref()?.waiting.as_ref().map(|w| w.deadline)
    }

    /// A launched app that never showed its window (Ruling 19).
    pub(crate) fn expire_restore(&mut self, now: Instant) -> Result<()> {
        // a restore with nothing to wait for must move on (belt and braces)
        if self.restore.as_ref().is_some_and(|r| r.waiting.is_none()) {
            return self.advance_restore();
        }
        if self.restore_deadline().is_none_or(|d| d > now) {
            return Ok(());
        }
        let w = self.restore.as_mut().expect("restoring").waiting.take().expect("waiting");
        eprintln!("ymux: {} did not come back within 15 s", w.tile.class);
        self.hypr.notify(&format!("ymux: {} did not come back; a launch-override line may help", w.tile.class));
        self.advance_restore()
    }

    /// Brings a workspace's windows in from the stash by its mode (spec 7.3), and
    /// records them with their saved geometry.
    fn restore_layout(&mut self, id: i64) -> Result<()> {
        let r = self.restore.as_mut().expect("restoring");
        let placed = r.placed.remove(&id).unwrap_or_default();
        let mode = r.modes.get(&id).cloned().unwrap_or_else(|| "dwindle".into());
        if placed.is_empty() || !self.state.workspaces.contains_key(&id) {
            return Ok(());
        }
        let mut ordered = placed;
        ordered.sort_by_key(|(_, _, t)| reading_key(t.at));
        match mode.as_str() {
            "dwindle" => {
                let tiles: Vec<_> = ordered.iter().map(|(a, _, t)| (a.clone(), t.at, t.size)).collect();
                self.lay_out_dwindle(id, &tiles)?;
            }
            "floating" => {
                for (a, _, t) in &ordered {
                    self.hypr.dispatch(&move_window(a, id, false))?;
                    self.hypr.dispatch(&move_exact(a, t.at.0, t.at.1))?;
                    self.hypr.dispatch(&resize_exact(a, t.size.0, t.size.1))?;
                }
            }
            _ => {
                for (a, _, _) in &ordered {
                    self.hypr.dispatch(&move_window(a, id, false))?;
                }
            }
        }
        for (a, pid, t) in ordered {
            self.remove_tile(&a);
            self.put_tile(id, Tile { address: a, pid: Some(pid), ..t });
        }
        self.dirty = true;
        Ok(())
    }

    /// The restore is done: record every other window, and land each monitor on its
    /// session again (the layout steps moved focus around).
    fn finish_restore(&mut self) -> Result<()> {
        let Some(r) = self.restore.take() else { return Ok(()) };
        for id in r.workspaces {
            self.busy.remove(&id);
        }
        self.record_live_windows()?;
        let monitors = self.hypr.monitors()?;
        self.land(&monitors)?;
        self.view_changed = true;
        self.dirty = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(class: &str, cmd: &[&str], at: (i32, i32)) -> Tile {
        Tile { class: class.into(), cmdline: cmd.iter().map(|s| s.to_string()).collect(), at, ..Default::default() }
    }

    #[test]
    fn override_beats_cmdline() {
        let mut o = BTreeMap::new();
        o.insert("steam".to_string(), "steam -silent".to_string());
        assert_eq!(launch_command(&tile("steam", &["/usr/lib/steam/steam"], (0, 0)), &o), Some(LaunchCmd::Shell("steam -silent".into())));
    }

    #[test]
    fn cmdline_runs_as_argv() {
        let t = tile("foot", &["foot", "--app-id", "x"], (0, 0));
        assert_eq!(launch_command(&t, &BTreeMap::new()), Some(LaunchCmd::Argv(vec!["foot".into(), "--app-id".into(), "x".into()])));
    }

    #[test]
    fn electron_style_single_string_cmdline_is_split() {
        let t = tile("com.anthropic.Claude", &["/usr/lib/nope/claude-desktop --ozone-platform=wayland --x"], (0, 0));
        assert_eq!(
            launch_command(&t, &BTreeMap::new()),
            Some(LaunchCmd::Argv(vec!["/usr/lib/nope/claude-desktop".into(), "--ozone-platform=wayland".into(), "--x".into()]))
        );
    }

    #[test]
    fn placeholder_has_no_launch_command() {
        let t = tile(crate::model::PLACEHOLDER_CLASS, &["ymux", "placeholder"], (0, 0));
        assert_eq!(launch_command(&t, &BTreeMap::new()), None);
        assert_eq!(launch_command(&tile("x", &[], (0, 0)), &BTreeMap::new()), None);
    }

    #[test]
    fn plan_orders_mode_then_tiles_in_reading_order_then_layout() {
        let mut s = State::default();
        s.ensure_monitor("LG A", "DP-1");
        let a = s.create_session("LG A", None);
        let w = s.workspaces.get_mut(&100001).unwrap();
        assert_eq!(w.session, a);
        w.mode = Some("scrolling".into());
        w.tiles = vec![tile("b", &["b"], (900, 0)), tile("a", &["a"], (0, 0))];
        let steps = plan(&s);
        assert_eq!(
            steps,
            vec![
                RestoreStep::SetMode { workspace: 100001, mode: "scrolling".into() },
                RestoreStep::Open { workspace: 100001, tile: tile("a", &["a"], (0, 0)) },
                RestoreStep::Open { workspace: 100001, tile: tile("b", &["b"], (900, 0)) },
                RestoreStep::Layout { workspace: 100001 },
            ]
        );
    }

    #[test]
    fn plan_defaults_the_mode_to_dwindle() {
        let mut s = State::default();
        s.ensure_monitor("LG A", "DP-1");
        s.create_session("LG A", None);
        assert_eq!(plan(&s)[0], RestoreStep::SetMode { workspace: 100001, mode: "dwindle".into() });
    }

    fn live(addr: &str, class: &str, pid: u32) -> HClient {
        HClient {
            address: addr.into(),
            pid,
            class: class.into(),
            title: String::new(),
            workspace: 1,
            workspace_name: "1".into(),
            floating: false,
            at: (0, 0),
            size: (0, 0),
            tags: vec![],
            hidden: false,
            fullscreen: 0,
        }
    }

    #[test]
    fn claim_needs_same_class_and_cmdline_and_unclaimed() {
        let t = tile("kitty", &["kitty"], (0, 0));
        let clients = vec![live("0x1", "foot", 1), live("0x2", "kitty", 2), live("0x3", "kitty", 3)];
        let procs = |pid: u32| ProcInfo { cmdline: if pid == 2 { vec!["kitty".into(), "-1".into()] } else { vec!["kitty".into()] }, cwd: None };
        let none = HashSet::new();
        assert_eq!(claim(&t, &clients, &none, &procs).map(|c| c.address.as_str()), Some("0x3"));
        let taken: HashSet<String> = ["0x3".to_string()].into();
        assert_eq!(claim(&t, &clients, &taken, &procs), None);
    }
}
