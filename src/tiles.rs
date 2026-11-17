//! Placeholders (spec 5.7): jobs that start a placeholder window and put it in place,
//! and the guards that keep workspaces alive while that happens.

use crate::engine::Engine;
use crate::hypr::{close_window, focus_window, move_window, swap_windows, Hypr};
use crate::model::{reading_key, PLACEHOLDER_CLASS};
use anyhow::Result;
use std::time::{Duration, Instant};

const JOB_WAIT: Duration = Duration::from_secs(5);

/// What to do with a placeholder once its window appears on the stash.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum JobKind {
    /// Fill a new, empty workspace (spec 5.7, user decision 2026-11-15).
    FillNew { target: i64 },
    /// Keep the spot an app is leaving, then deliver the app (spec 5.7).
    LeaveSpot { app: String, source: i64, target: i64, follow: bool },
    /// An app closed with Leader + x / Super + W has really gone: a placeholder takes
    /// its spot, and the workspace's layout recorded at the close is restored.
    FillSpot { target: i64, after: Option<String>, app: String, layout: Vec<crate::model::SavedTile> },
    /// Bring a placeholder lost while the daemon was down back next to its old
    /// neighbour (spec 3.2.8).
    Recreate { target: i64, after: Option<String>, refocus: Option<String> },
}

#[derive(Clone, Debug)]
pub(crate) struct PlaceholderJob {
    pub pid: u32,
    pub kind: JobKind,
    pub deadline: Instant,
    /// Ran already; kept until the deadline so the move events of the swap cannot
    /// delete the workspaces involved (busy guard).
    pub settling: bool,
}

const SETTLE: Duration = Duration::from_secs(1);
/// A close asked of an app (Leader + x / Super + W) that has not happened yet: the app
/// stays where it is, answering any question of its own (user decision 2026-11-16).
#[derive(Clone, Debug)]
pub(crate) struct PendingClose {
    pub app: String,
    pub workspace: i64,
    pub layout: Vec<crate::model::SavedTile>,
}

impl JobKind {
    fn workspaces(&self) -> Vec<i64> {
        match self {
            JobKind::FillNew { target } | JobKind::Recreate { target, .. } => vec![*target],
            JobKind::FillSpot { target, .. } => vec![*target],
            JobKind::LeaveSpot { source, target, .. } => vec![*source, *target],
        }
    }
}

impl<H: Hypr> Engine<H> {
    /// Placeholders only live on `dwindle` and `scrolling` workspaces (spec 1 item 5).
    pub(crate) fn wants_placeholder(&self, id: i64) -> Result<bool> {
        Ok(matches!(self.live_mode(id)?.as_str(), "dwindle" | "scrolling"))
    }

    pub(crate) fn spawn_placeholder(&mut self, kind: JobKind) -> Result<()> {
        let pid = self.hypr.spawn(&[self.exe.clone(), "placeholder".into()], None)?;
        self.spawned.insert(pid);
        for id in kind.workspaces() {
            self.busy.insert(id);
        }
        self.jobs.push(PlaceholderJob { pid, kind, deadline: Instant::now() + JOB_WAIT, settling: false });
        Ok(())
    }

    pub fn note_spawned(&mut self, pid: u32) {
        self.spawned.insert(pid);
    }

    /// Spec 2 takeovers 2-3: leave a placeholder where the app was (tiled and
    /// scrolling workspaces), then deliver the app to the target.
    pub fn move_app(&mut self, app: &str, source: i64, target: i64, follow: bool) -> Result<()> {
        // a special workspace (scratchpad, console) keeps no spot; nor does anything
        // with keep-spots off (opt-in, 2026-11-17)
        if self.config.keep_spots && source > 0 && self.wants_placeholder(source)? {
            self.spawn_placeholder(JobKind::LeaveSpot { app: app.into(), source, target, follow })
        } else {
            self.deliver(app, target, follow)
        }
    }

