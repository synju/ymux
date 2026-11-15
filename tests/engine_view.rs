mod common;
use ymux::config::Config;
use ymux::engine::{Engine, Request};
use ymux::events::Event;
use ymux::hypr::{focus_monitor, focus_workspace};
use ymux::model::{State, Tile};
use common::*;

fn tile(addr: &str, class: &str) -> Tile {
    Tile { address: addr.into(), pid: Some(1), class: class.into(), title: "t".into(), floating: false, at: (0, 0), size: (1, 1), ..Default::default() }
}

/// One monitor "LG A" (DP-1) showing 100001, which holds foot 0xa1.
fn one() -> Engine<FakeHypr> {
    let mut s = State::default();
    s.ensure_monitor("LG A", "DP-1");
    let a = s.create_session("LG A", None);
    s.monitor_mut("LG A").unwrap().active_session = Some(a);
    s.monitor_mut("LG A").unwrap().shown = Some(100001);
    s.workspaces.get_mut(&100001).unwrap().tiles.push(tile("0xa1", "foot"));
    let h = FakeHypr::default();
    *h.monitors.borrow_mut() = vec![monitor("DP-1", "LG A", 0, true, 100001)];
    *h.workspaces.borrow_mut() = vec![workspace(100001, "DP-1", 1)];
    *h.clients.borrow_mut() = vec![client("0xa1", 1, "foot", 100001)];
    Engine::new(s, Config::default(), h, "sig".into(), unique_dir("test-view"))
}

#[test]
fn focused_app_names_its_workspace() {
    let mut e = one();
    e.on_event(Event::ActiveWindow { address: Some("0xa1".into()) }).unwrap();
    assert_eq!(e.state.workspaces[&100001].auto_name.as_deref(), Some("foot"));
    assert!(e.take_dirty());
}

#[test]
fn focused_placeholder_keeps_the_name() {
    let mut e = one();
    e.on_event(Event::ActiveWindow { address: Some("0xa1".into()) }).unwrap();
    e.take_dirty();
    e.state.workspaces.get_mut(&100001).unwrap().tiles.push(tile("0xp1", "ymux-placeholder"));
    e.on_event(Event::ActiveWindow { address: Some("0xp1".into()) }).unwrap();
    assert_eq!(e.state.workspaces[&100001].auto_name.as_deref(), Some("foot"));
    assert!(!e.take_dirty());
}

#[test]
fn submap_event_changes_view_not_state() {
    let mut e = one();
    e.on_event(Event::Submap { name: "ymux".into() }).unwrap();
    assert!(e.take_view_changed());
    assert!(!e.take_dirty());
}

#[test]
fn focus_id_on_other_monitor_focuses_monitor_first() {
    let mut e = one();
    e.state.ensure_monitor("Dell B", "DP-2");
    let b = e.state.create_session("Dell B", None);
    e.state.monitor_mut("Dell B").unwrap().active_session = Some(b);
    e.hypr().monitors.borrow_mut().push(monitor("DP-2", "Dell B", 2560, false, 200001));
    e.handle(Request::FocusId { id: 200001 });
    assert_eq!(e.hypr().dispatched(), vec![focus_monitor("DP-2"), focus_workspace(200001)]);
}

#[test]
fn focus_id_unknown_does_nothing() {
    let mut e = one();
    e.handle(Request::FocusId { id: 7 });
    assert!(e.hypr().dispatched().is_empty());
}

#[test]
fn old_state_without_auto_name_loads() {
    let old = r#"{"version":1,"signature":"s","monitors":[{"key":"M","slot":1,"connector":"DP-1","active_session":1}],
      "sessions":{"1":{"number":1,"name":"main","monitor":"M","last_shown":100001,"last_used":1}},
      "workspaces":{"100001":{"real_id":100001,"session":1,"number":1,"name":null,"tiles":[],"previous":null}}}"#;
    assert!(serde_json::from_str::<State>(old).is_ok());
}

#[test]
fn view_json_has_monitor_rows() {
    let e = one();
    let v = serde_json::to_value(e.view().unwrap()).unwrap();
    assert_eq!(v["monitors"][0]["connector"], "DP-1");
    assert_eq!(v["monitors"][0]["workspaces"][0]["id"], 100001);
}

// ---- Leader + w remembers what is folded (user request 2026-11-15) ----------------

