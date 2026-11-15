//! Layout-mode tracking and geometry capture (spec 3.2.11, 6.2).

use crate::engine::Engine;
use crate::hypr::{HWorkspace, Hypr};
use crate::layout::{omarchy_default_mode, read_mode};
use anyhow::Result;

impl<H: Hypr> Engine<H> {
    /// A workspace's mode exactly as Omarchy resolves it.
    pub(crate) fn live_mode(&self, id: i64) -> Result<String> {
        let list = self.hypr.workspaces()?;
        let default = std::cell::OnceCell::new();
        Ok(self.mode_in(&list, &default, id))
    }

    /// The mode from one workspace list read and one default-script run per pass (a
    /// pass covers every workspace; the review found a hyprctl call per workspace).
    fn mode_in(&self, list: &[HWorkspace], default: &std::cell::OnceCell<Option<String>>, id: i64) -> String {
        let live = list.iter().find(|w| w.id == id).map(|w| w.tiled_layout.as_str()).filter(|s| !s.is_empty());
        read_mode(&self.layouts_dir, id, live, || default.get_or_init(omarchy_default_mode).clone())
    }

    /// Records every known workspace's live mode.
    pub fn check_modes(&mut self) -> Result<()> {
        let list = self.hypr.workspaces()?;
        let clients = self.hypr.clients()?;
        let default = std::cell::OnceCell::new();
        let ids: Vec<i64> = self.state.workspaces.keys().copied().collect();
        for id in ids {
            // Omarchy tags floated windows before it writes its mode file, so a check
            // landing in that gap must trust the tag (spec 5.7 "Noticing")
            let tagged = clients.iter().any(|c| {
                c.workspace == id && c.tags.iter().any(|t| t.trim_end_matches('*') == "omarchy-floating-workspace")
            });
            let mode = if tagged { "floating".to_string() } else { self.mode_in(&list, &default, id) };
            // the 30 s fallback for a missed floating event (spec 5.7 "Noticing"); it
            // needs the mode recorded before this change
            self.sync_archive(id, &mode)?;
            let Some(w) = self.state.workspaces.get_mut(&id) else { continue };
            if w.mode.as_deref() != Some(mode.as_str()) {
                w.mode = Some(mode.clone());
                self.dirty = true;
            }
        }
        Ok(())
    }

    /// Updates each tracked tile from Hyprland. Skips workspaces whose recorded mode
    /// differs from the live one (their tiled geometry must survive for archiving),
    /// and moves a record whose window Hyprland reports on another known workspace.
    pub fn capture_geometry(&mut self) -> Result<bool> {
        let clients = self.hypr.clients()?;
        let list = self.hypr.workspaces()?;
        let default = std::cell::OnceCell::new();
        let mut changed = false;
        let mut moved: Vec<(i64, i64, String)> = Vec::new();
        let ids: Vec<i64> = self.state.workspaces.keys().copied().collect();
        for id in ids {
            let recorded = self.state.workspaces[&id].mode.clone();
            let stale = match &recorded {
                Some(m) => self.mode_in(&list, &default, id) != *m,
                None => false,
            };
            let ws = self.state.workspaces.get_mut(&id).expect("listed");
            for t in &mut ws.tiles {
                let Some(c) = clients.iter().find(|c| c.address == t.address) else { continue };
                if c.workspace > 0 && c.workspace != id {
                    moved.push((id, c.workspace, t.address.clone()));
                    continue;
                }
                if stale {
                    continue;
                }
                if t.at != c.at || t.size != c.size || t.floating != c.floating || t.title != c.title {
                    t.at = c.at;
                    t.size = c.size;
                    t.floating = c.floating;
                    t.title = c.title.clone();
                    changed = true;
                }
            }
        }
        for (from, to, addr) in moved {
            if !self.state.workspaces.contains_key(&to) {
                continue;
            }
            let Some(src) = self.state.workspaces.get_mut(&from) else { continue };
            let Some(pos) = src.tiles.iter().position(|t| t.address == addr) else { continue };
            let tile = src.tiles.remove(pos);
            self.state.workspaces.get_mut(&to).expect("checked").tiles.push(tile);
            changed = true;
            self.dirty = true;
            self.maybe_delete(from)?;
        }
        Ok(changed)
    }
}
