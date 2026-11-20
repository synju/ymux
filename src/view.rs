//! The view ymux streams to the shell plugin (`ymux watch`): bar rows per
//! connected monitor and the tree, derived from state plus live facts (spec 3.1, 4, 5.1).

use crate::model::{reading_key, State, Workspace};
use serde::Serialize;

#[derive(Serialize, Debug, PartialEq)]
pub struct View {
    /// The options card's toggles (spec 5.4).
    pub keep_spots: bool,
    pub auto_close: bool,
    /// Folded tree rows (spec 5.1), so the tree opens as it was left.
    pub folded: Vec<String>,
    pub mode: String,
    pub marked: bool,
    pub relaunch: bool,
    pub monitors: Vec<MonitorView>,
    pub tree: Vec<TreeMonitor>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct MonitorView {
    pub connector: String,
    pub description: String,
    pub focused: bool,
    pub session: Option<u32>,
    pub session_label: String,
    pub workspaces: Vec<WsView>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct WsView {
    pub id: i64,
    pub number: u32,
    pub label: String,
    pub current: bool,
    pub empty: bool,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct TreeMonitor {
    /// The name given with `r`, if any.
    pub name: Option<String>,
    pub connector: String,
    pub description: String,
    pub focused: bool,
    pub sessions: Vec<TreeSession>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct TreeSession {
    /// The raw name, for the rename prompt (`label` adds `(n)` on a clash).
    pub name: String,
    pub number: u32,
    pub label: String,
    pub active: bool,
    pub workspaces: Vec<TreeWorkspace>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct TreeWorkspace {
    pub name: Option<String>,
    pub id: i64,
    pub number: u32,
    pub label: String,
    pub shown: bool,
    pub tiles: Vec<TreeTile>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct TreeTile {
    pub name: Option<String>,
    pub address: String,
    pub class: String,
    pub title: String,
    pub focused: bool,
}

/// Live facts the view needs besides state.
pub struct Live {
    /// (connector, description) of every connected monitor.
    pub connected: Vec<(String, String)>,
    pub focused_connector: Option<String>,
    pub focused_address: Option<String>,
    pub submap: String,
    pub relaunch: bool,
    pub keep_spots: bool,
    pub auto_close: bool,
}

pub fn mode_name(submap: &str) -> &'static str {
    match submap {
        "ymux" => "LEADER",
        "ymux_resize" => "RESIZE",
        _ => "",
    }
}

/// `name`, or `name (number)` when another session has the same name (spec 4.1,
/// user decision at phase 3 acceptance).
pub fn session_label(state: &State, number: u32) -> String {
    let Some(s) = state.sessions.get(&number) else { return number.to_string() };
    let clash = state.sessions.values().any(|o| o.number != number && o.name == s.name);
    if clash { format!("{} ({number})", s.name) } else { s.name.clone() }
}

fn has_app(ws: &Workspace) -> bool {
    ws.tiles.iter().any(|t| !t.is_placeholder())
}

/// A name the user gave shows as `name number`; an unnamed workspace shows its
/// number only, never an app name (user decision at phase 3 acceptance).
pub fn workspace_label(ws: &Workspace) -> String {
    match &ws.name {
        Some(n) => format!("{n} {}", ws.number),
        None => ws.number.to_string(),
    }
}

pub fn build(state: &State, live: &Live) -> View {
    let mut monitors = Vec::new();
    let mut tree = Vec::new();
    for (connector, description) in &live.connected {
        let Some(m) = state.monitor(description) else { continue };
        let focused = live.focused_connector.as_deref() == Some(connector.as_str());
        let workspaces = m
            .active_session
            .map(|s| {
                state
                    .session_workspaces(s)
                    .into_iter()
                    .map(|w| WsView {
                        id: w.real_id,
                        number: w.number,
                        label: workspace_label(w),
                        current: m.shown == Some(w.real_id),
                        empty: !has_app(w),
                    })
                    .collect()
            })
            .unwrap_or_default();
        monitors.push(MonitorView {
            connector: connector.clone(),
            description: description.clone(),
            focused,
            session: m.active_session,
            session_label: m.active_session.map(|s| session_label(state, s)).unwrap_or_default(),
            workspaces,
        });
        let sessions = state
            .sessions
            .values()
            .filter(|s| &s.monitor == description)
            .map(|s| TreeSession {
                name: s.name.clone(),
                number: s.number,
                label: session_label(state, s.number),
                active: m.active_session == Some(s.number),
                workspaces: state
                    .session_workspaces(s.number)
                    .into_iter()
                    .map(|w| {
                        let mut tiles: Vec<_> = w.tiles.iter().filter(|t| !t.is_placeholder()).collect();
                        tiles.sort_by_key(|t| reading_key(t.at));
                        TreeWorkspace {
                            name: w.name.clone(),
                            id: w.real_id,
                            number: w.number,
                            label: workspace_label(w),
                            shown: m.shown == Some(w.real_id),
                            tiles: tiles
                                .into_iter()
                                .map(|t| TreeTile {
                                    name: t.name.clone(),
                                    address: t.address.clone(),
                                    class: t.class.clone(),
                                    title: t.title.clone(),
                                    focused: live.focused_address.as_deref() == Some(t.address.as_str()),
                                })
                                .collect(),
                        }
                    })
                    .collect(),
            })
            .collect();
        let name = state.monitor(description).and_then(|m| m.name.clone());
        tree.push(TreeMonitor { name, connector: connector.clone(), description: description.clone(), focused, sessions });
    }
    View {
        folded: state.folded.iter().cloned().collect(),
        mode: mode_name(&live.submap).to_string(),
        marked: state.mark.is_some(),
        relaunch: live.relaunch,
        keep_spots: live.keep_spots,
        auto_close: live.auto_close,
        monitors,
        tree,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{State, Tile};

    fn t(addr: &str, class: &str, at: (i32, i32)) -> Tile {
        Tile { address: addr.into(), pid: Some(1), class: class.into(), title: class.into(), floating: false, at, size: (1, 1), ..Default::default() }
    }

    fn live(connected: &[(&str, &str)]) -> Live {
        Live {
            connected: connected.iter().map(|(c, d)| (c.to_string(), d.to_string())).collect(),
            focused_connector: connected.first().map(|(c, _)| c.to_string()),
            focused_address: None,
            submap: String::new(),
            relaunch: true,
            keep_spots: false,
            auto_close: false,
        }
    }

    #[test]
    fn bar_row_matches_tmux_example() {
        let mut s = State::default();
        s.ensure_monitor("LG A", "DP-1");
        let a = s.create_session("LG A", Some("SC Neural Nets"));
        let w2 = s.create_workspace(a);
        s.monitor_mut("LG A").unwrap().active_session = Some(a);
        s.monitor_mut("LG A").unwrap().shown = Some(w2);
        let w1 = s.workspace_by_number(a, 1).unwrap().real_id;
        s.workspaces.get_mut(&w1).unwrap().name = Some("Net Extraction".into());
        s.workspaces.get_mut(&w1).unwrap().tiles.push(t("0x1", "code", (0, 0)));
        let ws2 = s.workspaces.get_mut(&w2).unwrap();
        ws2.tiles.push(t("0x2", "zsh", (0, 0)));
        ws2.auto_name = Some("zsh".into());
        let v = build(&s, &live(&[("DP-1", "LG A")]));
        let m = &v.monitors[0];
        assert_eq!(m.session_label, "SC Neural Nets");
        // user decision: an unnamed workspace shows its number only, never the app
        assert_eq!(m.workspaces.iter().map(|w| w.label.as_str()).collect::<Vec<_>>(), ["Net Extraction 1", "2"]);
        assert_eq!(m.workspaces.iter().map(|w| w.current).collect::<Vec<_>>(), [false, true]);
    }

    #[test]
    fn clashing_session_names_show_numbers() {
        let mut s = State::default();
        s.ensure_monitor("LG A", "DP-1");
        s.ensure_monitor("Dell B", "DP-2");
        s.create_session("LG A", Some("main"));
        s.create_session("LG A", Some("work"));
        let c = s.create_session("Dell B", Some("main"));
        assert_eq!(session_label(&s, c), "main (3)");
        assert_eq!(session_label(&s, 2), "work");
    }

    #[test]
    fn placeholder_only_workspace_shows_number_only() {
        let mut s = State::default();
        s.ensure_monitor("M", "DP-1");
        let a = s.create_session("M", None);
        let id = s.session_workspaces(a)[0].real_id;
        let w = s.workspaces.get_mut(&id).unwrap();
        w.number = 4;
        w.tiles.push(t("0xp", "ymux-placeholder", (0, 0)));
        w.auto_name = Some("foot".into());
        assert_eq!(workspace_label(&s.workspaces[&id]), "4");
        let v = build(&s, &{ let mut l = live(&[("DP-1", "M")]); l.focused_connector = None; l });
        assert!(v.monitors.is_empty() || v.monitors[0].workspaces.is_empty() || v.monitors[0].workspaces[0].empty);
    }

    #[test]
    fn renamed_empty_workspace_keeps_its_name() {
        let w = crate::model::Workspace { number: 2, name: Some("notes".into()), ..Default::default() };
        assert_eq!(workspace_label(&w), "notes 2");
    }

    #[test]
    fn only_connected_monitors_get_rows() {
        let mut s = State::default();
        s.ensure_monitor("LG A", "DP-1");
        s.ensure_monitor("Old", "DP-1");
        let v = build(&s, &live(&[("DP-1", "LG A")]));
        assert_eq!(v.monitors.len(), 1);
        assert_eq!(v.monitors[0].description, "LG A");
    }

    #[test]
    fn tree_hides_placeholders_and_orders_tiles() {
        let mut s = State::default();
        s.ensure_monitor("M", "DP-1");
        let a = s.create_session("M", None);
        let id = s.session_workspaces(a)[0].real_id;
        let w = s.workspaces.get_mut(&id).unwrap();
        w.tiles.push(t("0xr", "r", (900, 0)));
        w.tiles.push(t("0xl", "l", (0, 0)));
        w.tiles.push(t("0xb", "b", (0, 500)));
        w.tiles.push(t("0xp", "ymux-placeholder", (0, 0)));
        let v = build(&s, &live(&[("DP-1", "M")]));
        let tiles: Vec<&str> = v.tree[0].sessions[0].workspaces[0].tiles.iter().map(|t| t.address.as_str()).collect();
        assert_eq!(tiles, ["0xl", "0xr", "0xb"]);
    }

    #[test]
    fn mode_marker_names() {
        assert_eq!(mode_name("ymux"), "LEADER");
        assert_eq!(mode_name("ymux_resize"), "RESIZE");
        assert_eq!(mode_name(""), "");
        assert_eq!(mode_name("mouse_slow"), "");
    }
}