#[test]
fn folds_are_remembered_and_shown_in_the_view() {
    let mut e = one();
    e.handle(Request::Fold { key: "s:1".into(), folded: true });
    e.handle(Request::Fold { key: "w:100001".into(), folded: true });
    assert_eq!(e.view().unwrap().folded, vec!["s:1".to_string(), "w:100001".into()]);
    e.handle(Request::Fold { key: "s:1".into(), folded: false });
    assert_eq!(e.view().unwrap().folded, vec!["w:100001".to_string()]);
}

#[test]
fn a_deleted_workspace_or_session_drops_its_fold() {
    let mut e = one();
    e.state.create_workspace(1); // 100002, empty and not shown
    e.handle(Request::Fold { key: "w:100002".into(), folded: true });
    e.handle(Request::KillWorkspace { id: 100002 });
    assert!(!e.state.workspaces.contains_key(&100002));
    assert!(e.state.folded.is_empty());
}

#[test]
fn state_without_folds_still_loads() {
    let old = r#"{"version":1,"signature":"s","monitors":[],"sessions":{},"workspaces":{}}"#;
    assert!(serde_json::from_str::<State>(old).unwrap().folded.is_empty());
}

// ---- r in the Leader + w tree renames any row (user request 2026-11-15) ------------

#[test]
fn a_monitor_can_be_named_and_unnamed() {
    let mut e = one();
    e.handle(Request::RenameMonitor { key: "LG A".into(), name: "Main".into() });
    assert_eq!(e.state.monitor("LG A").unwrap().name.as_deref(), Some("Main"));
    assert_eq!(e.view().unwrap().tree[0].name.as_deref(), Some("Main"));
    e.handle(Request::RenameMonitor { key: "LG A".into(), name: "".into() });
    assert_eq!(e.view().unwrap().tree[0].name, None);
}

#[test]
fn an_app_can_be_named_and_keeps_it_through_a_move() {
    let mut e = one();
    e.handle(Request::RenameTile { address: "0xa1".into(), name: "editor".into() });
    assert_eq!(e.view().unwrap().tree[0].sessions[0].workspaces[0].tiles[0].name.as_deref(), Some("editor"));
    e.state.create_workspace(1); // 100002
    e.hypr().workspaces.borrow_mut().push(workspace(100002, "DP-1", 0));
    e.hypr().clients.borrow_mut()[0].workspace = 100002;
    e.on_event(Event::MoveWindow { address: "0xa1".into(), workspace: 100002, workspace_name: "100002".into() }).unwrap();
    let t = e.state.workspaces[&100002].tiles.iter().find(|t| t.address == "0xa1").unwrap();
    assert_eq!(t.name.as_deref(), Some("editor"));
}

#[test]
fn app_names_survive_a_daemon_restart() {
    let mut e = one();
    e.handle(Request::RenameTile { address: "0xa1".into(), name: "editor".into() });
    e.state.signature = Some("sig".into());
    e.start().unwrap();
    let t = e.state.workspaces.values().flat_map(|w| &w.tiles).find(|t| t.address == "0xa1").unwrap();
    assert_eq!(t.name.as_deref(), Some("editor"));
}

#[test]
fn tree_rows_carry_the_raw_names_for_the_prompt() {
    let mut e = one();
    e.handle(Request::RenameWorkspace { id: 100001, name: "code".into() });
    let v = e.view().unwrap();
    assert_eq!(v.tree[0].sessions[0].name, e.state.sessions[&1].name);
    assert_eq!(v.tree[0].sessions[0].workspaces[0].name.as_deref(), Some("code"));
}

// ---- review fix 2: a failed view read never takes the daemon down ---------------

#[test]
fn view_json_is_none_when_hyprctl_fails_instead_of_an_error() {
    let e = one();
    assert!(ymux::daemon::view_json(&e).is_some());
    e.hypr().fail_monitors.set(true);
    assert!(ymux::daemon::view_json(&e).is_none());
}

#[test]
fn a_fold_follows_a_workspace_renumbered_from_outside() {
    let mut e = one();
    e.handle(Request::Fold { key: "w:100001".into(), folded: true });
    e.hypr().workspaces.borrow_mut()[0].id = 100005; // Hyprland already renumbered it
    e.on_event(Event::ChangeWorkspaceId { old: 100001, new: 100005 }).unwrap();
    assert!(e.state.folded.contains("w:100005"));
    assert!(!e.state.folded.contains("w:100001"));
}