    /// Puts an app on the target: into its first placeholder (top-left first) when
    /// it has one, else a plain move.
    pub(crate) fn deliver(&mut self, app: &str, target: i64, follow: bool) -> Result<()> {
        let ph = if self.wants_placeholder(target)? {
            self.state.workspaces.get(&target).and_then(|w| {
                let mut phs: Vec<_> = w.tiles.iter().filter(|t| t.is_placeholder()).collect();
                phs.sort_by_key(|t| reading_key(t.at));
                phs.first().map(|t| t.address.clone())
            })
        } else {
            None
        };
        let tile = self.tile_for(app)?;
        self.remove_tile(app);
        match ph {
            Some(ph) => {
                self.hypr.dispatch(&swap_windows(&ph, app))?;
                self.hypr.dispatch(&close_window(&ph))?;
                if follow {
                    self.hypr.dispatch(&focus_window(app))?;
                }
            }
            None => self.hypr.dispatch(&move_window(app, target, follow))?,
        }
        self.put_tile(target, tile);
        Ok(())
    }

    /// Spec 5.7: a user-opened tiled app on the workspace of the placeholder that had
    /// focus just before it takes that placeholder's spot.
    pub(crate) fn fill_on_open(&mut self, address: &str, workspace: i64) -> Result<bool> {
        if self.restoring() {
            return Ok(false);
        }
        let Some(c) = self.hypr.clients()?.into_iter().find(|c| c.address == address) else { return Ok(false) };
        if c.class == PLACEHOLDER_CLASS || c.floating || self.spawned.contains(&c.pid) {
            return Ok(false);
        }
        let before = if self.focused.as_deref() == Some(address) { self.prev_focused.clone() } else { self.focused.clone() };
        let Some(ph) = before else { return Ok(false) };
        let on_ws = self.state.workspaces.get(&workspace).is_some_and(|w| w.tiles.iter().any(|t| t.address == ph && t.is_placeholder()));
        if !on_ws {
            return Ok(false);
        }
        self.hypr.dispatch(&swap_windows(&ph, address))?;
        self.hypr.dispatch(&close_window(&ph))?;
        Ok(true)
    }

    /// After a swap the two windows trade workspace and geometry (spikes.md S4).
    pub(crate) fn swap_records(&mut self, a: &str, b: &str) {
        let find = |s: &crate::model::State, addr: &str| {
            s.workspaces.values().find_map(|w| w.tiles.iter().position(|t| t.address == addr).map(|i| (w.real_id, i)))
        };
        let (Some((wa, ia)), Some((wb, ib))) = (find(&self.state, a), find(&self.state, b)) else { return };
        let ta = self.state.workspaces[&wa].tiles[ia].clone();
        let tb = self.state.workspaces[&wb].tiles[ib].clone();
        let (mut na, mut nb) = (ta.clone(), tb.clone());
        (na.at, na.size, nb.at, nb.size) = (tb.at, tb.size, ta.at, ta.size);
        self.state.workspaces.get_mut(&wa).expect("found").tiles[ia] = nb;
        self.state.workspaces.get_mut(&wb).expect("found").tiles[ib] = na;
        self.dirty = true;
    }

    /// True while a relaunch restore runs (spec 7.3): no fill-on-open meanwhile.
    pub fn restoring(&self) -> bool {
        self.restore.is_some()
    }

    /// A new workspace starts with one placeholder filling it, unless it is floating.
    pub fn spawn_fill_placeholder(&mut self, id: i64) -> Result<()> {
        if self.wants_placeholder(id)? {
            self.spawn_placeholder(JobKind::FillNew { target: id })?;
        }
        Ok(())
    }

    /// Runs the job waiting for this window, if any. True when the window was a job's.
    pub(crate) fn run_placeholder_job(&mut self, address: &str) -> Result<bool> {
        let pid = self.hypr.clients()?.into_iter().find(|c| c.address == address).map(|c| c.pid);
        let Some(pid) = pid else { return Ok(false) };
        let Some(pos) = self.jobs.iter().position(|j| j.pid == pid && !j.settling) else { return Ok(false) };
        let job = self.jobs[pos].clone();
        self.jobs[pos].settling = true;
        self.jobs[pos].deadline = Instant::now() + SETTLE;
        match &job.kind {
            JobKind::FillNew { target } => {
                if self.state.workspaces.contains_key(target) {
                    self.hypr.dispatch(&move_window(address, *target, false))?;
                    let tile = self.tile_for(address)?;
                    self.put_tile(*target, tile);
                } else {
                    self.hypr.dispatch(&close_window(address))?;
                }
            }
            JobKind::LeaveSpot { app, source, target, follow } => {
                if self.state.workspaces.contains_key(source) {
                    // the placeholder takes the app's spot, the app goes to the stash
                    self.hypr.dispatch(&swap_windows(address, app))?;
                    let tile = self.tile_for(address)?;
                    self.put_tile(*source, tile);
                } else {
                    self.hypr.dispatch(&close_window(address))?;
                }
                self.deliver(app, *target, *follow)?;
            }
            JobKind::FillSpot { target, after, app, layout } => {
                let refocus = self.focused.clone();
                self.place_recreated(address, *target, after.as_deref(), refocus.as_deref())?;
                // restore the layout as it stood at the close, the placeholder where
                // the app was (spec 5.7; the rebuild of the floating round trip)
                if let Some(w) = self.state.workspaces.get_mut(target) {
                    w.archived_layout = layout
                        .iter()
                        .map(|t| crate::model::SavedTile { address: if &t.address == app { address.into() } else { t.address.clone() }, ..t.clone() })
                        .collect();
                    self.schedule_rearrange(*target);
                }
            }
            JobKind::Recreate { target, after, refocus } => {
                self.place_recreated(address, *target, after.as_deref(), refocus.as_deref())?;
            }
        }
        Ok(true)
    }

