//! ymux state model: monitors, sessions, workspaces, tiles (spec 1, 3.2.2).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::RangeInclusive;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct State {
    pub version: u32,
    pub signature: Option<String>,
    pub monitors: Vec<Monitor>,
    pub sessions: BTreeMap<u32, Session>,
    pub workspaces: BTreeMap<i64, Workspace>,
    /// Address of the marked tile (Leader + m, spec 2).
    #[serde(default)]
    pub mark: Option<String>,
    /// Rows folded in the Leader + w tree (`m:<connector>`, `s:<n>`, `w:<id>`), kept
    /// across openings, restarts and reboots (user request 2026-11-15).
    #[serde(default)]
    pub folded: std::collections::BTreeSet<String>,
}

impl Default for State {
    fn default() -> Self {
        State {
            version: 1,
            signature: None,
            monitors: Vec::new(),
            sessions: BTreeMap::new(),
            workspaces: BTreeMap::new(),
            mark: None,
            folded: Default::default(),
        }
    }
}

/// A physical screen, keyed by its stable description (make, model, serial).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Monitor {
    pub key: String,
    pub slot: u32,
    pub connector: Option<String>,
    pub active_session: Option<u32>,
    /// The ymux workspace this monitor last showed (for event-driven deletion).
    #[serde(default)]
    pub shown: Option<i64>,
    /// A name given in the Leader + w tree (`r`), shown instead of connector and
    /// description (user request 2026-11-15).
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Session {
    pub number: u32,
    pub name: String,
    pub monitor: String,
    pub last_shown: Option<i64>,
    pub last_used: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Workspace {
    pub real_id: i64,
    pub session: u32,
    pub number: u32,
    pub name: Option<String>,
    pub tiles: Vec<Tile>,
    pub previous: Option<i64>,
    /// Class of the app last focused on it (spec 1 item 3); a placeholder never sets it.
    #[serde(default)]
    pub auto_name: Option<String>,
    /// Last layout mode read (spec 3.2.11, 7.1).
    #[serde(default)]
    pub mode: Option<String>,
    /// Every tile as it stood when the workspace went floating (placeholders parked), in
    /// reading order, so the round trip back can put each window in its old place
    /// (spec 5.7). Empty when not parked.
    #[serde(default)]
    pub archived_layout: Vec<SavedTile>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SavedTile {
    pub address: String,
    pub at: (i32, i32),
    pub size: (i32, i32),
}

/// A placeholder's place in its workspace's reading order: its neighbour before it and
/// its spot (restart rule, spec 3.2.8).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ArchivedPlaceholder {
    pub index: usize,
    /// Address of the tile just before it in reading order, if any.
    pub after: Option<String>,
    pub at: (i32, i32),
    pub size: (i32, i32),
}

/// Wayland app id / Hyprland class of ymux's placeholder windows (spec 5.7).
pub const PLACEHOLDER_CLASS: &str = "ymux-placeholder";

/// Sort key for "left to right, top to bottom": by y, then x.
pub fn reading_key(at: (i32, i32)) -> (i32, i32) {
    (at.1, at.0)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Tile {
    pub address: String,
    pub pid: Option<u32>,
    pub class: String,
    pub title: String,
    pub floating: bool,
    pub at: (i32, i32),
    pub size: (i32, i32),
    /// How to start it again: `/proc/<pid>/cmdline` and cwd (spec 7.2).
    #[serde(default)]
    pub cmdline: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// A name given in the Leader + w tree (`r`), shown instead of class and title.
    #[serde(default)]
    pub name: Option<String>,
}

impl Tile {
    pub fn is_placeholder(&self) -> bool {
        self.class == PLACEHOLDER_CLASS
    }
}

const RANGE_SIZE: i64 = 100_000;

impl State {
    /// Real Hyprland workspace ids owned by monitor slot `slot` (1-based).
    pub fn range_for_slot(slot: u32) -> RangeInclusive<i64> {
        let base = slot as i64 * RANGE_SIZE;
        (base + 1)..=(base + RANGE_SIZE - 1)
    }

    pub fn monitor(&self, key: &str) -> Option<&Monitor> {
        self.monitors.iter().find(|m| m.key == key)
    }

    pub fn monitor_mut(&mut self, key: &str) -> Option<&mut Monitor> {
        self.monitors.iter_mut().find(|m| m.key == key)
    }

    /// Returns the monitor's permanent slot, registering it on first sight.
    pub fn ensure_monitor(&mut self, key: &str, connector: &str) -> u32 {
        if let Some(m) = self.monitor_mut(key) {
            m.connector = Some(connector.to_string());
            return m.slot;
        }
        let slot = self.monitors.iter().map(|m| m.slot).max().unwrap_or(0) + 1;
        self.monitors.push(Monitor {
            key: key.to_string(),
            slot,
            connector: Some(connector.to_string()),
            active_session: None,
            shown: None,
            name: None,
        });
        slot
    }

    /// Lowest id in the monitor's range not used by any workspace in state.
    pub fn alloc_real_id(&self, monitor_key: &str) -> i64 {
        let slot = self
            .monitor(monitor_key)
            .unwrap_or_else(|| panic!("unknown monitor {monitor_key}"))
            .slot;
        Self::range_for_slot(slot)
            .find(|id| !self.workspaces.contains_key(id))
            .expect("monitor id range exhausted")
    }

    pub fn next_session_number(&self) -> u32 {
        (1..).find(|n| !self.sessions.contains_key(n)).unwrap()
    }

    pub fn next_workspace_number(&self, session: u32) -> u32 {
        (1..)
            .find(|n| self.workspace_by_number(session, *n).is_none())
            .unwrap()
    }

    pub fn session_workspaces(&self, session: u32) -> Vec<&Workspace> {
        let mut v: Vec<&Workspace> = self
            .workspaces
            .values()
            .filter(|w| w.session == session)
            .collect();
        v.sort_by_key(|w| w.number);
        v
    }

    pub fn workspace_by_number(&self, session: u32, number: u32) -> Option<&Workspace> {
        self.workspaces
            .values()
            .find(|w| w.session == session && w.number == number)
    }

    /// Creates a session (with workspace 1) on a known monitor; returns its number.
    pub fn create_session(&mut self, monitor_key: &str, name: Option<&str>) -> u32 {
        let number = self.next_session_number();
        let name = name.map(str::to_string).unwrap_or_else(|| number.to_string());
        self.sessions.insert(
            number,
            Session {
                number,
                name,
                monitor: monitor_key.to_string(),
                last_shown: None,
                last_used: 0,
            },
        );
        self.create_workspace(number);
        number
    }

    /// Creates the session's lowest free workspace number; returns its real id.
    pub fn create_workspace(&mut self, session: u32) -> i64 {
        let number = self.next_workspace_number(session);
        self.create_workspace_numbered(session, number)
    }

    /// Creates workspace `number` in the session (caller checks it is free).
    pub fn create_workspace_numbered(&mut self, session: u32, number: u32) -> i64 {
        let monitor = self.sessions[&session].monitor.clone();
        let real_id = self.alloc_real_id(&monitor);
        self.workspaces.insert(
            real_id,
            Workspace { real_id, session, number, ..Default::default() },
        );
        real_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_ranges() {
        assert_eq!(State::range_for_slot(1), 100001..=199999);
        assert_eq!(State::range_for_slot(2), 200001..=299999);
    }

    #[test]
    fn monitor_slots_are_first_seen_order_and_permanent() {
        let mut s = State::default();
        assert_eq!(s.ensure_monitor("LG A", "DP-1"), 1);
        assert_eq!(s.ensure_monitor("Panel B", "eDP-1"), 2);
        assert_eq!(s.ensure_monitor("LG A", "DP-3"), 1);
        assert_eq!(s.monitor("LG A").unwrap().connector.as_deref(), Some("DP-3"));
    }

    #[test]
    fn real_ids_lowest_free_in_range_across_all_sessions() {
        let mut s = State::default();
        s.ensure_monitor("M", "DP-1");
        let a = s.create_session("M", None);
        assert_eq!(s.session_workspaces(a)[0].real_id, 100001);
        assert_eq!(s.create_workspace(a), 100002);
        s.workspaces.remove(&100001);
        assert_eq!(s.alloc_real_id("M"), 100001);
    }

    #[test]
    fn session_numbers_unique_and_named_after_number() {
        let mut s = State::default();
        s.ensure_monitor("M", "DP-1");
        s.ensure_monitor("N", "DP-2");
        let a = s.create_session("M", None);
        let b = s.create_session("N", Some("main"));
        assert_eq!((a, b), (1, 2));
        assert_eq!(s.sessions[&1].name, "1");
        assert_eq!(s.sessions[&2].name, "main");
    }

    #[test]
    fn workspace_numbers_lowest_free_from_one() {
        let mut s = State::default();
        s.ensure_monitor("M", "DP-1");
        let a = s.create_session("M", None);
        let w2 = s.create_workspace(a);
        let _w3 = s.create_workspace(a);
        s.workspaces.remove(&w2);
        assert_eq!(s.next_workspace_number(a), 2);
        assert_eq!(s.workspace_by_number(a, 3).unwrap().number, 3);
    }
}
