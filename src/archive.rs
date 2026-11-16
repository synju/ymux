//! Placeholders on workspaces that go floating (spec 5.7 "Workspace switched to
//! floating"): parked as 1 x 1 windows in a corner, and every window swapped back
//! into its saved place when Omarchy re-tiles the workspace.

use crate::engine::Engine;
use crate::hypr::{close_window, focus_window, layout_msg, move_exact, move_to, move_window, resize_exact, swap_windows, Hypr};
use crate::model::{reading_key, ArchivedPlaceholder, SavedTile, Tile};
use crate::tiles::JobKind;
use anyhow::Result;

/// Omarchy's tag on every window of a floating workspace (workspace-modes.lua).
const FLOATING_TAG: &str = "omarchy-floating-workspace";

/// A window and the spot it should take: (address, position, size).
pub(crate) type PlacedTile = (String, (i32, i32), (i32, i32));

/// The hidden workspace placeholders start on and a rebuild empties windows onto.
const STASH: &str = "special:ymux-stash";

/// How long after a return to tiled the layout is left to settle before rearranging.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(300);

/// The placeholders among `tiles` in reading order, each with its place
/// (`index` counted from `first`, `after` = the tile just before it among all tiles).
pub(crate) fn archive_entries(tiles: &[Tile], first: usize) -> Vec<(Tile, ArchivedPlaceholder)> {
    let mut ordered: Vec<&Tile> = tiles.iter().collect();
    ordered.sort_by_key(|t| reading_key(t.at));
    let mut out = Vec::new();
    for (i, t) in ordered.iter().enumerate() {
        if t.is_placeholder() {
            let after = i.checked_sub(1).map(|j| ordered[j].address.clone());
            let entry = ArchivedPlaceholder { index: first + out.len(), after, at: t.at, size: t.size };
            out.push(((*t).clone(), entry));
        }
    }
    out
}

impl<H: Hypr> Engine<H> {
    /// Saves the layout from the recorded geometry (never a fresh read: the windows are
    /// already floated) and parks every placeholder 1 x 1 in the monitor's top-left
    /// corner, under the bar.
    pub(crate) fn park(&mut self, id: i64) -> Result<()> {
        let Some(w) = self.state.workspaces.get(&id) else { return Ok(()) };
        let mut layout: Vec<SavedTile> =
            w.tiles.iter().map(|t| SavedTile { address: t.address.clone(), at: t.at, size: t.size }).collect();
        layout.sort_by_key(|t| reading_key(t.at));
        let phs: Vec<String> = w.tiles.iter().filter(|t| t.is_placeholder()).map(|t| t.address.clone()).collect();
        let monitor = self.hypr.workspaces()?.into_iter().find(|x| x.id == id).map(|x| x.monitor);
        let corner = self
            .hypr
            .monitors()?
            .into_iter()
            .find(|m| Some(&m.name) == monitor.as_ref())
            .map_or((0, 0), |m| (m.x, m.y));
        let w = self.state.workspaces.get_mut(&id).expect("checked");
        w.archived_layout = layout;
        w.mode = Some("floating".into());
        self.dirty = true;
        for ph in phs {
            self.hypr.dispatch(&resize_exact(&ph, 1, 1))?;
            self.hypr.dispatch(&move_exact(&ph, corner.0, corner.1))?;
        }
        Ok(())
    }

    /// Restart recreate job: next to its old neighbour, focus given back. Not resized,
    /// so it takes dwindle's split.
    pub(crate) fn place_recreated(&mut self, address: &str, target: i64, after: Option<&str>, refocus: Option<&str>) -> Result<()> {
        if !self.state.workspaces.contains_key(&target) {
            return self.hypr.dispatch(&close_window(address));
        }
        let clients = self.hypr.clients()?;
        if let Some(a) = after
            && clients.iter().any(|c| c.address == a && c.workspace == target)
        {
            self.hypr.dispatch(&focus_window(a))?;
        }
        self.hypr.dispatch(&move_window(address, target, false))?;
        if let Some(r) = refocus {
            self.hypr.dispatch(&focus_window(r))?;
        }
        let tile = self.tile_for(address)?;
        self.put_tile(target, tile);
        self.dirty = true;
        Ok(())
    }

