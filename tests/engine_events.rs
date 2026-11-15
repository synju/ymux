mod common;
use ymux::config::Config;
use ymux::engine::{Engine, Request};
use ymux::events::Event;
use ymux::hypr::{focus_monitor, focus_workspace, move_workspace};
use ymux::model::{State, Tile};
use common::*;

fn tile(addr: &str, pid: u32) -> Tile {
    Tile { address: addr.into(), pid: Some(pid), class: "foot".into(), title: "t".into(), floating: false, at: (0, 0), size: (1, 1), ..Default::default() }
}

/// Two monitors: "LG A" (DP-1, x=0, focused) and "Dell B" (DP-2, x=2560).
/// Session 1 on LG A owns 100001 (shown, tile 0xa1) and 100002 (tile 0xa2).
/// Session 2 "Gaming" on Dell B owns 200001 (shown, tile 0xb1).
fn two() -> Engine<FakeHypr> {
    let mut s = State::default();
    s.ensure_monitor("LG A", "DP-1");
    s.ensure_monitor("Dell B", "DP-2");
    let a = s.create_session("LG A", None);
    s.create_workspace(a);
    let b = s.create_session("Dell B", Some("Gaming"));
    s.monitor_mut("LG A").unwrap().active_session = Some(a);
    s.monitor_mut("Dell B").unwrap().active_session = Some(b);
    s.sessions.get_mut(&a).unwrap().last_shown = Some(100001);
    s.monitor_mut("LG A").unwrap().shown = Some(100001);
    s.monitor_mut("Dell B").unwrap().shown = Some(200001);
    s.sessions.get_mut(&b).unwrap().last_shown = Some(200001);
    s.workspaces.get_mut(&100001).unwrap().tiles.push(tile("0xa1", 11));
    s.workspaces.get_mut(&100002).unwrap().tiles.push(tile("0xa2", 12));
    s.workspaces.get_mut(&200001).unwrap().tiles.push(tile("0xb1", 21));
    let h = FakeHypr::default();
    *h.monitors.borrow_mut() = vec![monitor("DP-1", "LG A", 0, true, 100001), monitor("DP-2", "Dell B", 2560, false, 200001)];
    *h.workspaces.borrow_mut() = vec![workspace(100001, "DP-1", 1), workspace(100002, "DP-1", 1), workspace(200001, "DP-2", 1)];
    *h.clients.borrow_mut() = vec![client("0xa1", 11, "foot", 100001), client("0xa2", 12, "foot", 100002), client("0xb1", 21, "foot", 200001)];
    Engine::new(s, Config::default(), h, "sig".into(), unique_dir("test-layouts-ev"))
}

fn ev(e: &mut Engine<FakeHypr>, event: Event) {
    e.on_event(event).unwrap();
}

#[test]
fn open_window_on_known_workspace_records_tile_with_pid() {
    let mut e = two();
    e.hypr().clients.borrow_mut().push(client("0xc1", 31, "kitty", 100002));
    ev(&mut e, Event::OpenWindow { address: "0xc1".into(), workspace_name: "100002".into(), class: "kitty".into(), title: "k".into() });
    let t = e.state.workspaces[&100002].tiles.iter().find(|t| t.address == "0xc1").unwrap();
    assert_eq!(t.pid, Some(31));
}

#[test]
fn closing_last_window_of_unshown_workspace_deletes_it() {
    let mut e = two();
    ev(&mut e, Event::CloseWindow { address: "0xa2".into() });
    assert!(!e.state.workspaces.contains_key(&100002));
}

#[test]
fn shown_empty_workspace_is_kept_until_left() {
    let mut e = two();
    ev(&mut e, Event::CloseWindow { address: "0xa1".into() });
    assert!(e.state.workspaces.contains_key(&100001));
    e.hypr().monitors.borrow_mut()[0].active_workspace = 100002;
    ev(&mut e, Event::Workspace { id: 100002 });
    assert!(!e.state.workspaces.contains_key(&100001));
}

#[test]
fn workspace_switch_sets_previous_and_last_shown() {
    let mut e = two();
    e.hypr().monitors.borrow_mut()[0].active_workspace = 100002;
    ev(&mut e, Event::Workspace { id: 100002 });
    assert_eq!(e.state.workspaces[&100002].previous, Some(100001));
    assert_eq!(e.state.sessions[&1].last_shown, Some(100002));
}

