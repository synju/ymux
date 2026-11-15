mod common;
use ymux::config::Config;
use ymux::engine::{Engine, Request};
use ymux::hypr::{focus_workspace, move_window};
use ymux::model::State;
use common::*;

/// One monitor "LG A" (DP-1), focused on 100001; session 1 owns 100001 (1) and 100002 (2).
fn engine() -> Engine<FakeHypr> {
    let mut s = State::default();
    s.ensure_monitor("LG A", "DP-1");
    let a = s.create_session("LG A", None);
    s.create_workspace(a);
    s.monitor_mut("LG A").unwrap().active_session = Some(a);
    let h = FakeHypr::default();
    *h.monitors.borrow_mut() = vec![monitor("DP-1", "LG A", 0, true, 100001)];
    *h.workspaces.borrow_mut() = vec![workspace(100001, "DP-1", 1)];
    *h.clients.borrow_mut() = vec![client("0xab", 7, "foot", 100001)];
    Engine::new(s, Config::default(), h, "sig".into(), unique_dir("test-layouts"))
}

/// Move tests are about the target, not placeholders: make the source floating so
/// the move is direct (placeholders have their own tests in engine_tiles.rs).
fn floating_source(e: &Engine<FakeHypr>) {
    let d = e.layouts_dir();
    std::fs::create_dir_all(d).unwrap();
    std::fs::write(d.join("100001.lua"), "o.workspace_mode({ workspace = \"100001\", mode = \"floating\" })\n").unwrap();
}

fn engine_on(id: i64) -> Engine<FakeHypr> {
    let e = engine();
    e.hypr().monitors.borrow_mut()[0].active_workspace = id;
    e
}

#[test]
fn focus_existing_number_dispatches_real_id() {
    let mut e = engine();
    assert!(e.handle(Request::Focus { n: 2 }).ok);
    assert_eq!(e.hypr().dispatched(), vec![focus_workspace(100002)]);
}

#[test]
fn focus_missing_number_does_nothing() {
    let mut e = engine();
    let before = e.state.clone();
    let r = e.handle(Request::Focus { n: 7 });
    assert!(r.ok);
    assert!(e.hypr().dispatched().is_empty());
    assert_eq!(e.state, before);
}

#[test]
fn move_follows_by_default() {
    let mut e = engine();
    floating_source(&e);
    *e.hypr().active.borrow_mut() = Some("0xab".into());
    e.handle(Request::Move { n: 2, follow: true });
    assert_eq!(e.hypr().dispatched(), vec![move_window("0xab", 100002, true)]);
}

#[test]
fn move_silent_keeps_focus() {
    let mut e = engine();
    floating_source(&e);
    *e.hypr().active.borrow_mut() = Some("0xab".into());
    e.handle(Request::Move { n: 2, follow: false });
    assert_eq!(e.hypr().dispatched(), vec![move_window("0xab", 100002, false)]);
}

#[test]
fn move_to_missing_number_creates_that_workspace() {
    let mut e = engine();
    floating_source(&e);
    *e.hypr().active.borrow_mut() = Some("0xab".into());
    e.handle(Request::Move { n: 9, follow: true });
    let w = e.state.workspace_by_number(1, 9).expect("workspace 9 created");
    assert_eq!(w.real_id, 100003);
    assert_eq!(e.hypr().dispatched(), vec![move_window("0xab", 100003, true)]);
    assert!(e.take_dirty());
}

#[test]
fn move_without_window_does_nothing_and_creates_nothing() {
    let mut e = engine();
    let before = e.state.clone();
    e.handle(Request::Move { n: 9, follow: true });
    e.handle(Request::Move { n: 2, follow: true });
    assert!(e.hypr().dispatched().is_empty());
    assert_eq!(e.state, before);
}

