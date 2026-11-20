//! Workspaces stay until closed (auto-close opt-in), closing from the card, and moving
//! a workspace to another session (user decisions 2026-11-18).
mod common;
use common::*;
use std::fs;
use ymux::config::Config;
use ymux::engine::{Engine, Request};
use ymux::events::Event;
use ymux::hypr::{change_id, close_window, focus_workspace, move_workspace};
use ymux::model::{State, Tile};

/// Monitor "LG A" (DP-1) shows 100001 of session 1 (foot 0xa1); session 1 also has an
/// empty 100002. Session 2 on the same monitor has 100003. Monitor "Dell B" (DP-2,
/// slot 2) has session 3 with 200001.
fn engine(name: &str) -> Engine<FakeHypr> {
    let mut s = State::default();
    s.ensure_monitor("LG A", "DP-1");
    s.ensure_monitor("Dell B", "DP-2");
    let one = s.create_session("LG A", Some("work"));
    s.create_workspace(one); // 100002
    let two = s.create_session("LG A", Some("play")); // 100003
    let three = s.create_session("Dell B", Some("side")); // 200001
    s.monitor_mut("LG A").unwrap().active_session = Some(one);
    s.monitor_mut("LG A").unwrap().shown = Some(100001);
    s.monitor_mut("Dell B").unwrap().active_session = Some(three);
    s.monitor_mut("Dell B").unwrap().shown = Some(200001);
    s.workspaces.get_mut(&100001).unwrap().tiles.push(Tile { address: "0xa1".into(), pid: Some(1), class: "foot".into(), ..Default::default() });
    assert_eq!(two, 2);
    let h = FakeHypr::default();
    *h.monitors.borrow_mut() = vec![monitor("DP-1", "LG A", 0, true, 100001), monitor("DP-2", "Dell B", 2560, false, 200001)];
    *h.workspaces.borrow_mut() = vec![
        workspace(100001, "DP-1", 1),
        workspace(100002, "DP-1", 0),
        workspace(100003, "DP-1", 0),
        workspace(200001, "DP-2", 0),
    ];
    *h.clients.borrow_mut() = vec![client("0xa1", 1, "foot", 100001)];
    let mut e = Engine::new(s, Config::default(), h, "sig".into(), unique_dir(name));
    e.exe = "cm".into();
    e
}

/// The monitor now shows `id` (what Hyprland reports after a focus).
fn shown(e: &mut Engine<FakeHypr>, id: i64) {
    let connector = if id > 200000 { "DP-2" } else { "DP-1" };
    e.hypr().monitors.borrow_mut().iter_mut().find(|m| m.name == connector).unwrap().active_workspace = id;
    e.on_event(Event::Workspace { id }).unwrap();
}

// ---- auto-close is opt-in -----------------------------------------------------------

#[test]
fn auto_close_is_off_by_default_and_opt_in() {
    assert!(!Config::default().auto_close);
    let (c, e) = ymux::config::parse("set auto-close on\n");
    assert!(e.is_empty() && c.auto_close);
    assert!(ymux::config::default_text().contains("set auto-close off\n"));
}

#[test]
fn with_auto_close_off_an_empty_workspace_stays_when_left() {
    let mut e = engine("stay");
    shown(&mut e, 100002);
    shown(&mut e, 100001); // leaves the empty 100002
    assert!(e.state.workspaces.contains_key(&100002));
}

#[test]
fn with_auto_close_off_the_last_window_closing_keeps_the_workspace() {
    let mut e = engine("lastwin");
    shown(&mut e, 100002);
    e.hypr().clients.borrow_mut().clear();
    e.on_event(Event::CloseWindow { address: "0xa1".into() }).unwrap();
    assert!(e.state.workspaces.contains_key(&100001));
}

#[test]
fn with_auto_close_on_empty_workspaces_go_as_before() {
    let mut e = engine("autoon");
    e.config.auto_close = true;
    shown(&mut e, 100002);
    shown(&mut e, 100001);
    assert!(!e.state.workspaces.contains_key(&100002));
}

#[test]
fn set_auto_close_rewrites_its_line_and_shows_in_the_view() {
    let d = unique_dir("autoconf");
    fs::create_dir_all(&d).unwrap();
    let f = d.join("ymux.conf");
    fs::write(&f, "# mine\n").unwrap();
    let mut e = engine("autoconfview");
    e.config_path = f.clone();
    assert!(!e.view().unwrap().auto_close);
    assert!(e.handle(Request::SetAutoClose { on: true }).ok);
    assert_eq!(fs::read_to_string(&f).unwrap(), "# mine\nset auto-close on\n");
    assert!(e.view().unwrap().auto_close);
}

