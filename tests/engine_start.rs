mod common;
use ymux::config::Config;
use ymux::engine::{Engine, StartKind};
use ymux::hypr::{change_id, focus_monitor, focus_workspace};
use ymux::layout::read_mode;
use ymux::model::{State, Tile};
use common::*;
use std::fs;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ymux-start-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// Live desktop like the user's: eDP-2 (laptop, x=1920) shows 1; DP-1 (x=0, focused)
/// shows 12 and also holds 13 and 14.
fn live() -> FakeHypr {
    let h = FakeHypr::default();
    *h.monitors.borrow_mut() = vec![monitor("eDP-2", "BOE", 1920, false, 1), monitor("DP-1", "HKC", 0, true, 12)];
    *h.workspaces.borrow_mut() = vec![workspace(1, "eDP-2", 2), workspace(12, "DP-1", 2), workspace(13, "DP-1", 1), workspace(14, "DP-1", 1), workspace(-98, "DP-1", 1)];
    *h.clients.borrow_mut() = vec![
        client("0x1", 1, "brave", 1),
        client("0x2", 2, "vivaldi", 1),
        client("0x3", 3, "ghostty", 12),
        client("0x4", 4, "claude", 12),
        client("0x5", 5, "steam", 13),
        client("0x6", 6, "spotify", 14),
        client("0x7", 7, "agent", -98),
    ];
    h
}

fn session_named(e: &Engine<FakeHypr>, name: &str, monitor: &str) -> u32 {
    e.state.sessions.values().find(|s| s.name == name && s.monitor == monitor).unwrap().number
}

fn nums(e: &Engine<FakeHypr>, s: u32) -> Vec<u32> {
    e.state.session_workspaces(s).iter().map(|w| w.number).collect()
}

#[test]
fn adoption_keeps_slots_and_makes_main_per_monitor() {
    let mut e = Engine::new(State::default(), Config::default(), live(), "sig".into(), dir("adopt"));
    assert_eq!(e.start().unwrap(), StartKind::Adopted);
    let boe = session_named(&e, "main", "BOE");
    let hkc = session_named(&e, "main", "HKC");
    assert_eq!(nums(&e, boe), vec![1]);
    assert_eq!(nums(&e, hkc), vec![2, 3, 4]);
    let d = e.hypr().dispatched();
    assert!(d.contains(&change_id(1, 100001)), "{d:?}");
    assert!(d.contains(&change_id(12, 200001)), "{d:?}");
    assert!(e.state.workspaces.keys().all(|id| *id > 100000));
    let w = e.state.workspace_by_number(hkc, 2).unwrap();
    assert_eq!(w.tiles.len(), 2);
    assert_eq!(e.state.monitor("HKC").unwrap().active_session, Some(hkc));
    assert_eq!(e.state.monitor("HKC").unwrap().shown, Some(w.real_id));
}

#[test]
fn adoption_keeps_scrolling_mode() {
    let d = dir("scroll");
    fs::write(d.join("1.lua"), "o.workspace_mode({ workspace = \"1\", mode = \"scrolling\" })\n").unwrap();
    let mut e = Engine::new(State::default(), Config::default(), live(), "sig".into(), d.clone());
    e.start().unwrap();
    assert!(!d.join("1.lua").exists());
    assert_eq!(read_mode(&d, 100001, None, || None), "scrolling");
}

#[test]
fn adoption_collision_takes_lowest_free_number() {
    let h = live();
    // guest 2 from an unplugged monitor sits on DP-1 next to 12: both map to number 2
    h.workspaces.borrow_mut().push(workspace(2, "DP-1", 1));
    h.clients.borrow_mut().push(client("0x8", 8, "foot", 2));
    let mut e = Engine::new(State::default(), Config::default(), h, "sig".into(), dir("collide"));
    e.start().unwrap();
    let hkc = session_named(&e, "main", "HKC");
    assert_eq!(nums(&e, hkc), vec![1, 2, 3, 4]);
}

/// State as the daemon saved it in this Hyprland session ("sig"), after adoption.
fn adopted_state() -> State {
    let mut e = Engine::new(State::default(), Config::default(), live(), "sig".into(), dir("base"));
    e.start().unwrap();
    e.state
}

/// Live desktop after adoption (ids renumbered as the adoption above dispatched).
fn live_after() -> FakeHypr {
    let h = live();
    *h.monitors.borrow_mut() = vec![monitor("eDP-2", "BOE", 1920, false, 100001), monitor("DP-1", "HKC", 0, true, 200001)];
    *h.workspaces.borrow_mut() = vec![workspace(100001, "eDP-2", 2), workspace(200001, "DP-1", 2), workspace(200002, "DP-1", 1), workspace(200003, "DP-1", 1)];
    for c in h.clients.borrow_mut().iter_mut() {
        c.workspace = match c.workspace { 1 => 100001, 12 => 200001, 13 => 200002, 14 => 200003, o => o };
        c.workspace_name = c.workspace.to_string();
    }
    h
}