#[test]
fn last_workspace_deleted_deletes_unshown_session() {
    let mut e = two();
    e.handle(Request::NewSession { name: Some("Tmp".into()) }); // session 3 on LG A, now active
    let tmp_ws = e.state.session_workspaces(3)[0].real_id;
    e.hypr().monitors.borrow_mut()[0].active_workspace = tmp_ws;
    ev(&mut e, Event::Workspace { id: tmp_ws });
    e.handle(Request::SwitchSession { number: 1 });
    e.hypr().monitors.borrow_mut()[0].active_workspace = 100001;
    ev(&mut e, Event::Workspace { id: 100001 });
    // the new session's placeholder never arrives in this test: once its job times
    // out the workspace is empty and unshown, so it and the session go (spec 5.6)
    e.on_deadline(std::time::Instant::now() + std::time::Duration::from_secs(6)).unwrap();
    assert!(!e.state.sessions.contains_key(&3));
}

#[test]
fn outside_focus_switches_active_session() {
    let mut e = two();
    e.handle(Request::NewSession { name: None }); // session 3 active on LG A
    e.hypr().monitors.borrow_mut()[0].active_workspace = 100002; // e.g. Super+Shift+K
    ev(&mut e, Event::Workspace { id: 100002 });
    assert_eq!(e.state.monitor("LG A").unwrap().active_session, Some(1));
}

#[test]
fn move_window_moves_tile_record_and_cleans_source() {
    let mut e = two();
    ev(&mut e, Event::MoveWindow { address: "0xa2".into(), workspace: 100001, workspace_name: "100001".into() });
    assert!(e.state.workspaces[&100001].tiles.iter().any(|t| t.address == "0xa2"));
    assert!(!e.state.workspaces.contains_key(&100002));
}

#[test]
fn move_to_scratchpad_removes_tile_and_ignores_special() {
    let mut e = two();
    ev(&mut e, Event::MoveWindow { address: "0xa2".into(), workspace: -97, workspace_name: "special:scratchpad".into() });
    assert!(!e.state.workspaces.contains_key(&100002));
    assert!(e.state.workspaces.keys().all(|id| *id > 0));
}

#[test]
fn minimized_window_keeps_origin_alive() {
    let mut e = two();
    {
        let mut c = e.hypr().clients.borrow_mut();
        let w = c.iter_mut().find(|c| c.address == "0xa2").unwrap();
        w.workspace = -90;
        w.workspace_name = "special:minimized".into();
        w.tags = vec!["omarchy-minimized-from-100002".into()];
    }
    ev(&mut e, Event::MoveWindow { address: "0xa2".into(), workspace: -90, workspace_name: "special:minimized".into() });
    assert!(e.state.workspaces.contains_key(&100002));
}

#[test]
fn special_workspaces_never_enter_state() {
    let mut e = two();
    ev(&mut e, Event::CreateWorkspace { id: -98 });
    e.hypr().clients.borrow_mut().push(client("0xq", 41, "foot", -98));
    ev(&mut e, Event::OpenWindow { address: "0xq".into(), workspace_name: "special:qconsole".into(), class: "foot".into(), title: "q".into() });
    assert!(e.state.workspaces.keys().all(|id| *id > 0));
}

#[test]
fn window_opened_on_unknown_id_is_adopted_into_monitor_active_session() {
    let mut e = two();
    e.hypr().workspaces.borrow_mut().push(workspace(5, "DP-2", 1));
    e.hypr().clients.borrow_mut().push(client("0xz", 51, "foot", 5));
    ev(&mut e, Event::OpenWindow { address: "0xz".into(), workspace_name: "5".into(), class: "foot".into(), title: "z".into() });
    let ws = e.state.workspaces.values().find(|w| w.tiles.iter().any(|t| t.address == "0xz")).unwrap();
    assert_eq!(ws.session, 2);
}

#[test]
fn monitor_events_only_mark_invariant_pending() {
    let mut e = two();
    ev(&mut e, Event::MoveWorkspace { id: 200001, monitor: "DP-1".into() });
    assert!(e.invariant_pending);
    assert!(e.hypr().dispatched().is_empty());
}

#[test]
fn invariant_is_idempotent() {
    let mut e = two();
    e.enforce_invariant().unwrap();
    assert!(e.hypr().dispatched().is_empty());
    assert!(!e.invariant_pending);
}

#[test]
fn new_monitor_without_sessions_gets_main() {
    let mut e = two();
    e.hypr().monitors.borrow_mut().push(monitor("HDMI-A-1", "Acer C", 5120, false, 7));
    e.enforce_invariant().unwrap();
    let s = e.state.sessions.values().find(|s| s.monitor == "Acer C").unwrap();
    assert_eq!(s.name, "main");
    assert_eq!(e.state.session_workspaces(s.number).len(), 1);
    assert_eq!(e.state.monitor("Acer C").unwrap().active_session, Some(s.number));
}