// ---- closing from the card --------------------------------------------------------

#[test]
fn closing_an_empty_unshown_workspace_removes_it_at_once() {
    let mut e = engine("closeempty");
    assert!(e.handle(Request::KillWorkspace { id: 100002 }).ok);
    assert!(!e.state.workspaces.contains_key(&100002));
}

#[test]
fn closing_the_shown_workspace_closes_its_apps_then_switches_away_and_removes_it() {
    let mut e = engine("closeshown");
    e.handle(Request::KillWorkspace { id: 100001 });
    assert_eq!(e.hypr().dispatched(), vec![close_window("0xa1")]);
    e.hypr().clear();
    e.hypr().clients.borrow_mut().clear();
    e.on_event(Event::CloseWindow { address: "0xa1".into() }).unwrap();
    // the session's other workspace takes the monitor, then the closed one goes
    assert!(e.hypr().dispatched().contains(&focus_workspace(100002)));
    shown(&mut e, 100002);
    assert!(!e.state.workspaces.contains_key(&100001));
    assert!(e.state.sessions.contains_key(&1));
}

#[test]
fn closing_a_sessions_only_shown_workspace_switches_session_and_removes_both() {
    let mut e = engine("closeonly");
    shown(&mut e, 100003); // session 2 ("play"), its only workspace
    e.hypr().clear();
    e.handle(Request::KillWorkspace { id: 100003 });
    assert!(e.hypr().dispatched().contains(&focus_workspace(100001)), "back to session 1");
    shown(&mut e, 100001);
    assert!(!e.state.workspaces.contains_key(&100003));
    assert!(!e.state.sessions.contains_key(&2));
}

#[test]
fn closing_a_session_removes_its_workspaces_even_with_auto_close_off() {
    let mut e = engine("closesession");
    e.handle(Request::KillSession { number: 1 });
    e.hypr().clients.borrow_mut().clear();
    e.on_event(Event::CloseWindow { address: "0xa1".into() }).unwrap();
    // the monitor went to session 2; leaving 100001 removes it, 100002 went at once
    shown(&mut e, 100003);
    assert!(!e.state.workspaces.contains_key(&100001));
    assert!(!e.state.workspaces.contains_key(&100002));
    assert!(!e.state.sessions.contains_key(&1));
}

// ---- moving a workspace to another session ------------------------------------------

#[test]
fn a_workspace_moves_to_another_session_on_the_same_monitor() {
    let mut e = engine("movesame");
    assert!(e.handle(Request::MoveWorkspaceToSession { id: 100002, session: 2 }).ok);
    let w = &e.state.workspaces[&100002];
    assert_eq!(w.session, 2);
    assert_eq!(w.number, 2, "lowest free number in the session");
    assert!(e.hypr().dispatched().iter().all(|d| !d.contains("workspace.move")), "same monitor: no move");
}

#[test]
fn a_workspace_moves_to_a_session_on_another_monitor() {
    let mut e = engine("moveother");
    assert!(e.handle(Request::MoveWorkspaceToSession { id: 100002, session: 3 }).ok);
    let d = e.hypr().dispatched();
    assert!(d.contains(&move_workspace(100002, "DP-2")));
    assert!(d.contains(&change_id(100002, 200002)), "renumbered into that monitor's range");
    let w = &e.state.workspaces[&200002];
    assert_eq!((w.session, w.number), (3, 2));
}

#[test]
fn moving_a_sessions_last_workspace_away_removes_the_session() {
    let mut e = engine("movelast");
    assert!(e.handle(Request::MoveWorkspaceToSession { id: 100003, session: 1 }).ok);
    assert!(!e.state.sessions.contains_key(&2));
    assert_eq!(e.state.workspaces[&100003].number, 3);
}

#[test]
fn moving_to_the_same_or_an_unknown_session_is_refused() {
    let mut e = engine("moveinvalid");
    assert!(!e.handle(Request::MoveWorkspaceToSession { id: 100002, session: 1 }).ok);
    assert!(!e.handle(Request::MoveWorkspaceToSession { id: 100002, session: 9 }).ok);
    assert!(!e.handle(Request::MoveWorkspaceToSession { id: 999, session: 2 }).ok);
}
