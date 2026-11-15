mod common;
use ymux::config::Config;
use ymux::engine::Engine;
use ymux::events::Event;
use ymux::hypr::change_id;
use ymux::layout::read_mode;
use ymux::model::{State, Tile};
use common::*;
use std::fs;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ymux-englayout-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// One monitor "LG A" (DP-1) showing 100001; session 1 owns 100001 and 100002 (tile 0xa2).
fn engine(layouts: PathBuf) -> Engine<FakeHypr> {
    let mut s = State::default();
    s.ensure_monitor("LG A", "DP-1");
    let a = s.create_session("LG A", None);
    s.create_workspace(a);
    s.monitor_mut("LG A").unwrap().active_session = Some(a);
    s.monitor_mut("LG A").unwrap().shown = Some(100001);
    s.workspaces.get_mut(&100002).unwrap().tiles.push(Tile {
        address: "0xa2".into(), pid: Some(12), class: "foot".into(), title: "t".into(), floating: false, at: (0, 0), size: (1, 1), ..Default::default()
    });
    let h = FakeHypr::default();
    *h.monitors.borrow_mut() = vec![monitor("DP-1", "LG A", 0, true, 100001)];
    *h.workspaces.borrow_mut() = vec![workspace(100001, "DP-1", 0), workspace(100002, "DP-1", 1)];
    *h.clients.borrow_mut() = vec![client("0xa2", 12, "foot", 100002)];
    Engine::new(s, Config::default(), h, "sig".into(), layouts)
}

#[test]
fn deleting_a_workspace_deletes_its_mode_file() {
    let d = dir("del");
    fs::write(d.join("100002.lua"), "o.workspace_mode({ workspace = \"100002\", mode = \"scrolling\" })\n").unwrap();
    let mut e = engine(d.clone());
    e.on_event(Event::CloseWindow { address: "0xa2".into() }).unwrap();
    assert!(!d.join("100002.lua").exists());
    assert_eq!(e.state.create_workspace(1), 100002);
    assert_eq!(read_mode(&d, 100002, None, || None), "dwindle");
}

#[test]
fn renumber_dispatches_moves_file_and_rekeys_state() {
    let d = dir("ren");
    fs::write(d.join("100002.lua"), "o.workspace_mode({ workspace = \"100002\", mode = \"scrolling\" })\n").unwrap();
    let mut e = engine(d.clone());
    e.renumber(100002, 100005).unwrap();
    assert_eq!(e.hypr().dispatched(), vec![change_id(100002, 100005)]);
    assert!(e.state.workspaces.contains_key(&100005));
    assert!(!e.state.workspaces.contains_key(&100002));
    assert_eq!(read_mode(&d, 100005, None, || None), "scrolling");
}

#[test]
fn live_adoption_renumbers_into_range() {
    let mut e = engine(dir("adopt"));
    e.hypr().workspaces.borrow_mut().push(workspace(5, "DP-1", 1));
    e.hypr().clients.borrow_mut().push(client("0xz", 51, "foot", 5));
    e.on_event(Event::OpenWindow { address: "0xz".into(), workspace_name: "5".into(), class: "foot".into(), title: "z".into() }).unwrap();
    assert_eq!(e.hypr().dispatched(), vec![change_id(5, 100003)]);
    let w = &e.state.workspaces[&100003];
    assert_eq!((w.session, w.number), (1, 3));
    assert!(w.tiles.iter().any(|t| t.address == "0xz"));
}

#[test]
fn outside_change_id_is_renumbered_back_into_range() {
    let mut e = engine(dir("outside"));
    *e.hypr().workspaces.borrow_mut() = vec![workspace(100001, "DP-1", 0), workspace(7, "DP-1", 1)];
    e.on_event(Event::ChangeWorkspaceId { old: 100002, new: 7 }).unwrap();
    assert_eq!(e.hypr().dispatched(), vec![change_id(7, 100002)]);
    assert_eq!(e.state.workspaces[&100002].number, 2);
}

#[test]
fn tick_renumber_check_follows_outside_change_id_only_when_safe() {
    let mut e = engine(dir("tick"));
    // 0xa2's workspace now reported under 7 and 100002 is gone
    *e.hypr().workspaces.borrow_mut() = vec![workspace(100001, "DP-1", 0), workspace(7, "DP-1", 1)];
    *e.hypr().clients.borrow_mut() = vec![client("0xa2", 12, "foot", 7)];
    e.tick().unwrap();
    assert!(e.hypr().dispatched().contains(&change_id(7, 100002)));
    assert_eq!(e.state.workspaces[&100002].number, 2);
    // while 100002 still exists, nothing happens
    let mut e = engine(dir("tick2"));
    *e.hypr().workspaces.borrow_mut() = vec![workspace(100001, "DP-1", 0), workspace(100002, "DP-1", 0), workspace(7, "DP-1", 1)];
    *e.hypr().clients.borrow_mut() = vec![client("0xa2", 12, "foot", 7)];
    e.tick().unwrap();
    assert!(!e.hypr().dispatched().iter().any(|d| d.contains("change_id")));
}

#[test]
fn renumber_reapplies_live_mode() {
    let d = dir("reapply");
    fs::write(d.join("100002.lua"), "o.workspace_mode({ workspace = \"100002\", mode = \"scrolling\" })\n").unwrap();
    let mut e = engine(d);
    e.renumber(100002, 100005).unwrap();
    assert_eq!(e.hypr().evaled.borrow().clone(), vec!["o.workspace_mode({ workspace = \"100005\", mode = \"scrolling\" })".to_string()]);
}

#[test]
fn tick_records_windows_missed_by_events() {
    let mut e = engine(dir("rescan"));
    e.hypr().clients.borrow_mut().push(client("0xmiss", 77, "foot", 100001));
    e.tick().unwrap();
    assert!(e.state.workspaces[&100001].tiles.iter().any(|t| t.address == "0xmiss"));
}