    /// Restores the saved layout once the workspace is tiled again (spec 5.7
    /// "Returning"): sizes only when the arrangement is already right; otherwise, on
    /// `dwindle`, the saved tree is rebuilt window by window; on `scrolling`, windows
    /// are swapped into the spots nearest their saved positions.
    fn rearrange(&mut self, id: i64) -> Result<()> {
        let layout = std::mem::take(&mut self.state.workspaces.get_mut(&id).expect("live").archived_layout);
        if layout.is_empty() {
            return Ok(());
        }
        self.dirty = true;
        let refocus = self.focused.clone();
        let live: Vec<Spot> = self
            .hypr
            .clients()?
            .into_iter()
            .filter(|c| c.workspace == id && !c.floating)
            .map(|c| Spot { address: c.address, at: c.at, size: c.size })
            .collect();
        // saved windows still open, in reading order; windows opened while floating
        let saved: Vec<Spot> = layout
            .iter()
            .filter(|t| live.iter().any(|s| s.address == t.address))
            .map(|t| Spot { address: t.address.clone(), at: t.at, size: t.size })
            .collect();
        let extras: Vec<String> =
            live.iter().filter(|s| !saved.iter().any(|t| t.address == s.address)).map(|s| s.address.clone()).collect();
        if saved.is_empty() {
            return Ok(());
        }
        if extras.is_empty() && same_arrangement(&saved, &live) {
            if sizes_differ(&saved, &live) {
                self.resize_to(&saved)?;
            }
            return Ok(());
        }
        let tiled = self.hypr.workspaces()?.into_iter().find(|w| w.id == id).map(|w| w.tiled_layout);
        match (tiled.as_deref(), build_tree(&saved)) {
            (Some("dwindle"), Some(tree)) => {
                self.rebuild(id, &tree, &saved, &extras)?;
                if extras.is_empty() {
                    self.resize_to(&saved)?;
                }
            }
            _ => self.swap_into_place(&saved, live)?,
        }
        if let Some(r) = refocus {
            self.hypr.dispatch(&focus_window(&r))?;
        }
        Ok(())
    }

    /// Each tile to its saved size, in reading order.
    fn resize_to(&mut self, saved: &[Spot]) -> Result<()> {
        for t in saved {
            self.hypr.dispatch(&resize_exact(&t.address, t.size.0, t.size.1))?;
        }
        Ok(())
    }

    /// Empties the workspace onto the stash and brings the windows back one at a time
    /// so dwindle builds the saved tree (spikes.md "rebuilding a dwindle tree").
    fn rebuild(&mut self, id: i64, tree: &Node, saved: &[Spot], extras: &[String]) -> Result<()> {
        let Err(e) = self.rebuild_steps(id, tree, saved, extras) else { return Ok(()) };
        // never leave windows hidden on the stash: bring every one back, best effort
        eprintln!("ymux: layout rebuild failed, putting the windows back: {e:#}");
        for a in saved.iter().map(|t| &t.address).chain(extras) {
            let _ = self.hypr.dispatch(&move_window(a, id, false));
        }
        Ok(())
    }

    fn rebuild_steps(&mut self, id: i64, tree: &Node, saved: &[Spot], extras: &[String]) -> Result<()> {
        for a in saved.iter().map(|t| &t.address).chain(extras) {
            self.hypr.dispatch(&move_to(a, STASH))?;
        }
        self.hypr.dispatch(&move_window(tree.first(), id, false))?;
        self.grow(id, tree)?;
        for a in extras {
            self.hypr.dispatch(&move_window(a, id, false))?;
        }
        Ok(())
    }

