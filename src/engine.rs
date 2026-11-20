//! The ymux engine: owns the state and turns requests and Hyprland events into
//! state changes and Hyprland dispatches (spec 2, 3.2, 5.6).

use crate::config::Config;
use crate::events::Event;
use crate::hypr::{change_id, close_window, focus_monitor, focus_window, focus_workspace, move_workspace, HMonitor, Hypr};
use crate::layout::{delete_mode_file, move_mode_file};
use crate::model::{State, Tile, Workspace};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A kill from the tree waiting for its windows to close (spec 5.6).
pub(crate) struct KillJob {
    pub label: String,
    pub addresses: Vec<String>,
    pub deadline: Instant,
    /// Monitor key to switch to another session once done (killing an active session).
    pub switch_monitor: Option<(String, u32)>,
}

const KILL_WAIT: Duration = Duration::from_secs(10);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum Request {
    Focus { n: u32 },
    Move { n: u32, follow: bool },
    Next,
    Prev,
    Previous,
    NewWorkspace,
    NewSession { name: Option<String> },
    SwitchSession { number: u32 },
    SessionMove { dir: char },
    Action { name: String },
    Status,
    /// Shows a workspace by real id (bar widget clicks), focusing its monitor first.
    FocusId { id: i64 },
    /// Rename prompt results (spec 5.2).
    RenameSession { number: u32, name: String },
    RenameWorkspace { id: i64, name: String },
    /// Name a monitor (by its key: make, model, serial) or a window; empty clears it.
    RenameMonitor { key: String, name: String },
    RenameTile { address: String, name: String },
    FocusWindow { address: String },
    CloseWindow { address: String },
    /// Alt + Tab / Alt + Shift + Tab, skipping placeholders (spec 2 takeover 8).
    Cycle { next: bool },
    FocusMonitor { connector: String },
    /// Command prompt line (spec 5.3).
    Command { line: String },
    SetRelaunch { on: bool },
    /// Moving or closing an app leaves its spot open (opt-in).
    SetKeepSpots { on: bool },
    /// Empty workspaces and sessions close by themselves (opt-in).
    SetAutoClose { on: bool },
    /// Move a workspace into another session (lowest free number there).
    MoveWorkspaceToSession { id: i64, session: u32 },
    /// Remember a Leader + w tree row as folded or unfolded.
    Fold { key: String, folded: bool },
    KillSession { number: u32 },
    KillWorkspace { id: i64 },
    /// Leader + `.` (tmux move-window): give a workspace another number in its session.
    RenumberWorkspace { id: i64, number: u32 },
    /// Stops the daemon (used by install before switching builds). Handled by the
    /// daemon loop, never by the engine.
    Shutdown,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Response {
    pub ok: bool,
    pub message: Option<String>,
}

impl Response {
    pub fn ok() -> Self {
        Response { ok: true, message: None }
    }
    pub fn info(msg: String) -> Self {
        Response { ok: true, message: Some(msg) }
    }
    pub fn err(msg: String) -> Self {
        Response { ok: false, message: Some(msg) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartKind {
    /// No saved state: the open workspaces become one `main` session per monitor.
    Adopted,
    /// Same Hyprland session: the daemon restarted while Hyprland kept running.
    Restarted,
    /// A new Hyprland session: land each monitor on its session (then relaunch, phase 6).
    HyprlandStarted,
}

pub struct Engine<H: Hypr> {
    pub state: State,
    pub config: Config,
    pub(crate) hypr: H,
    pub(crate) signature: String,
    pub(crate) layouts_dir: PathBuf,
    pub(crate) spawned: HashSet<u32>,
    pub(crate) dirty: bool,
    /// Current Hyprland submap ("" when none), for the LEADER / RESIZE marker.
    pub(crate) submap: String,
    pub(crate) focused: Option<String>,
    pub(crate) prev_focused: Option<String>,
    pub(crate) view_changed: bool,
    /// Set by monitor and workspace-move events; the daemon runs the invariant once
    /// these settle (spec 3.2.5, docs/spikes.md S1).
    pub invariant_pending: bool,
    /// (old, new) for every renumber this engine dispatched.
    pub(crate) renumber_log: Vec<(i64, i64)>,
    /// `~/.ymux.conf`; tests point it elsewhere.
    pub config_path: PathBuf,
    pub(crate) kills: Vec<KillJob>,
    /// The ymux binary placeholders are started from; tests overwrite it.
    pub exe: String,
    pub(crate) jobs: Vec<crate::tiles::PlaceholderJob>,
    /// Workspaces back from floating, rearranged once Hyprland has laid them out
    /// (spec 5.7 "Returning").
    pub(crate) rearrange_due: Vec<(i64, Instant)>,
    /// Closes asked with Leader + x / Super + W that have not happened yet.
    pub(crate) pending_close: Vec<crate::tiles::PendingClose>,
    /// When a window last closed; the restore snapshot waits for closes to stop (a
    /// shutdown closes every window).
    pub last_close: Option<Instant>,
    /// A relaunch on Hyprland start in progress (spec 7.3).
    pub(crate) restore: Option<crate::relaunch::Restore>,
    /// Workspaces a placeholder job is working on: never deleted mid-job.
    pub(crate) busy: HashSet<i64>,
    /// Workspaces asked to close (tree or options card): removed once empty and left,
    /// whatever auto-close says.
    pub(crate) doomed: HashSet<i64>,
}

impl<H: Hypr> Engine<H> {
    pub fn new(state: State, config: Config, hypr: H, signature: String, layouts_dir: PathBuf) -> Self {
        Engine {
            state,
            config,
            hypr,
            signature,
            layouts_dir,
            spawned: HashSet::new(),
            dirty: false,
            submap: String::new(),
            focused: None,
            prev_focused: None,
            view_changed: false,
            invariant_pending: false,
            renumber_log: Vec::new(),
            config_path: crate::config::config_path(),
            kills: Vec::new(),
            exe: crate::daemon::installed_exe().display().to_string(),
            jobs: Vec::new(),
            rearrange_due: Vec::new(),
            pending_close: Vec::new(),
            restore: None,
            last_close: None,
            busy: HashSet::new(),
            doomed: HashSet::new(),
        }
    }

    pub fn hypr(&self) -> &H {
        &self.hypr
    }

    pub fn layouts_dir(&self) -> &std::path::Path {
        &self.layouts_dir
    }

    /// The view for the watch stream (one `hyprctl -j monitors` call).
    pub fn view(&self) -> Result<crate::view::View> {
        let monitors = self.hypr.monitors()?;
        let live = crate::view::Live {
            connected: monitors.iter().map(|m| (m.name.clone(), m.description.clone())).collect(),
            focused_connector: monitors.iter().find(|m| m.focused).map(|m| m.name.clone()),
            focused_address: self.focused.clone(),
            submap: self.submap.clone(),
            relaunch: self.config.relaunch,
            keep_spots: self.config.keep_spots,
            auto_close: self.config.auto_close,
        };
        Ok(crate::view::build(&self.state, &live))
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// True once after anything only the view shows changed (submap, focus).
    pub fn take_view_changed(&mut self) -> bool {
        std::mem::take(&mut self.view_changed)
    }


    fn focused_monitor(&self) -> Result<HMonitor> {
        self.hypr
            .monitors()?
            .into_iter()
            .find(|m| m.focused)
            .ok_or_else(|| anyhow!("no focused monitor"))
    }

    /// The active session of the focused monitor.
    pub fn focused_session(&self) -> Result<u32> {
        let m = self.focused_monitor()?;
        self.state
            .monitor(&m.description)
            .and_then(|s| s.active_session)
            .ok_or_else(|| anyhow!("no session on {}", m.name))
    }

    /// Makes `session` its monitor's active session and bumps its recency.
    pub(crate) fn activate(&mut self, session: u32) {
        let next = self.state.sessions.values().map(|s| s.last_used).max().unwrap_or(0) + 1;
        let monitor = {
            let s = self.state.sessions.get_mut(&session).expect("known session");
            s.last_used = next;
            s.monitor.clone()
        };
        if let Some(m) = self.state.monitor_mut(&monitor) {
            m.active_session = Some(session);
        }
        self.dirty = true;
    }

    /// Opens one of the plugin's overlays (spec 5).
    pub(crate) fn summon(&self, payload: serde_json::Value) -> Result<()> {
        self.hypr.spawn(&crate::overlay::summon_argv(&payload), None).map(|_| ())
    }

    pub(crate) fn focus(&self, id: i64) -> Result<()> {
        self.hypr.dispatch(&focus_workspace(id))
    }

    /// Real id of the workspace shown on the focused monitor, if ymux knows it.
    fn current(&self) -> Result<Option<i64>> {
        let id = self.focused_monitor()?.active_workspace;
        Ok(self.state.workspaces.contains_key(&id).then_some(id))
    }

    fn step(&self, forward: bool) -> Result<()> {
        let session = self.focused_session()?;
        let Some(cur) = self.current()? else { return Ok(()) };
        let cur_n = self.state.workspaces[&cur].number;
        let list = self.state.session_workspaces(session);
        // wraps around at either end, like tmux next-window / previous-window
        let target = if forward {
            list.iter().find(|w| w.number > cur_n).or(list.first())
        } else {
            list.iter().rev().find(|w| w.number < cur_n).or(list.last())
        }
        .filter(|w| w.number != cur_n);
        if let Some(w) = target {
            self.focus(w.real_id)?;
        }
        Ok(())
    }

    pub fn handle(&mut self, req: Request) -> Response {
        match self.try_handle(req) {
            Ok(r) => r,
            Err(e) => Response::err(format!("ymux: {e}")),
        }
    }

    fn try_handle(&mut self, req: Request) -> Result<Response> {
        match req {
            Request::Focus { n } => {
                let session = self.focused_session()?;
                if let Some(w) = self.state.workspace_by_number(session, n) {
                    self.focus(w.real_id)?;
                }
            }
            Request::Move { n, follow } => {
                let session = self.focused_session()?;
                let Some(addr) = self.hypr.active_address()? else { return Ok(Response::ok()) };
                let Some(c) = self.hypr.clients()?.into_iter().find(|c| c.address == addr) else {
                    return Ok(Response::ok());
                };
                if c.class == crate::model::PLACEHOLDER_CLASS {
                    return Ok(Response::ok());
                }
                if self.state.workspace_by_number(session, n).is_some_and(|w| w.real_id == c.workspace) {
                    return Ok(Response::ok());
                }
                // a missing workspace is created, like tmux move-window to a free index
                let id = match self.state.workspace_by_number(session, n) {
                    Some(w) => w.real_id,
                    None => {
                        self.dirty = true;
                        self.state.create_workspace_numbered(session, n)
                    }
                };
                self.move_app(&addr, c.workspace, id, follow)?;
            }
            Request::Next => self.step(true)?,
            Request::Prev => self.step(false)?,
            Request::Previous => {
                let session = self.focused_session()?;
                let prev = self.current()?.and_then(|c| self.state.workspaces[&c].previous);
                if let Some(p) = prev.filter(|p| self.state.workspaces.get(p).is_some_and(|w| w.session == session)) {
                    self.focus(p)?;
                }
            }
            Request::NewWorkspace => {
                let session = self.focused_session()?;
                let id = self.state.create_workspace(session);
                self.dirty = true;
                self.focus(id)?;
                self.spawn_fill_placeholder(id)?;
            }
            Request::NewSession { name } => {
                let m = self.focused_monitor()?;
                self.state.ensure_monitor(&m.description, &m.name);
                let s = self.state.create_session(&m.description, name.as_deref());
                self.activate(s);
                let id = self.state.session_workspaces(s)[0].real_id;
                self.focus(id)?;
                self.spawn_fill_placeholder(id)?;
            }
            Request::SwitchSession { number } => {
                let Some(sess) = self.state.sessions.get(&number) else {
                    return Ok(Response::err(format!("ymux: no session {number}")));
                };
                let target = sess
                    .last_shown
                    .filter(|id| self.state.workspaces.contains_key(id))
                    .or_else(|| self.state.session_workspaces(number).first().map(|w| w.real_id));
                let monitor_key = sess.monitor.clone();
                let connector = self.state.monitor(&monitor_key).and_then(|m| m.connector.clone());
                self.activate(number);
                if let Some(id) = target {
                    let focused = self.focused_monitor()?;
                    if focused.description != monitor_key
                        && let Some(c) = connector
                    {
                        self.hypr.dispatch(&focus_monitor(&c))?;
                    }
                    self.focus(id)?;
                }
            }
            Request::SessionMove { dir } => self.session_move(dir)?,
            Request::RenameSession { number, name } => {
                let name = name.trim();
                if name.is_empty() {
                    return Ok(Response::err("session name cannot be empty".into()));
                }
                let Some(s) = self.state.sessions.get_mut(&number) else {
                    return Ok(Response::err(format!("ymux: no session {number}")));
                };
                s.name = name.to_string();
                self.dirty = true;
            }
            Request::RenameWorkspace { id, name } => {
                let Some(w) = self.state.workspaces.get_mut(&id) else {
                    return Ok(Response::err(format!("ymux: no workspace {id}")));
                };
                let name = name.trim();
                w.name = (!name.is_empty()).then(|| name.to_string());
                self.dirty = true;
            }
            Request::RenameMonitor { key, name } => {
                let Some(m) = self.state.monitor_mut(&key) else {
                    return Ok(Response::err(format!("ymux: no monitor {key}")));
                };
                let name = name.trim();
                m.name = (!name.is_empty()).then(|| name.to_string());
                self.dirty = true;
                self.view_changed = true;
            }
            Request::RenameTile { address, name } => {
                let Some(t) = self.state.workspaces.values_mut().flat_map(|w| w.tiles.iter_mut()).find(|t| t.address == address) else {
                    return Ok(Response::err(format!("ymux: no window {address}")));
                };
                let name = name.trim();
                t.name = (!name.is_empty()).then(|| name.to_string());
                self.dirty = true;
                self.view_changed = true;
            }
            Request::RenumberWorkspace { id, number } => {
                if number == 0 {
                    return Ok(Response::err("the number must be 1 or higher".into()));
                }
                let Some(session) = self.state.workspaces.get(&id).map(|w| w.session) else {
                    return Ok(Response::err(format!("ymux: no workspace {id}")));
                };
                if self.state.workspace_by_number(session, number).is_some_and(|w| w.real_id != id) {
                    return Ok(Response::err(format!("workspace {number} already exists")));
                }
                self.state.workspaces.get_mut(&id).expect("checked").number = number;
                self.dirty = true;
            }
            Request::Action { name } if matches!(name.as_str(), "tree" | "command-prompt" | "options") => {
                let view = match name.as_str() {
                    "tree" => "tree",
                    "command-prompt" => "command",
                    _ => "options",
                };
                self.summon(serde_json::json!({ "view": view }))?;
            }
            Request::Action { name } if name == "help" => self.summon(crate::help::help_payload(&self.config))?,
            Request::Action { name } if name == "display-panes" => {
                let panes = crate::overlay::number_panes(&self.hypr.monitors()?, &self.hypr.clients()?);
                self.summon(serde_json::json!({
                    "view": "panes",
                    "timeout": self.config.display_panes_ms,
                    "panes": panes,
                }))?;
            }
            Request::FocusWindow { address } => {
                let Some(c) = self.hypr.clients()?.into_iter().find(|c| c.address == address) else {
                    return Ok(Response::err(format!("ymux: no window {address}")));
                };
                let on = self.hypr.workspaces()?.into_iter().find(|w| w.id == c.workspace).map(|w| w.monitor);
                if let Some(name) = on
                    && self.focused_monitor()?.name != name
                {
                    self.hypr.dispatch(&focus_monitor(&name))?;
                }
                self.hypr.dispatch(&focus_window(&address))?;
            }
            Request::CloseWindow { address } => self.close_app(&address)?,
            Request::Action { name } if name == "mark" => {
                // one mark at a time, like tmux select-pane -m; again clears it
                if let Some(addr) = self.hypr.active_address()? {
                    self.state.mark = if self.state.mark.as_deref() == Some(addr.as_str()) { None } else { Some(addr) };
                    self.dirty = true;
                    self.view_changed = true;
                }
            }
            Request::Action { name } if name == "swap" => {
                let clients = self.hypr.clients()?;
                let Some(mark) = self.state.mark.clone().filter(|m| clients.iter().any(|c| &c.address == m)) else {
                    return Ok(Response::err("ymux: no marked tile".into()));
                };
                let Some(focused) = self.hypr.active_address()? else { return Ok(Response::ok()) };
                if focused != mark {
                    // Hyprland will not swap a fullscreen or maximized window: leave that
                    // state, swap, and give it back, also when the swap fails
                    let states: Vec<(String, &str)> = [&mark, &focused]
                        .into_iter()
                        .filter_map(|a| {
                            let state = clients.iter().find(|c| &c.address == a)?.fullscreen;
                            crate::hypr::fullscreen_mode(state).map(|m| (a.clone(), m))
                        })
                        .collect();
                    for (a, m) in &states {
                        self.hypr.dispatch(&crate::hypr::fullscreen(a, m, "unset"))?;
                    }
                    let swapped = self.hypr.dispatch(&crate::hypr::swap_windows(&mark, &focused));
                    for (a, m) in &states {
                        self.hypr.dispatch(&crate::hypr::fullscreen(a, m, "set"))?;
                    }
                    swapped?;
                    self.swap_records(&mark, &focused);
                    // the marked window is "pasted" where you are: give it the focus
                    // (user request 2026-11-16)
                    self.hypr.dispatch(&crate::hypr::focus_window(&mark))?;
                }
                self.state.mark = None;
                self.dirty = true;
                self.view_changed = true;
            }
            Request::Action { name } if name == "kill-tile" => {
                // no confirmation (user decision 2026-11-15); a tiled app keeps its spot
                if let Some(addr) = self.hypr.active_address()? {
                    self.close_app(&addr)?;
                }
            }
            Request::FocusMonitor { connector } => self.hypr.dispatch(&focus_monitor(&connector))?,
            Request::Cycle { next } => {
                // the focused workspace's windows in Hyprland's client order,
                // without placeholders or hidden windows; wraps at either end
                let ws = self.focused_monitor()?.active_workspace;
                let list: Vec<String> = self
                    .hypr
                    .clients()?
                    .into_iter()
                    .filter(|c| c.workspace == ws && !c.hidden && c.class != crate::model::PLACEHOLDER_CLASS)
                    .map(|c| c.address)
                    .collect();
                if list.is_empty() {
                    return Ok(Response::ok());
                }
                let cur = self.hypr.active_address()?.and_then(|a| list.iter().position(|x| *x == a));
                let i = match (cur, next) {
                    (Some(i), true) => (i + 1) % list.len(),
                    (Some(i), false) => (i + list.len() - 1) % list.len(),
                    (None, _) => 0,
                };
                if cur == Some(i) {
                    return Ok(Response::ok());
                }
                // keep Omarchy's raise behaviour (tiling.lua:57-60)
                if !next {
                    self.hypr.dispatch("hl.dsp.window.alter_zorder({ mode = \"bottom\" })")?;
                }
                self.hypr.dispatch(&focus_window(&list[i]))?;
                if next {
                    self.hypr.dispatch("hl.dsp.window.bring_to_top()")?;
                }
            }
            Request::Command { line } => {
                let line = line.trim();
                let mut words = line.split_whitespace();
                match words.next() {
                    None => {}
                    Some("new") => {
                        let rest = words.collect::<Vec<_>>().join(" ");
                        return self.try_handle(Request::NewSession { name: (!rest.is_empty()).then_some(rest) });
                    }
                    Some(_) => return Ok(Response::err(format!("unknown command: {line}"))),
                }
            }
            Request::Fold { key, folded } => {
                let changed = if folded { self.state.folded.insert(key) } else { self.state.folded.remove(&key) };
                if changed {
                    self.dirty = true;
                    self.view_changed = true;
                }
            }
            Request::SetAutoClose { on } => {
                let text = std::fs::read_to_string(&self.config_path).unwrap_or_else(|_| crate::config::default_text());
                crate::store::write_atomic(&self.config_path, &crate::config::set_toggle_line(&text, "auto-close", on))?;
                self.config.auto_close = on;
                self.view_changed = true;
            }
            Request::MoveWorkspaceToSession { id, session } => return self.move_workspace_to_session(id, session),
            Request::SetKeepSpots { on } => {
                let text = std::fs::read_to_string(&self.config_path).unwrap_or_else(|_| crate::config::default_text());
                crate::store::write_atomic(&self.config_path, &crate::config::set_toggle_line(&text, "keep-spots", on))?;
                self.config.keep_spots = on;
                self.view_changed = true;
            }
            Request::SetRelaunch { on } => {
                let text = std::fs::read_to_string(&self.config_path).unwrap_or_else(|_| crate::config::default_text());
                crate::store::write_atomic(&self.config_path, &crate::config::set_relaunch_line(&text, on))?;
                self.config.relaunch = on;
                self.view_changed = true;
            }
            Request::KillSession { number } => {
                let Some(s) = self.state.sessions.get(&number) else {
                    return Ok(Response::err(format!("ymux: no session {number}")));
                };
                let label = s.name.clone();
                let monitor = s.monitor.clone();
                let active = self.state.monitor(&monitor).and_then(|m| m.active_session) == Some(number);
                let ids: Vec<i64> = self.state.session_workspaces(number).iter().map(|w| w.real_id).collect();
                self.start_kill(label, ids, active.then_some((monitor, number)))?;
            }
            Request::KillWorkspace { id } => {
                let Some(w) = self.state.workspaces.get(&id) else {
                    return Ok(Response::err(format!("ymux: no workspace {id}")));
                };
                let label = crate::view::workspace_label(w);
                self.start_kill(label, vec![id], None)?;
            }
            Request::Action { name } if name == "move-workspace" => {
                let Some(id) = self.current()? else {
                    return Ok(Response::err("ymux: this workspace is not managed by ymux".into()));
                };
                let current = self.state.workspaces[&id].number.to_string();
                self.summon(serde_json::json!({"view":"renumber","id":id,"current":current}))?;
            }
            Request::Action { name } if name == "rename-session" => {
                let s = self.focused_session()?;
                let current = self.state.sessions[&s].name.clone();
                self.summon(serde_json::json!({"view":"rename","target":"session","id":s,"current":current}))?;
            }
            Request::Action { name } if name == "rename-workspace" => {
                let Some(id) = self.current()? else {
                    return Ok(Response::err("ymux: this workspace is not managed by ymux".into()));
                };
                let current = self.state.workspaces[&id].name.clone().unwrap_or_default();
                self.summon(serde_json::json!({"view":"rename","target":"workspace","id":id,"current":current}))?;
            }
            Request::Action { name } => {
                let req = match name.as_str() {
                    "new-workspace" => Request::NewWorkspace,
                    "previous-workspace" => Request::Prev,
                    "next-workspace" => Request::Next,
                    _ => return Ok(Response::err(format!("ymux: unknown action {name}"))),
                };
                return self.try_handle(req);
            }
            Request::Status => return Ok(Response::info(self.status())),
            Request::FocusId { id } => {
                if let Some(ws) = self.state.workspaces.get(&id) {
                    let key = self.state.sessions[&ws.session].monitor.clone();
                    let connector = self.state.monitor(&key).and_then(|m| m.connector.clone());
                    if self.focused_monitor()?.description != key
                        && let Some(c) = connector
                    {
                        self.hypr.dispatch(&focus_monitor(&c))?;
                    }
                    self.focus(id)?;
                }
            }
            Request::Shutdown => {}
        }
        Ok(Response::ok())
    }

    fn status(&self) -> String {
        let mut out = String::new();
        for m in &self.state.monitors {
            let active = m.active_session.map(|n| n.to_string()).unwrap_or_else(|| "-".into());
            out += &format!(
                "{} {} (active session {active})\n",
                m.connector.as_deref().unwrap_or("?"),
                m.key
            );
            for s in self.state.sessions.values().filter(|s| s.monitor == m.key) {
                let ws: Vec<String> = self
                    .state
                    .session_workspaces(s.number)
                    .iter()
                    .map(|w| format!("{}={}", w.number, w.real_id))
                    .collect();
                out += &format!("  {}: {} workspaces {}\n", s.number, s.name, ws.join(" "));
            }
        }
        out
    }

    // ---- events (spec 3.2.6, 3.2.7, 3.2.10, 5.6) ---------------------------------

    pub fn on_event(&mut self, ev: Event) -> Result<()> {
        match ev {
            Event::OpenWindow { address, workspace_name, .. } => {
                if self.restore_window_opened(&address)? {
                    return Ok(());
                }
                if self.run_placeholder_job(&address)? {
                    return Ok(());
                }
                let Ok(id) = workspace_name.parse::<i64>() else { return Ok(()) };
                if id <= 0 {
                    return Ok(());
                }
                let tile = self.tile_for(&address)?;
                self.place_tile(id, tile)?;
                self.fill_on_open(&address, id)?;
            }
            Event::CloseWindow { address } => {
                self.last_close = Some(Instant::now());
                // a close asked with Leader + x / Super + W happened: its spot stays (the
                // placeholder job keeps the workspace alive meanwhile)
                self.placeholder_for_close(&address)?;
                if self.state.mark.as_deref() == Some(address.as_str()) {
                    self.state.mark = None;
                    self.dirty = true;
                    self.view_changed = true;
                }
                if let Some(src) = self.remove_tile(&address) {
                    self.maybe_delete(src)?;
                }
                self.kill_progress(&address)?;
            }
            Event::MoveWindow { address, workspace, .. } => {
                // a window moved before it closed leaves no spot behind
                self.pending_close.retain(|p| p.app != address);
                // a given name goes with the window
                let name = self.state.workspaces.values().flat_map(|w| &w.tiles).find(|t| t.address == address).and_then(|t| t.name.clone());
                let old = self.remove_tile(&address);
                if workspace > 0 {
                    let tile = Tile { name, ..self.tile_for(&address)? };
                    self.place_tile(workspace, tile)?;
                }
                if let Some(src) = old.filter(|s| *s != workspace) {
                    self.maybe_delete(src)?;
                }
            }
            Event::Workspace { id } => self.on_shown(id)?,
            Event::CreateWorkspace { .. }
            | Event::MoveWorkspace { .. }
            | Event::MonitorAdded { .. }
            | Event::MonitorRemoved { .. } => self.invariant_pending = true,
            Event::ChangeWorkspaceId { old, new } => self.on_change_id(old, new)?,
            Event::Submap { name } => {
                self.submap = name;
                self.view_changed = true;
            }
            Event::ActiveWindow { address } => self.on_focus(address),
            Event::FloatingChanged { address, floating } => self.on_floating_changed(&address, floating)?,
            Event::DestroyWorkspace { .. } | Event::ConfigReloaded | Event::Other => {}
        }
        Ok(())
    }

    /// Focus moved: remember it (fill-on-open needs the previous one) and name the
    /// workspace after the focused app (spec 1 item 3; never after a placeholder).
    fn on_focus(&mut self, address: Option<String>) {
        self.prev_focused = std::mem::replace(&mut self.focused, address.clone());
        self.view_changed = true;
        let Some(addr) = address else { return };
        let hit = self
            .state
            .workspaces
            .values()
            .find_map(|w| w.tiles.iter().find(|t| t.address == addr).map(|t| (w.real_id, t.clone())));
        if let Some((id, t)) = hit
            && !t.is_placeholder()
        {
            let w = self.state.workspaces.get_mut(&id).expect("found");
            if w.auto_name.as_deref() != Some(t.class.as_str()) {
                w.auto_name = Some(t.class);
                self.dirty = true;
            }
        }
    }

    /// A tile record for a live window (pid and geometry from `hyprctl -j clients`).
    pub(crate) fn tile_for(&self, address: &str) -> Result<Tile> {
        let c = self.hypr.clients()?.into_iter().find(|c| c.address == address);
        Ok(match c {
            Some(c) => {
                // recorded whatever the relaunch toggle says (spec 6.5, 7.2)
                let proc = self.hypr.proc_info(c.pid);
                Tile {
                    address: c.address,
                    pid: Some(c.pid),
                    class: c.class,
                    title: c.title,
                    floating: c.floating,
                    at: c.at,
                    size: c.size,
                    cmdline: proc.cmdline,
                    cwd: proc.cwd,
                    name: None,
                }
            }
            None => Tile { address: address.to_string(), ..Default::default() },
        })
    }

    fn place_tile(&mut self, id: i64, tile: Tile) -> Result<()> {
        if !self.state.workspaces.contains_key(&id) {
            if tile.pid.is_some_and(|p| self.spawned.contains(&p)) {
                return Ok(());
            }
            let Some(adopted) = self.adopt_unknown(id)? else { return Ok(()) };
            return self.place_tile(adopted, tile);
        }
        let ws = self.state.workspaces.get_mut(&id).expect("checked");
        ws.tiles.retain(|t| t.address != tile.address);
        ws.tiles.push(tile);
        self.dirty = true;
        Ok(())
    }

    /// Removes a window's tile record; returns the workspace it was on.
    /// Records a tile on a known workspace (replacing any record of the same window).
    pub(crate) fn put_tile(&mut self, id: i64, tile: Tile) {
        if let Some(ws) = self.state.workspaces.get_mut(&id) {
            ws.tiles.retain(|t| t.address != tile.address);
            ws.tiles.push(tile);
            self.dirty = true;
        }
    }

    pub(crate) fn remove_tile(&mut self, address: &str) -> Option<i64> {
        let id = self
            .state
            .workspaces
            .values()
            .find(|w| w.tiles.iter().any(|t| t.address == address))?
            .real_id;
        self.state.workspaces.get_mut(&id)?.tiles.retain(|t| t.address != address);
        self.dirty = true;
        Some(id)
    }

    /// Adopts a regular workspace unknown to ymux into the active session of the
    /// monitor it is on (spec 3.2.10). Returns the id it is known under.
    fn adopt_unknown(&mut self, id: i64) -> Result<Option<i64>> {
        let Some(live) = self.hypr.workspaces()?.into_iter().find(|w| w.id == id) else {
            return Ok(None);
        };
        let Some(mon) = self.hypr.monitors()?.into_iter().find(|m| m.name == live.monitor) else {
            return Ok(None);
        };
        self.state.ensure_monitor(&mon.description, &mon.name);
        let session = match self.state.monitor(&mon.description).and_then(|m| m.active_session) {
            Some(s) => s,
            None => {
                let s = match self.most_recent_session_on(&mon.description) {
                    Some(s) => s,
                    None => self.state.create_session(&mon.description, Some("main")),
                };
                self.activate(s);
                s
            }
        };
        let number = self.state.next_workspace_number(session);
        self.state.workspaces.insert(
            id,
            Workspace { real_id: id, session, number, ..Default::default() },
        );
        self.dirty = true;
        self.adopted(id)
    }

    /// A freshly adopted workspace is renumbered into its monitor's ymux range, so
    /// no ymux workspace keeps an id inside an Omarchy block (spec 3.2.8, 3.2.10).
    fn adopted(&mut self, id: i64) -> Result<Option<i64>> {
        Ok(Some(self.move_into_range(id)?))
    }

    /// Renumbers `id` into its session monitor's range if it is outside it.
    fn move_into_range(&mut self, id: i64) -> Result<i64> {
        let session = self.state.workspaces[&id].session;
        let key = self.state.sessions[&session].monitor.clone();
        let slot = self.state.monitor(&key).map(|m| m.slot).unwrap_or(0);
        if State::range_for_slot(slot).contains(&id) {
            return Ok(id);
        }
        let new = self.state.alloc_real_id(&key);
        self.renumber(id, new)?;
        Ok(new)
    }

    /// Renumbers a workspace in Hyprland, moves its mode file, and rekeys the state.
    pub fn renumber(&mut self, old: i64, new: i64) -> Result<()> {
        self.hypr.dispatch(&change_id(old, new))?;
        // a folded workspace stays folded under its new id
        if self.state.folded.remove(&format!("w:{old}")) {
            self.state.folded.insert(format!("w:{new}"));
        }
        move_mode_file(&self.layouts_dir, old, new)?;
        // Omarchy keys the live mode by id too; re-apply it under the new id
        let file = self.layouts_dir.join(format!("{new}.lua"));
        if file.exists() {
            let mode = crate::layout::read_mode(&self.layouts_dir, new, None, || None);
            self.hypr.eval(&crate::hypr::set_mode(new, &mode))?;
        }
        self.rekey(old, new);
        self.renumber_log.push((old, new));
        Ok(())
    }

    fn rekey(&mut self, old: i64, new: i64) {
        let Some(mut ws) = self.state.workspaces.remove(&old) else { return };
        ws.real_id = new;
        self.state.workspaces.insert(new, ws);
        if self.state.folded.remove(&format!("w:{old}")) {
            self.state.folded.insert(format!("w:{new}"));
        }
        for w in self.state.workspaces.values_mut() {
            if w.previous == Some(old) {
                w.previous = Some(new);
            }
        }
        for s in self.state.sessions.values_mut() {
            if s.last_shown == Some(old) {
                s.last_shown = Some(new);
            }
        }
        for m in &mut self.state.monitors {
            if m.shown == Some(old) {
                m.shown = Some(new);
            }
        }
        self.dirty = true;
    }

    fn shown_ids(&self) -> Result<Vec<i64>> {
        Ok(self.hypr.monitors()?.iter().map(|m| m.active_workspace).collect())
    }

    /// True while a window minimized from `id` waits on `special:minimized` (spec 1 item 6).
    fn minimized_from(&self, id: i64) -> Result<bool> {
        let tag = format!("omarchy-minimized-from-{id}");
        Ok(self
            .hypr
            .clients()?
            .iter()
            .any(|c| c.workspace_name == "special:minimized" && c.tags.iter().any(|t| t == &tag)))
    }

    /// Event-driven deletion (spec 5.6): an empty workspace that is not shown goes.
    pub(crate) fn maybe_delete(&mut self, id: i64) -> Result<()> {
        let Some(ws) = self.state.workspaces.get(&id) else { return Ok(()) };
        if self.busy.contains(&id) || !ws.tiles.is_empty() || self.shown_ids()?.contains(&id) || self.minimized_from(id)? {
            return Ok(());
        }
        // with auto-close off (the default) only a workspace asked to close goes
        if !self.config.auto_close && !self.doomed.contains(&id) {
            return Ok(());
        }
        self.delete_workspace(id)
    }

    /// The single deletion path for workspaces (Task 8 adds the mode file).
    fn delete_workspace(&mut self, id: i64) -> Result<()> {
        let Some(ws) = self.state.workspaces.remove(&id) else { return Ok(()) };
        self.dirty = true;
        self.doomed.remove(&id);
        self.state.folded.remove(&format!("w:{id}"));
        delete_mode_file(&self.layouts_dir, id)?;
        for w in self.state.workspaces.values_mut() {
            if w.previous == Some(id) {
                w.previous = None;
            }
        }
        if let Some(s) = self.state.sessions.get_mut(&ws.session)
            && s.last_shown == Some(id)
        {
            s.last_shown = None;
        }
        if self.state.session_workspaces(ws.session).is_empty() {
            self.delete_session(ws.session);
        }
        Ok(())
    }

    pub(crate) fn delete_session(&mut self, number: u32) {
        let Some(s) = self.state.sessions.remove(&number) else { return };
        self.dirty = true;
        self.state.folded.remove(&format!("s:{number}"));
        let was_active = self.state.monitor(&s.monitor).and_then(|m| m.active_session) == Some(number);
        if was_active {
            let next = self.most_recent_session_on(&s.monitor);
            match next {
                Some(n) => self.activate(n),
                None => {
                    if let Some(m) = self.state.monitor_mut(&s.monitor) {
                        m.active_session = None;
                    }
                }
            }
        }
    }

    fn most_recent_session_on(&self, monitor: &str) -> Option<u32> {
        self.state
            .sessions
            .values()
            .filter(|s| s.monitor == monitor)
            .max_by_key(|s| s.last_used)
            .map(|s| s.number)
    }

    /// The focused monitor now shows `id` (spec 3.2.6: focus from outside switches
    /// session; previous pointer; leaving an empty workspace deletes it).
    fn on_shown(&mut self, id: i64) -> Result<()> {
        // the monitor that shows `id` is the one the live workspace is on; the focused
        // monitor is only a fallback (bursts during session moves and landing)
        let owner = self.hypr.workspaces()?.into_iter().find(|w| w.id == id).map(|w| w.monitor);
        let m = match owner.and_then(|name| self.hypr.monitors().ok()?.into_iter().find(|m| m.name == name)) {
            Some(m) => m,
            None => self.focused_monitor()?,
        };
        let key = m.description.clone();
        self.state.ensure_monitor(&key, &m.name);
        let prev_session = self.state.monitor(&key).and_then(|m| m.active_session);
        let left = self.state.monitor(&key).and_then(|m| m.shown);
        if let Some(mon) = self.state.monitor_mut(&key) {
            mon.shown = (id > 0).then_some(id);
        }
        if let Some(ws) = self.state.workspaces.get(&id).cloned() {
            if let Some(l) = left.filter(|l| *l != id && self.state.workspaces.get(l).is_some_and(|w| w.session == ws.session)) {
                self.state.workspaces.get_mut(&id).expect("known").previous = Some(l);
            }
            if let Some(s) = self.state.sessions.get_mut(&ws.session) {
                s.last_shown = Some(id);
            }
            self.dirty = true;
            if prev_session != Some(ws.session) && self.state.sessions[&ws.session].monitor == key {
                self.activate(ws.session);
            }
        }
        if let Some(l) = left.filter(|l| *l != id) {
            self.maybe_delete(l)?;
        }
        Ok(())
    }

    /// Outside renumbering (spec 3.2.9), guarded: only when the old id is gone and the
    /// new id is not mapped.
    fn on_change_id(&mut self, old: i64, new: i64) -> Result<()> {
        if !self.state.workspaces.contains_key(&old) || self.state.workspaces.contains_key(&new) {
            return Ok(());
        }
        if self.hypr.workspaces()?.iter().any(|w| w.id == old) {
            return Ok(());
        }
        move_mode_file(&self.layouts_dir, old, new)?;
        self.rekey(old, new);
        // back into ymux's range, normally to the very id it had
        let key = self.state.sessions[&self.state.workspaces[&new].session].monitor.clone();
        let slot = self.state.monitor(&key).map(|m| m.slot).unwrap_or(0);
        if !State::range_for_slot(slot).contains(&new) {
            let back = if State::range_for_slot(slot).contains(&old) { old } else { self.state.alloc_real_id(&key) };
            self.renumber(new, back)?;
        }
        Ok(())
    }

    /// The 30 s check (spec 3.2.9 fallback): a known workspace gone from Hyprland whose
    /// windows now sit under an unmapped regular id was renumbered from outside.
    pub fn tick(&mut self) -> Result<()> {
        let live_ids: Vec<i64> = self.hypr.workspaces()?.iter().map(|w| w.id).collect();
        let clients = self.hypr.clients()?;
        // forget spawned pids whose windows are gone and nothing waits for, so a
        // recycled pid is not mistaken for a placeholder (final review)
        let waiting: Vec<u32> = self.jobs.iter().map(|j| j.pid).chain(self.restore_pid()).collect();
        self.spawned.retain(|p| waiting.contains(p) || clients.iter().any(|c| c.pid == *p));
        // windows no event told us about (e.g. mapped before socket2 connected); during
        // a restore they are recorded by its final sweep instead (spec 7.3)
        let restoring = self.restoring();
        for c in clients.iter().filter(|_| !restoring) {
            let known = self.state.workspaces.values().any(|w| w.tiles.iter().any(|t| t.address == c.address));
            if !known && c.workspace > 0 && !self.spawned.contains(&c.pid) && live_ids.contains(&c.workspace) {
                let tile = self.tile_for(&c.address)?;
                self.place_tile(c.workspace, tile)?;
            }
        }
        // spec 6.2 order: mode check (and archiving, phase 5) before geometry capture
        self.check_modes()?;
        if self.capture_geometry()? {
            self.dirty = true;
        }
        let clients = self.hypr.clients()?;
        let gone: Vec<i64> = self.state.workspaces.keys().copied().filter(|id| !live_ids.contains(id)).collect();
        for old in gone {
            let addrs: Vec<String> = self.state.workspaces[&old].tiles.iter().map(|t| t.address.clone()).collect();
            let new = clients
                .iter()
                .find(|c| addrs.contains(&c.address) && c.workspace > 0 && !self.state.workspaces.contains_key(&c.workspace))
                .map(|c| c.workspace);
            if let Some(new) = new {
                self.on_change_id(old, new)?;
            }
        }
        self.enforce_invariant()
    }

    // ---- monitor invariant (spec 3.2.5) -------------------------------------------

    pub fn enforce_invariant(&mut self) -> Result<()> {
        self.invariant_pending = false;
        let monitors = self.hypr.monitors()?;
        let live_ws = self.hypr.workspaces()?;
        for m in &monitors {
            self.state.ensure_monitor(&m.description, &m.name);
        }
        let connected: Vec<&HMonitor> = monitors.iter().collect();
        let is_connected = |key: &str| connected.iter().any(|m| m.description == key);
        let focused_key = connected.iter().find(|m| m.focused).map(|m| m.description.clone());

        // 1. sessions of a gone monitor join the monitor holding most of their workspaces
        let orphans: Vec<u32> = self
            .state
            .sessions
            .values()
            .filter(|s| !is_connected(&s.monitor))
            .map(|s| s.number)
            .collect();
        for n in orphans {
            let mut counts: Vec<(String, usize)> = Vec::new();
            for w in self.state.session_workspaces(n) {
                if let Some(l) = live_ws.iter().find(|l| l.id == w.real_id)
                    && let Some(m) = connected.iter().find(|m| m.name == l.monitor)
                {
                    match counts.iter_mut().find(|(k, _)| *k == m.description) {
                        Some((_, c)) => *c += 1,
                        None => counts.push((m.description.clone(), 1)),
                    }
                }
            }
            let best = counts.iter().map(|(_, c)| *c).max();
            let target = match best {
                Some(b) => {
                    let ties: Vec<&String> = counts.iter().filter(|(_, c)| *c == b).map(|(k, _)| k).collect();
                    focused_key.as_ref().filter(|f| ties.contains(f)).cloned().unwrap_or_else(|| ties[0].clone())
                }
                None => match &focused_key {
                    Some(f) => f.clone(),
                    None => continue,
                },
            };
            let old = self.state.sessions[&n].monitor.clone();
            if let Some(m) = self.state.monitor_mut(&old)
                && m.active_session == Some(n)
            {
                m.active_session = None;
            }
            self.state.sessions.get_mut(&n).expect("orphan").monitor = target;
            self.dirty = true;
        }

        // 2. workspaces sitting on another connected monitor go back to their session's
        for w in self.state.workspaces.values() {
            let Some(l) = live_ws.iter().find(|l| l.id == w.real_id) else { continue };
            let owner_key = &self.state.sessions[&w.session].monitor;
            let Some(owner) = connected.iter().find(|m| &m.description == owner_key) else { continue };
            if l.monitor != owner.name {
                self.hypr.dispatch(&move_workspace(w.real_id, &owner.name))?;
            }
        }

        // 3. a connected monitor with no sessions gets `main`; keep an active session
        for m in &connected {
            let has = self.state.sessions.values().any(|s| s.monitor == m.description);
            if !has {
                let s = self.state.create_session(&m.description, Some("main"));
                self.activate(s);
                let id = self.state.session_workspaces(s)[0].real_id;
                self.spawn_fill_placeholder(id)?;
            } else if self.state.monitor(&m.description).and_then(|x| x.active_session).is_none()
                && let Some(n) = self.most_recent_session_on(&m.description)
            {
                self.activate(n);
            }
        }
        Ok(())
    }

    // ---- session move (spec 2, takeover 7) -----------------------------------------

    fn session_move(&mut self, dir: char) -> Result<()> {
        let monitors = self.hypr.monitors()?;
        let Some(src) = monitors.iter().find(|m| m.focused).cloned() else { return Ok(()) };
        let target = monitors
            .iter()
            .filter(|m| m.name != src.name)
            .filter(|m| match dir {
                'r' => m.x > src.x,
                'l' => m.x < src.x,
                'd' => m.y > src.y,
                'u' => m.y < src.y,
                _ => false,
            })
            .min_by_key(|m| (m.x - src.x).abs() + (m.y - src.y).abs())
            .cloned();
        let Some(dst) = target else { return Ok(()) };
        let session = self.focused_session()?;
        let live: Vec<i64> = self.hypr.workspaces()?.iter().map(|w| w.id).collect();
        let ids: Vec<i64> = self.state.session_workspaces(session).iter().map(|w| w.real_id).collect();
        for id in &ids {
            if live.contains(id) {
                self.hypr.dispatch(&move_workspace(*id, &dst.name))?;
            }
        }
        self.state.ensure_monitor(&dst.description, &dst.name);
        self.state.sessions.get_mut(&session).expect("focused").monitor = dst.description.clone();
        if let Some(m) = self.state.monitor_mut(&src.description) {
            m.active_session = None;
        }
        // refill the source monitor
        let refill = match self.most_recent_session_on(&src.description) {
            Some(n) => n,
            None => self.state.create_session(&src.description, Some("main")),
        };
        self.activate(refill);
        if let Some(id) = self.show_target(refill) {
            self.hypr.dispatch(&focus_monitor(&src.name))?;
            self.focus(id)?;
        }
        // the moved session is shown on the target, and focus goes with it
        self.activate(session);
        if let Some(id) = self.show_target(session) {
            self.hypr.dispatch(&focus_monitor(&dst.name))?;
            self.focus(id)?;
        }
        Ok(())
    }

    /// The workspace to show for a session: its last shown, else its lowest.
    pub(crate) fn show_target(&self, session: u32) -> Option<i64> {
        self.state
            .sessions
            .get(&session)?
            .last_shown
            .filter(|id| self.state.workspaces.contains_key(id))
            .or_else(|| self.state.session_workspaces(session).first().map(|w| w.real_id))
    }

    // ---- start (spec 3.2.3, 3.2.8) ----------------------------------------------------

    pub fn start(&mut self) -> Result<StartKind> {
        let kind = if self.state.sessions.is_empty() {
            StartKind::Adopted
        } else if self.state.signature.as_deref() == Some(self.signature.as_str()) {
            StartKind::Restarted
        } else {
            StartKind::HyprlandStarted
        };
        let monitors = self.hypr.monitors()?;
        for m in &monitors {
            self.state.ensure_monitor(&m.description, &m.name);
        }
        // placeholders left on the stash by an earlier daemon (Ruling 13); app windows
        // left there by an interrupted rebuild or restore go back where they were
        // recorded, else to the focused monitor's workspace
        let fallback = monitors.iter().find(|m| m.focused).or(monitors.first()).map(|m| m.active_workspace);
        for c in self.hypr.clients()? {
            if c.workspace_name != "special:ymux-stash" {
                continue;
            }
            if c.class == crate::model::PLACEHOLDER_CLASS {
                self.hypr.dispatch(&close_window(&c.address))?;
                continue;
            }
            let recorded =
                self.state.workspaces.values().find(|w| w.tiles.iter().any(|t| t.address == c.address)).map(|w| w.real_id);
            if let Some(target) = recorded.or(fallback).filter(|id| *id > 0) {
                self.hypr.dispatch(&crate::hypr::move_window(&c.address, target, false))?;
            }
        }
        match kind {
            StartKind::Adopted => self.adopt_all(&monitors)?,
            StartKind::Restarted => {
                let had_tiles: Vec<i64> =
                    self.state.workspaces.values().filter(|w| !w.tiles.is_empty()).map(|w| w.real_id).collect();
                let saved: Vec<(i64, Vec<Tile>)> =
                    self.state.workspaces.values().map(|w| (w.real_id, w.tiles.clone())).collect();
                self.clear_tiles();
                self.record_live_windows()?;
                // given names survive a daemon restart (same window: address and pid)
                for t in saved.iter().flat_map(|(_, ts)| ts).filter(|t| t.name.is_some()) {
                    if let Some(live) = self.state.workspaces.values_mut().flat_map(|w| w.tiles.iter_mut()).find(|l| l.address == t.address && l.pid == t.pid) {
                        live.name = t.name.clone();
                    }
                }
                self.restore_lost_placeholders(saved)?;
                // dropping a workspace's last saved tile counts as that tile closing;
                // workspaces that were already empty are kept (spec 5.6)
                for id in had_tiles {
                    self.maybe_delete(id)?;
                }
            }
            StartKind::HyprlandStarted => {
                // the saved tiles are the restore plan; with relaunch off only sessions,
                // names and empty workspaces come back (spec 7.5)
                let steps = crate::relaunch::plan(&self.state);
                self.clear_tiles();
                self.land(&monitors)?;
                if self.config.relaunch {
                    self.begin_restore(steps)?;
                } else {
                    self.record_live_windows()?;
                }
            }
        }
        self.enforce_invariant()?;
        self.state.signature = Some(self.signature.clone());
        self.dirty = true;
        Ok(kind)
    }

    fn clear_tiles(&mut self) {
        for w in self.state.workspaces.values_mut() {
            w.tiles.clear();
        }
    }

    /// Records every live window on a regular workspace. A window the daemon spawned
    /// itself is placed by the daemon instead (spec 3.2.10).
    pub(crate) fn record_live_windows(&mut self) -> Result<()> {
        for c in self.hypr.clients()? {
            if c.workspace <= 0 || self.spawned.contains(&c.pid) {
                continue;
            }
            // a window the restore already recorded keeps one record
            if self.state.workspaces.values().any(|w| w.tiles.iter().any(|t| t.address == c.address)) {
                continue;
            }
            let tile = self.tile_for(&c.address)?;
            self.place_tile(c.workspace, tile)?;
        }
        Ok(())
    }

    /// First run: each monitor's occupied workspaces become session `main`, keeping
    /// their slot number (12 -> 2), renumbered into the monitor's range (spec 3.2.3).
    fn adopt_all(&mut self, monitors: &[HMonitor]) -> Result<()> {
        let live = self.hypr.workspaces()?;
        for m in monitors {
            let mut ids: Vec<i64> = live
                .iter()
                .filter(|w| w.monitor == m.name && w.id > 0 && w.windows > 0)
                .map(|w| w.id)
                .collect();
            ids.sort();
            let session = self.state.next_session_number();
            self.state.sessions.insert(
                session,
                crate::model::Session {
                    number: session,
                    name: "main".into(),
                    monitor: m.description.clone(),
                    last_shown: None,
                    last_used: 0,
                },
            );
            for id in ids {
                let slot_number = (id - ((id - 1) / 10) * 10) as u32;
                let number = if self.state.workspace_by_number(session, slot_number).is_none() {
                    slot_number
                } else {
                    self.state.next_workspace_number(session)
                };
                self.state.workspaces.insert(
                    id,
                    Workspace { real_id: id, session, number, ..Default::default() },
                );
                let new = self.state.alloc_real_id(&m.description);
                self.renumber(id, new)?;
                if m.active_workspace == id {
                    self.state.sessions.get_mut(&session).expect("new").last_shown = Some(new);
                    if let Some(mon) = self.state.monitor_mut(&m.description) {
                        mon.shown = Some(new);
                    }
                }
            }
            if self.state.session_workspaces(session).is_empty() {
                self.state.create_workspace(session);
            }
            self.activate(session);
        }
        // tiles: live windows still report the pre-adoption ids
        for c in self.hypr.clients()? {
            if c.workspace <= 0 {
                continue;
            }
            // Hyprland may already report the new id, or still the old one
            let id = if self.state.workspaces.contains_key(&c.workspace) {
                Some(c.workspace)
            } else {
                self.adopted_id(c.workspace)
            };
            if let Some(id) = id {
                let tile = self.tile_for(&c.address)?;
                if let Some(w) = self.state.workspaces.get_mut(&id) {
                    w.tiles.push(tile);
                }
            }
        }
        Ok(())
    }

    /// The ymux id a pre-adoption Omarchy id was renumbered to, via the renumber log.
    fn adopted_id(&self, old: i64) -> Option<i64> {
        self.renumber_log.iter().rev().find(|(o, _)| *o == old).map(|(_, n)| *n)
    }

    /// After a new Hyprland start, each monitor shows its active session's last-shown
    /// workspace before anything else happens (spec 3.2.8 Landing).
    pub(crate) fn land(&mut self, monitors: &[HMonitor]) -> Result<()> {
        for m in monitors {
            let session = match self.state.monitor(&m.description).and_then(|x| x.active_session) {
                Some(s) if self.state.sessions.contains_key(&s) => Some(s),
                _ => self.most_recent_session_on(&m.description),
            };
            let Some(session) = session else { continue };
            self.activate(session);
            let Some(target) = self.show_target(session) else { continue };
            self.hypr.dispatch(&focus_monitor(&m.name))?;
            self.focus(target)?;
            self.state.sessions.get_mut(&session).expect("known").last_shown = Some(target);
            if let Some(mon) = self.state.monitor_mut(&m.description) {
                mon.shown = Some(target);
            }
        }
        Ok(())
    }

    // ---- kills from the tree (spec 5.6) and deadlines ------------------------------

    /// Asks every window in the workspaces to close and deletes the ones that are
    /// already empty and not shown; the rest go as their windows actually close.
    fn start_kill(&mut self, label: String, ids: Vec<i64>, switch: Option<(String, u32)>) -> Result<()> {
        let mut addresses = Vec::new();
        for id in &ids {
            if let Some(w) = self.state.workspaces.get(id) {
                for t in &w.tiles {
                    addresses.push(t.address.clone());
                }
            }
        }
        for a in &addresses {
            self.hypr.dispatch(&close_window(a))?;
        }
        self.doomed.extend(ids.iter().copied());
        // killing drops a saved floating layout (spec 5.6, 5.7)
        for id in &ids {
            if let Some(w) = self.state.workspaces.get_mut(id) {
                w.archived_layout.clear();
            }
        }
        let shown = self.shown_ids()?;
        for id in &ids {
            if self.state.workspaces.get(id).is_some_and(|w| w.tiles.is_empty()) && !shown.contains(id) {
                self.delete_workspace(*id)?;
            }
        }
        let job = KillJob { label, addresses, deadline: Instant::now() + KILL_WAIT, switch_monitor: switch };
        if job.addresses.is_empty() {
            self.finish_kill(job)
        } else {
            self.kills.push(job);
            Ok(())
        }
    }

    fn kill_progress(&mut self, closed: &str) -> Result<()> {
        for j in &mut self.kills {
            j.addresses.retain(|a| a != closed);
        }
        let (done, rest): (Vec<KillJob>, Vec<KillJob>) = std::mem::take(&mut self.kills).into_iter().partition(|j| j.addresses.is_empty());
        self.kills = rest;
        for j in done {
            self.finish_kill(j)?;
        }
        Ok(())
    }

    /// Killing the monitor's active session switches it to its most recently used
    /// other session, or to a new `main` (spec 5.6).
    fn finish_kill(&mut self, job: KillJob) -> Result<()> {
        let Some((monitor, killed)) = job.switch_monitor else { return self.leave_doomed() };
        let next = self
            .state
            .sessions
            .values()
            .filter(|s| s.monitor == monitor && s.number != killed)
            .max_by_key(|s| s.last_used)
            .map(|s| s.number);
        let next = match next {
            Some(n) => n,
            None => self.state.create_session(&monitor, Some("main")),
        };
        self.activate(next);
        if let Some(target) = self.show_target(next) {
            if let Some(c) = self.state.monitor(&monitor).and_then(|m| m.connector.clone()) {
                self.hypr.dispatch(&focus_monitor(&c))?;
            }
            self.focus(target)?;
        }
        Ok(())
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.kills
            .iter()
            .map(|j| j.deadline)
            .chain(self.jobs.iter().map(|j| j.deadline))
            .chain(self.rearrange_due.iter().map(|(_, t)| *t))
            .chain(self.restore_deadline())
            .min()
    }

    /// Timed jobs: a kill still waiting after 10 s names what is still open.
    pub fn on_deadline(&mut self, now: Instant) -> Result<()> {
        self.expire_placeholder_jobs(now)?;
        self.run_due_rearranges(now)?;
        self.expire_restore(now)?;
        let (due, rest): (Vec<KillJob>, Vec<KillJob>) = std::mem::take(&mut self.kills).into_iter().partition(|j| j.deadline <= now);
        self.kills = rest;
        if due.is_empty() {
            return Ok(());
        }
        let clients = self.hypr.clients()?;
        for job in due {
            let open: Vec<String> = clients
                .iter()
                .filter(|c| job.addresses.contains(&c.address))
                .map(|c| format!("{} \"{}\"", c.class, c.title))
                .collect();
            if !open.is_empty() {
                self.hypr.notify(&format!("ymux: still open after 10 s: {}", open.join(", ")));
            }
            let _ = job.label;
        }
        Ok(())
    }
}