    /// Leader + x / Super + W: ask the window to close where it is. A tiled app on a
    /// tiled workspace keeps its spot once it has really closed (`placeholder_for_close`);
    /// until then it stays untouched, so a question it asks first shows in place.
    pub(crate) fn close_app(&mut self, app: &str) -> Result<()> {
        let hit = self
            .state
            .workspaces
            .values()
            .find_map(|w| w.tiles.iter().find(|t| t.address == app).map(|t| (w.real_id, t.is_placeholder() || t.floating)));
        if let Some((ws, false)) = hit
            && self.config.keep_spots
            && self.wants_placeholder(ws)?
        {
            // the layout as it stands now, to give the spot back exactly
            if self.capture_geometry()? {
                self.dirty = true;
            }
            let mut layout: Vec<crate::model::SavedTile> = self.state.workspaces[&ws]
                .tiles
                .iter()
                .map(|t| crate::model::SavedTile { address: t.address.clone(), at: t.at, size: t.size })
                .collect();
            layout.sort_by_key(|t| reading_key(t.at));
            self.pending_close.retain(|p| p.app != app);
            self.pending_close.push(PendingClose { app: app.into(), workspace: ws, layout });
        }
        self.hypr.dispatch(&close_window(app))
    }

    /// The app asked to close has closed: a placeholder comes for its spot.
    pub(crate) fn placeholder_for_close(&mut self, app: &str) -> Result<()> {
        let Some(i) = self.pending_close.iter().position(|p| p.app == app) else { return Ok(()) };
        let p = self.pending_close.remove(i);
        if !self.state.workspaces.contains_key(&p.workspace) || !self.wants_placeholder(p.workspace)? {
            return Ok(());
        }
        let i = p.layout.iter().position(|t| t.address == app);
        let after = i.and_then(|i| i.checked_sub(1)).map(|j| p.layout[j].address.clone());
        self.spawn_placeholder(JobKind::FillSpot { target: p.workspace, after, app: app.into(), layout: p.layout })
    }

    fn release_job(&mut self, job: &PlaceholderJob) -> Result<()> {
        for id in job.kind.workspaces() {
            if !self.jobs.iter().any(|j| j.kind.workspaces().contains(&id)) {
                self.busy.remove(&id);
                self.maybe_delete(id)?;
            }
        }
        Ok(())
    }

    pub(crate) fn expire_placeholder_jobs(&mut self, now: Instant) -> Result<()> {
        let (due, rest): (Vec<PlaceholderJob>, Vec<PlaceholderJob>) =
            std::mem::take(&mut self.jobs).into_iter().partition(|j| j.deadline <= now);
        self.jobs = rest;
        // a placeholder job has settled: record real positions instead of the ones read
        // mid-swap (spec 6.2); a floating round trip saves its layout from these
        if due.iter().any(|j| j.settling) && self.capture_geometry()? {
            self.dirty = true;
        }
        for job in due {
            if let (JobKind::LeaveSpot { app, target, follow, .. }, false) = (&job.kind, job.settling) {
                // the placeholder never came: move the app anyway
                eprintln!("ymux: placeholder did not appear; moving the app without it");
                self.deliver(app, *target, *follow)?;
            }
            self.release_job(&job)?;
        }
        Ok(())
    }
}