    /// `node`'s first window already fills the node's box: split it as saved.
    fn grow(&mut self, id: i64, node: &Node) -> Result<()> {
        let Node::Split { side_by_side, first, second, size } = node else { return Ok(()) };
        let (a, b) = (first.first(), second.first());
        self.hypr.dispatch(&focus_window(a))?;
        self.hypr.dispatch(&move_window(b, id, false))?;
        // dwindle splits a box side by side when it is wider than tall, else stacked
        if (size.0 > size.1) != *side_by_side {
            self.hypr.dispatch(&focus_window(b))?;
            self.hypr.dispatch(&layout_msg("togglesplit"))?;
        }
        self.grow(id, first)?;
        self.grow(id, second)
    }

    /// Scrolling (and any layout the tree cannot describe): swap each saved window into
    /// the spot nearest its saved position.
    fn swap_into_place(&mut self, saved: &[Spot], mut spots: Vec<Spot>) -> Result<()> {
        let mut fixed: Vec<String> = Vec::new();
        for t in saved {
            let Some(mine) = spots.iter().position(|s| s.address == t.address) else { continue };
            let want = t.centre();
            let Some(other) = spots
                .iter()
                .enumerate()
                .filter(|(_, s)| !fixed.contains(&s.address))
                .min_by_key(|(_, s)| dist(s.centre(), want))
                .map(|(i, _)| i)
            else {
                continue;
            };
            if other != mine {
                let b = spots[other].address.clone();
                self.hypr.dispatch(&swap_windows(&t.address, &b))?;
                self.swap_records(&t.address, &b);
                let (ga, gb) = ((spots[mine].at, spots[mine].size), (spots[other].at, spots[other].size));
                (spots[mine].at, spots[mine].size) = gb;
                (spots[other].at, spots[other].size) = ga;
            }
            fixed.push(t.address.clone());
        }
        Ok(())
    }

    /// Relaunch layout step for `dwindle` (spec 7.3): the windows wait on the stash;
    /// bring them in as the saved tree, then set the saved sizes.
    pub(crate) fn lay_out_dwindle(&mut self, id: i64, tiles: &[PlacedTile]) -> Result<()> {
        let mut spots: Vec<Spot> = tiles.iter().map(|(a, at, size)| Spot { address: a.clone(), at: *at, size: *size }).collect();
        spots.sort_by_key(|s| reading_key(s.at));
        match build_tree(&spots) {
            Some(tree) => {
                self.rebuild(id, &tree, &spots, &[])?;
                self.resize_to(&spots)
            }
            None => {
                for s in &spots {
                    self.hypr.dispatch(&move_window(&s.address, id, false))?;
                }
                Ok(())
            }
        }
    }

    /// Restore a workspace's saved layout once Hyprland has laid it out.
    pub(crate) fn schedule_rearrange(&mut self, id: i64) {
        if !self.rearrange_due.iter().any(|(w, _)| *w == id) {
            self.rearrange_due.push((id, std::time::Instant::now() + SETTLE));
        }
    }

    pub(crate) fn run_due_rearranges(&mut self, now: std::time::Instant) -> Result<()> {
        let (due, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.rearrange_due).into_iter().partition(|(_, t)| *t <= now);
        self.rearrange_due = rest;
        for (id, _) in due {
            if self.state.workspaces.contains_key(&id) {
                self.rearrange(id)?;
            }
        }
        Ok(())
    }

