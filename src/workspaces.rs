//! Closing a workspace that is on screen, and moving a workspace to another session
//! (options card actions, user decisions 2026-11-18).

use crate::engine::{Engine, Response};
use crate::hypr::{focus_monitor, move_workspace, Hypr};
use anyhow::Result;

impl<H: Hypr> Engine<H> {
    /// A workspace asked to close that a monitor still shows: the monitor moves to the
    /// session's other workspace (its previous one first), or, when it was the session's
    /// only one, to the monitor's most recent other session (or a new `main`). Leaving it
    /// then removes it (`maybe_delete`).
    pub(crate) fn leave_doomed(&mut self) -> Result<()> {
        for m in self.hypr.monitors()? {
            let id = m.active_workspace;
            let Some(ws) = self.state.workspaces.get(&id).cloned() else { continue };
            if !self.doomed.contains(&id) {
                continue;
            }
            let others: Vec<i64> =
                self.state.session_workspaces(ws.session).iter().map(|w| w.real_id).filter(|w| *w != id).collect();
            let target = match ws.previous.filter(|p| others.contains(p)).or(others.first().copied()) {
                Some(t) => t,
                None => {
                    let next = self
                        .state
                        .sessions
                        .values()
                        .filter(|s| s.monitor == m.description && s.number != ws.session)
                        .max_by_key(|s| s.last_used)
                        .map(|s| s.number);
                    let next = match next {
                        Some(n) => n,
                        None => self.state.create_session(&m.description, Some("main")),
                    };
                    self.activate(next);
                    let Some(t) = self.show_target(next) else { continue };
                    t
                }
            };
            self.hypr.dispatch(&focus_monitor(&m.name))?;
            self.focus(target)?;
        }
        Ok(())
    }

    /// Moves a workspace into another session, with the lowest free number there; to a
    /// session on another monitor it moves with Hyprland and takes an id in that
    /// monitor's range. A session left without workspaces goes.
    pub(crate) fn move_workspace_to_session(&mut self, id: i64, session: u32) -> Result<Response> {
        let Some(ws) = self.state.workspaces.get(&id).cloned() else {
            return Ok(Response::err(format!("ymux: no workspace {id}")));
        };
        let Some(target) = self.state.sessions.get(&session).cloned() else {
            return Ok(Response::err(format!("ymux: no session {session}")));
        };
        if ws.session == session {
            return Ok(Response::err("ymux: the workspace is already in that session".into()));
        }
        let from = ws.session;
        let from_monitor = self.state.sessions[&from].monitor.clone();
        let number = self.state.next_workspace_number(session);
        let mut id = id;
        if target.monitor != from_monitor {
            let Some(connector) = self.state.monitor(&target.monitor).and_then(|m| m.connector.clone()) else {
                return Ok(Response::err("ymux: that session's monitor is not connected".into()));
            };
            self.hypr.dispatch(&move_workspace(id, &connector))?;
            let new = self.state.alloc_real_id(&target.monitor);
            self.renumber(id, new)?;
            id = new;
        }
        let shown_here = self.state.monitor(&target.monitor).and_then(|m| m.shown) == Some(id);
        let w = self.state.workspaces.get_mut(&id).expect("known");
        w.session = session;
        w.number = number;
        w.previous = None;
        if shown_here {
            self.activate(session);
        }
        if self.state.session_workspaces(from).is_empty() {
            self.delete_session(from);
        }
        self.dirty = true;
        self.view_changed = true;
        self.invariant_pending = true;
        Ok(Response::ok())
    }
}