#[test]
fn next_and_prev_step_within_session() {
    let mut e = engine_on(100001);
    e.handle(Request::Next);
    assert_eq!(e.hypr().dispatched(), vec![focus_workspace(100002)]);
    let mut e = engine_on(100002);
    e.handle(Request::Prev);
    assert_eq!(e.hypr().dispatched(), vec![focus_workspace(100001)]);
}

#[test]
fn next_wraps_from_last_to_first() {
    let mut e = engine_on(100002);
    e.handle(Request::Next);
    assert_eq!(e.hypr().dispatched(), vec![focus_workspace(100001)]);
}

#[test]
fn prev_wraps_from_first_to_last() {
    let mut e = engine_on(100001);
    e.handle(Request::Prev);
    assert_eq!(e.hypr().dispatched(), vec![focus_workspace(100002)]);
}

#[test]
fn single_workspace_session_steps_nowhere() {
    let mut e = engine_on(100001);
    e.state.workspaces.remove(&100002);
    e.handle(Request::Next);
    e.handle(Request::Prev);
    assert!(e.hypr().dispatched().is_empty());
}

#[test]
fn previous_uses_pointer_within_session() {
    let mut e = engine_on(100002);
    e.state.workspaces.get_mut(&100002).unwrap().previous = Some(100001);
    e.handle(Request::Previous);
    assert_eq!(e.hypr().dispatched(), vec![focus_workspace(100001)]);
}

#[test]
fn new_workspace_takes_lowest_free_number_and_focuses_it() {
    let mut e = engine();
    e.state.workspaces.remove(&100002);
    e.handle(Request::NewWorkspace);
    assert_eq!(e.state.workspace_by_number(1, 2).unwrap().real_id, 100002);
    assert_eq!(e.hypr().dispatched(), vec![focus_workspace(100002)]);
    assert!(e.take_dirty());
}

#[test]
fn new_session_named_and_active() {
    let mut e = engine();
    e.handle(Request::NewSession { name: Some("Work".into()) });
    let s = e.state.sessions.values().find(|s| s.name == "Work").unwrap().number;
    assert_eq!(e.state.monitor("LG A").unwrap().active_session, Some(s));
    let ws = e.state.session_workspaces(s)[0].real_id;
    assert_eq!(e.hypr().dispatched(), vec![focus_workspace(ws)]);
}

#[test]
fn activation_bumps_last_used() {
    let mut e = engine();
    let before = e.state.sessions[&1].last_used;
    e.handle(Request::NewSession { name: None });
    e.handle(Request::SwitchSession { number: 1 });
    assert!(e.state.sessions[&1].last_used > before);
    assert!(e.state.sessions[&1].last_used > e.state.sessions[&2].last_used);
}

#[test]
fn switch_session_focuses_last_shown() {
    let mut e = engine();
    e.handle(Request::NewSession { name: None });
    e.hypr().clear();
    e.state.sessions.get_mut(&1).unwrap().last_shown = Some(100002);
    e.handle(Request::SwitchSession { number: 1 });
    assert_eq!(e.hypr().dispatched(), vec![focus_workspace(100002)]);
    assert_eq!(e.state.monitor("LG A").unwrap().active_session, Some(1));
}

#[test]
fn status_lists_sessions_per_monitor() {
    let r = engine().handle(Request::Status);
    assert!(r.ok);
    let m = r.message.unwrap();
    assert!(m.contains("DP-1 LG A"), "{m}");
    assert!(m.contains("1: 1 workspaces 1=100001 2=100002"), "{m}");
}

#[test]
fn action_names_map_to_requests() {
    let mut e = engine_on(100001);
    assert!(e.handle(Request::Action { name: "next-workspace".into() }).ok);
    assert_eq!(e.hypr().dispatched(), vec![focus_workspace(100002)]);
}

#[test]
fn unknown_action_says_so() {
    let r = engine().handle(Request::Action { name: "fly".into() });
    assert!(!r.ok);
    assert_eq!(r.message.unwrap(), "ymux: unknown action fly");
}