#[test]
fn restart_keeps_sessions_and_does_not_relaunch_or_land() {
    let mut e = Engine::new(adopted_state(), Config::default(), live_after(), "sig".into(), dir("r1"));
    assert_eq!(e.start().unwrap(), StartKind::Restarted);
    assert!(e.hypr().dispatched().is_empty());
    assert_eq!(e.state.workspaces[&200001].tiles.len(), 2);
}

#[test]
fn restart_matches_by_address_and_pid() {
    let mut s = adopted_state();
    s.workspaces.get_mut(&200002).unwrap().tiles[0].pid = Some(999); // same address, other pid
    let mut e = Engine::new(s, Config::default(), live_after(), "sig".into(), dir("r2"));
    e.start().unwrap();
    let t: &Tile = &e.state.workspaces[&200002].tiles[0];
    assert_eq!(t.pid, Some(5));
    assert_eq!(e.state.workspaces[&200002].tiles.len(), 1);
}

#[test]
fn restart_drops_closed_apps_and_deletes_unshown_empty_workspace() {
    let h = live_after();
    h.clients.borrow_mut().retain(|c| c.address != "0x6"); // spotify closed while down
    h.workspaces.borrow_mut().retain(|w| w.id != 200003);
    let mut e = Engine::new(adopted_state(), Config::default(), h, "sig".into(), dir("r3"));
    e.start().unwrap();
    assert!(!e.state.workspaces.contains_key(&200003));
}

#[test]
fn restart_records_new_window_on_known_workspace() {
    let h = live_after();
    h.clients.borrow_mut().push(client("0xcd", 40, "foot", 200001));
    let mut e = Engine::new(adopted_state(), Config::default(), h, "sig".into(), dir("r4"));
    e.start().unwrap();
    assert!(e.state.workspaces[&200001].tiles.iter().any(|t| t.address == "0xcd"));
    assert_eq!(e.state.workspaces.len(), 4);
}

#[test]
fn restart_adopts_unknown_workspace_into_its_monitors_active_session() {
    let h = live_after();
    h.workspaces.borrow_mut().push(workspace(4, "DP-1", 1));
    h.clients.borrow_mut().push(client("0xee", 41, "foot", 4));
    let mut e = Engine::new(adopted_state(), Config::default(), h, "sig".into(), dir("r5"));
    e.start().unwrap();
    assert!(e.hypr().dispatched().contains(&change_id(4, 200004)));
    let hkc = session_named(&e, "main", "HKC");
    assert_eq!(e.state.workspaces[&200004].session, hkc);
}

#[test]
fn hyprland_started_lands_each_monitor_on_last_shown_first() {
    // new Hyprland: fresh default workspaces 1 and 2, no windows yet
    let h = FakeHypr::default();
    *h.monitors.borrow_mut() = vec![monitor("eDP-2", "BOE", 1920, false, 1), monitor("DP-1", "HKC", 0, true, 2)];
    *h.workspaces.borrow_mut() = vec![workspace(1, "eDP-2", 0), workspace(2, "DP-1", 0)];
    let mut s = adopted_state();
    let hkc = s.sessions.values().find(|x| x.monitor == "HKC").unwrap().number;
    s.sessions.get_mut(&hkc).unwrap().last_shown = Some(200002);
    let mut e = Engine::new(s, Config::default(), h, "new-sig".into(), dir("hs"));
    assert_eq!(e.start().unwrap(), StartKind::HyprlandStarted);
    let d = e.hypr().dispatched();
    assert_eq!(d[..4], [focus_monitor("eDP-2"), focus_workspace(100001), focus_monitor("DP-1"), focus_workspace(200002)]);
    // restored empty workspaces are kept (spec 5.6, 7.5)
    assert_eq!(e.state.workspaces.len(), 4);
    assert!(e.state.workspaces.values().all(|w| w.tiles.is_empty()));
}

#[test]
fn signature_saved_after_start() {
    let mut e = Engine::new(State::default(), Config::default(), live(), "sig-x".into(), dir("sig"));
    e.start().unwrap();
    assert_eq!(e.state.signature.as_deref(), Some("sig-x"));
    assert!(e.take_dirty());
}

#[test]
fn adoption_records_tiles_when_hyprland_already_reports_new_ids() {
    let h = live();
    // real Hyprland applies change_id at once: clients read after adoption show new ids
    for c in h.clients.borrow_mut().iter_mut() {
        c.workspace = match c.workspace { 1 => 100001, 12 => 200001, 13 => 200002, 14 => 200003, o => o };
    }
    let mut e = Engine::new(State::default(), Config::default(), h, "sig".into(), dir("newids"));
    e.start().unwrap();
    assert_eq!(e.state.workspaces[&200001].tiles.len(), 2);
    assert_eq!(e.state.workspaces[&100001].tiles.len(), 2);
}

#[test]
fn restart_keeps_unshown_workspaces_that_were_already_empty() {
    let mut s = adopted_state();
    let hkc = s.sessions.values().find(|x| x.monitor == "HKC").unwrap().number;
    let kept = s.create_workspace(hkc); // restored-empty, never shown since
    let mut e = Engine::new(s, Config::default(), live_after(), "sig".into(), dir("r6"));
    e.start().unwrap();
    assert!(e.state.workspaces.contains_key(&kept));
}