    /// The mode a workspace now reads, and what that means for its placeholders.
    pub(crate) fn sync_archive(&mut self, id: i64, mode: &str) -> Result<()> {
        let Some(w) = self.state.workspaces.get(&id) else { return Ok(()) };
        let parked = !w.archived_layout.is_empty();
        let tiled_before = w.mode.as_deref().is_none_or(|m| m != "floating");
        match mode {
            // save the layout of any tiled workspace going floating (parking its
            // placeholders, if it has any)
            "floating" if !parked && tiled_before && !w.tiles.is_empty() => self.park(id),
            "dwindle" | "scrolling" if parked => {
                // back to tiled: record it now, so geometry capture runs again
                self.state.workspaces.get_mut(&id).expect("checked").mode = Some(mode.to_string());
                self.dirty = true;
                // every window sends an event as it leaves floating, and Hyprland lays
                // them out after: rearrange once, a moment later
                if !self.rearrange_due.iter().any(|(w, _)| *w == id) {
                    self.rearrange_due.push((id, std::time::Instant::now() + SETTLE));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// `changefloatingmode` (spec 5.7 "Noticing"): a window floated with its workspace
    /// saves the layout and parks the placeholders; any window leaving floating may
    /// mean the workspace is tiled again.
    pub(crate) fn on_floating_changed(&mut self, address: &str, floating: bool) -> Result<()> {
        let hit = self
            .state
            .workspaces
            .values()
            .find_map(|w| w.tiles.iter().find(|t| t.address == address).map(|t| (w.real_id, t.is_placeholder())));
        let Some((id, _)) = hit else { return Ok(()) };
        let mode = self.mode_at_event(id)?;
        if floating && mode != "floating" {
            return Ok(()); // one window floated by hand on a tiled workspace
        }
        self.sync_archive(id, &mode)
    }

    /// The mode while Omarchy's toggle is still running: it floats the windows before
    /// it writes its mode file and un-floats them before it removes it, so the file
    /// lags every event. Its tag on the windows does not: tagged means floating,
    /// otherwise Hyprland's own tiled layout (spikes.md, Omarchy workspace-modes.lua).
    fn mode_at_event(&self, id: i64) -> Result<String> {
        let tagged = self
            .hypr
            .clients()?
            .iter()
            .any(|c| c.workspace == id && c.tags.iter().any(|t| t.trim_end_matches('*') == FLOATING_TAG));
        if tagged {
            return Ok("floating".into());
        }
        let tiled = self.hypr.workspaces()?.into_iter().find(|w| w.id == id).map(|w| w.tiled_layout);
        match tiled.as_deref() {
            Some("dwindle") | Some("scrolling") => Ok(tiled.expect("matched")),
            _ => self.live_mode(id),
        }
    }

    /// Restart (spec 3.2.8): saved placeholders with no live window are recreated on
    /// tiled workspaces and dropped on floating ones. `saved` is taken before the tile
    /// records are rebuilt.
    pub(crate) fn restore_lost_placeholders(&mut self, saved: Vec<(i64, Vec<Tile>)>) -> Result<()> {
        let clients = self.hypr.clients()?;
        for (id, tiles) in saved {
            if !self.state.workspaces.contains_key(&id) || !self.wants_placeholder(id)? {
                continue;
            }
            for (t, e) in archive_entries(&tiles, 0) {
                if clients.iter().any(|c| c.address == t.address && Some(c.pid) == t.pid) {
                    continue;
                }
                let refocus = self.focused.clone();
                self.spawn_placeholder(JobKind::Recreate { target: id, after: e.after, refocus })?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct Spot {
    address: String,
    at: (i32, i32),
    size: (i32, i32),
}

impl Spot {
    fn centre(&self) -> (i32, i32) {
        (self.at.0 + self.size.0 / 2, self.at.1 + self.size.1 / 2)
    }
}

fn dist(a: (i32, i32), b: (i32, i32)) -> i64 {
    ((a.0 - b.0) as i64).pow(2) + ((a.1 - b.1) as i64).pow(2)
}

/// Gaps and borders between tiles (spike: about 12 px).
const SLACK: i32 = 40;

fn bounds(spots: &[Spot]) -> ((i32, i32), (i32, i32)) {
    let x0 = spots.iter().map(|s| s.at.0).min().unwrap_or(0);
    let y0 = spots.iter().map(|s| s.at.1).min().unwrap_or(0);
    let x1 = spots.iter().map(|s| s.at.0 + s.size.0).max().unwrap_or(0);
    let y1 = spots.iter().map(|s| s.at.1 + s.size.1).max().unwrap_or(0);
    ((x0, y0), (x1, y1))
}

/// A saved dwindle tree: a window, or a box cut in two (side by side or stacked).
#[derive(Debug, PartialEq)]
enum Node {
    Leaf(String),
    Split { side_by_side: bool, first: Box<Node>, second: Box<Node>, size: (i32, i32) },
}

impl Node {
    /// The window that fills the node's box before it is split.
    fn first(&self) -> &str {
        match self {
            Node::Leaf(a) => a,
            Node::Split { first, .. } => first.first(),
        }
    }
}

/// Reads the split tree off saved rectangles: the cut (vertical, then horizontal) that
/// puts every rectangle wholly on one side, recursively. `None` when there is none.
fn build_tree(spots: &[Spot]) -> Option<Node> {
    match spots {
        [] => None,
        [one] => Some(Node::Leaf(one.address.clone())),
        _ => {
            let ((x0, y0), (x1, y1)) = bounds(spots);
            for side_by_side in [true, false] {
                let cuts: Vec<i32> = spots.iter().map(|s| if side_by_side { s.at.0 + s.size.0 } else { s.at.1 + s.size.1 }).collect();
                for cut in cuts {
                    let span = |s: &Spot| if side_by_side { (s.at.0, s.at.0 + s.size.0) } else { (s.at.1, s.at.1 + s.size.1) };
                    let first: Vec<Spot> = spots.iter().filter(|s| span(s).1 <= cut + SLACK).cloned().collect();
                    let second: Vec<Spot> = spots.iter().filter(|s| span(s).0 >= cut - SLACK).cloned().collect();
                    if first.is_empty() || second.is_empty() || first.len() + second.len() != spots.len() {
                        continue;
                    }
                    return Some(Node::Split {
                        side_by_side,
                        first: Box::new(build_tree(&first)?),
                        second: Box::new(build_tree(&second)?),
                        size: (x1 - x0, y1 - y0),
                    });
                }
            }
            None
        }
    }
}

/// True when any window's size is off its saved one by more than the gaps.
fn sizes_differ(saved: &[Spot], live: &[Spot]) -> bool {
    saved.iter().any(|t| {
        live.iter()
            .find(|s| s.address == t.address)
            .is_none_or(|s| (s.size.0 - t.size.0).abs() > SLACK || (s.size.1 - t.size.1).abs() > SLACK)
    })
}

/// How `a` sits relative to `b`: left of, right of, above, below, or overlapping
/// (gaps allowed), on each axis.
fn relation(a: &Spot, b: &Spot) -> (i8, i8) {
    let axis = |a0: i32, a1: i32, b0: i32, b1: i32| {
        if a1 <= b0 + SLACK {
            -1
        } else if b1 <= a0 + SLACK {
            1
        } else {
            0
        }
    };
    (
        axis(a.at.0, a.at.0 + a.size.0, b.at.0, b.at.0 + b.size.0),
        axis(a.at.1, a.at.1 + a.size.1, b.at.1, b.at.1 + b.size.1),
    )
}

/// True when every pair of windows sits the same way round in both layouts (same
/// tree shape, split ratios aside).
fn same_arrangement(saved: &[Spot], live: &[Spot]) -> bool {
    let find = |addr: &str| live.iter().find(|s| s.address == addr);
    saved.iter().enumerate().all(|(i, a)| {
        saved[i + 1..].iter().all(|b| match (find(&a.address), find(&b.address)) {
            (Some(la), Some(lb)) => relation(a, b) == relation(la, lb),
            _ => false,
        })
    })
}