#[test]
fn unplugged_monitor_sessions_join_remaining_monitor_keeping_names() {
    let mut e = two();
    *e.hypr().monitors.borrow_mut() = vec![monitor("DP-1", "LG A", 0, true, 100001)];
    *e.hypr().workspaces.borrow_mut() = vec![workspace(100001, "DP-1", 1), workspace(100002, "DP-1", 1), workspace(200001, "DP-1", 1)];
    e.enforce_invariant().unwrap();
    let g = e.state.sessions.values().find(|s| s.name == "Gaming").unwrap();
    assert_eq!(g.monitor, "LG A");
    assert_eq!(e.state.monitor("LG A").unwrap().active_session, Some(1));
    assert!(e.hypr().dispatched().is_empty());
}

#[test]
fn replug_moves_returned_workspaces_back_to_their_session_monitor() {
    let mut e = two();
    // Gaming now belongs to LG A (after an unplug); Hyprland returned 200001 to DP-2 on replug.
    e.state.sessions.get_mut(&2).unwrap().monitor = "LG A".into();
    e.state.monitor_mut("Dell B").unwrap().active_session = None;
    e.enforce_invariant().unwrap();
    assert_eq!(e.hypr().dispatched(), vec![move_workspace(200001, "DP-1")]);
    let s = e.state.sessions.values().find(|s| s.monitor == "Dell B").unwrap();
    assert_eq!(s.name, "main");
}

#[test]
fn session_move_right_activates_on_target_and_refills_source() {
    let mut e = two();
    e.handle(Request::SessionMove { dir: 'r' });
    let d = e.hypr().dispatched();
    assert!(d.contains(&move_workspace(100001, "DP-2")));
    assert!(d.contains(&move_workspace(100002, "DP-2")));
    assert_eq!(e.state.sessions[&1].monitor, "Dell B");
    assert_eq!(e.state.monitor("Dell B").unwrap().active_session, Some(1));
    let src = e.state.monitor("LG A").unwrap().active_session.unwrap();
    assert_eq!(e.state.sessions[&src].name, "main");
    // focus ends on the moved session (tmux-like)
    let n = d.len();
    assert_eq!(d[n - 2..], [focus_monitor("DP-2"), focus_workspace(100001)]);
}

#[test]
fn session_move_with_no_monitor_in_direction_is_noop() {
    let mut e = two();
    let before = e.state.clone();
    e.handle(Request::SessionMove { dir: 'l' });
    assert!(e.hypr().dispatched().is_empty());
    assert_eq!(e.state, before);
}

#[test]
fn outside_change_id_onto_mapped_id_is_ignored() {
    let mut e = two();
    ev(&mut e, Event::ChangeWorkspaceId { old: 100002, new: 100001 });
    assert_eq!(e.state.workspaces[&100002].number, 2);
    assert_eq!(e.state.workspaces[&100001].number, 1);
}

#[test]
fn outside_change_id_moves_mapping_when_safe() {
    let mut e = two();
    *e.hypr().workspaces.borrow_mut() = vec![workspace(100001, "DP-1", 1), workspace(7, "DP-1", 1), workspace(200001, "DP-2", 1)];
    ev(&mut e, Event::ChangeWorkspaceId { old: 100002, new: 7 });
    let w = e.state.workspaces.values().find(|w| w.tiles.iter().any(|t| t.address == "0xa2")).unwrap();
    assert_eq!((w.session, w.number), (1, 2));
}

#[test]
fn adopting_while_active_session_unset_reuses_existing_session() {
    let mut e = two();
    e.state.monitor_mut("LG A").unwrap().active_session = None;
    e.hypr().workspaces.borrow_mut().push(workspace(5, "DP-1", 1));
    e.hypr().clients.borrow_mut().push(client("0xz", 51, "foot", 5));
    ev(&mut e, Event::OpenWindow { address: "0xz".into(), workspace_name: "5".into(), class: "foot".into(), title: "z".into() });
    assert!(!e.state.sessions.values().any(|s| s.name == "main"));
    let w = e.state.workspaces.values().find(|w| w.tiles.iter().any(|t| t.address == "0xz")).unwrap();
    assert_eq!(w.session, 1);
}

#[test]
fn shown_event_attributed_to_the_workspaces_own_monitor() {
    let mut e = two();
    // DP-1 focused, but 200001 lives on DP-2
    ev(&mut e, Event::Workspace { id: 200001 });
    assert_eq!(e.state.monitor("Dell B").unwrap().shown, Some(200001));
    assert_eq!(e.state.monitor("LG A").unwrap().shown, Some(100001));
}

#[test]
fn a_closed_window_is_remembered_for_the_restore_snapshot() {
    let mut e = two();
    assert!(e.last_close.is_none());
    e.on_event(Event::CloseWindow { address: "0xa1".into() }).unwrap();
    assert!(e.last_close.is_some());
}
