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
    /// Leader + x on an app: the placeholder takes its spot, then the app is closed;
    /// `ph` is set once swapped in (user decision 2026-11-15).
    CloseApp { app: String, source: i64, ph: Option<String> },
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
/// How long a closed app gets before it is put back (unsaved-changes dialogs).
const CLOSE_WAIT: Duration = Duration::from_secs(3);

impl JobKind {
    fn workspaces(&self) -> Vec<i64> {
        match self {
            JobKind::FillNew { target } | JobKind::Recreate { target, .. } => vec![*target],
            JobKind::CloseApp { source, .. } => vec![*source],
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
        // a special workspace (scratchpad, console) keeps no spot
        if source > 0 && self.wants_placeholder(source)? {
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
            JobKind::CloseApp { app, source, .. } => {
                let live = self.hypr.clients()?.iter().any(|c| &c.address == app);
                if live && self.state.workspaces.contains_key(source) {
                    self.hypr.dispatch(&swap_windows(address, app))?;
                    self.hypr.dispatch(&close_window(app))?;
                    self.remove_tile(app);
                    let tile = self.tile_for(address)?;
                    self.put_tile(*source, tile);
                    let job = &mut self.jobs[pos];
                    job.kind = JobKind::CloseApp { app: app.clone(), source: *source, ph: Some(address.into()) };
                    job.deadline = Instant::now() + CLOSE_WAIT;
                } else {
                    self.hypr.dispatch(&close_window(address))?;
                }
            }
            JobKind::Recreate { target, after, refocus } => {
                self.place_recreated(address, *target, after.as_deref(), refocus.as_deref())?;
            }
        }
        Ok(true)
    }

    /// Leader + x / Super + W: close the window; a tiled app keeps its spot on tiled
    /// workspaces, anything else (placeholder, floating, untracked) closes at once.
    pub(crate) fn close_app(&mut self, app: &str) -> Result<()> {
        let hit = self
            .state
            .workspaces
            .values()
            .find_map(|w| w.tiles.iter().find(|t| t.address == app).map(|t| (w.real_id, t.is_placeholder() || t.floating)));
        match hit {
            Some((source, false)) if self.wants_placeholder(source)? => {
                self.spawn_placeholder(JobKind::CloseApp { app: app.into(), source, ph: None })
            }
            _ => self.hypr.dispatch(&close_window(app)),
        }
    }

    /// An app still open after it was asked to close goes back into its spot.
    fn put_back(&mut self, app: &str, source: i64, ph: &str) -> Result<()> {
        let Some(c) = self.hypr.clients()?.into_iter().find(|c| c.address == app) else { return Ok(()) };
        self.hypr.dispatch(&swap_windows(app, ph))?;
        self.hypr.dispatch(&close_window(ph))?;
        self.remove_tile(ph);
        if self.state.workspaces.contains_key(&source) {
            let tile = self.tile_for(app)?;
            self.put_tile(source, tile);
        }
        self.hypr.notify(&format!("ymux: {} did not close; it is back in its spot", c.class));
        Ok(())
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
            match (&job.kind, job.settling) {
                (JobKind::LeaveSpot { app, target, follow, .. }, false) => {
                    // the placeholder never came: move the app anyway
                    eprintln!("ymux: placeholder did not appear; moving the app without it");
                    self.deliver(app, *target, *follow)?;
                }
                (JobKind::CloseApp { app, .. }, false) => {
                    eprintln!("ymux: placeholder did not appear; closing the app without it");
                    self.hypr.dispatch(&close_window(app))?;
                }
                (JobKind::CloseApp { app, source, ph: Some(ph) }, true) => self.put_back(app, *source, ph)?,
                _ => {}
            }
            self.release_job(&job)?;
        }
        Ok(())
    }
}
