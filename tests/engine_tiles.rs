mod common;
use ymux::config::Config;
use ymux::engine::{Engine, Request};
use ymux::events::Event;
use ymux::hypr::{close_window, move_window};
use ymux::model::{State, Tile};
use common::*;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ymux-tiles-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// One monitor "LG A" (DP-1) showing 100001 with foot 0xa1; e.exe = "cm".
fn engine(name: &str) -> Engine<FakeHypr> {
    let mut s = State::default();
    s.ensure_monitor("LG A", "DP-1");
    let a = s.create_session("LG A", None);
    s.monitor_mut("LG A").unwrap().active_session = Some(a);
    s.monitor_mut("LG A").unwrap().shown = Some(100001);
    s.workspaces.get_mut(&100001).unwrap().tiles.push(Tile { address: "0xa1".into(), pid: Some(1), class: "foot".into(), ..Default::default() });
    let h = FakeHypr::default();
    *h.monitors.borrow_mut() = vec![monitor("DP-1", "LG A", 0, true, 100001)];
    *h.workspaces.borrow_mut() = vec![workspace(100001, "DP-1", 1)];
    *h.clients.borrow_mut() = vec![client("0xa1", 1, "foot", 100001)];
    let mut e = Engine::new(s, Config::default(), h, "sig".into(), dir(name));
    e.exe = "cm".into();
    e
}

/// A placeholder window with `pid` appears on the stash.
fn open_ph(e: &mut Engine<FakeHypr>, pid: u32, addr: &str) {
    let mut c = client(addr, pid, "ymux-placeholder", -99);
    c.workspace_name = "special:ymux-stash".into();
    e.hypr().clients.borrow_mut().push(c);
    e.on_event(Event::OpenWindow { address: addr.into(), workspace_name: "special:ymux-stash".into(), class: "ymux-placeholder".into(), title: "ymux placeholder".into() }).unwrap();
}

fn ph_argv() -> Vec<String> {
    vec!["cm".into(), "placeholder".into()]
}

#[test]
fn new_workspace_spawns_a_placeholder_and_fills_it() {
    let mut e = engine("new");
    e.handle(Request::NewWorkspace);
    assert_eq!(e.hypr().spawned.borrow().clone(), vec![ph_argv()]);
    open_ph(&mut e, 9000, "0xp");
    assert!(e.hypr().dispatched().contains(&move_window("0xp", 100002, false)));
    let w = &e.state.workspaces[&100002];
    assert_eq!(w.tiles.len(), 1);
    assert!(w.tiles[0].is_placeholder());
}

#[test]
fn floating_default_gets_no_placeholder() {
    let mut e = engine("floatdef");
    fs::write(e.layouts_dir().join("100002.lua"), "o.workspace_mode({ workspace = \"100002\", mode = \"floating\" })\n").unwrap();
    e.handle(Request::NewWorkspace);
    assert!(e.hypr().spawned.borrow().is_empty());
}

#[test]
fn new_session_workspace_starts_with_placeholder() {
    let mut e = engine("newsess");
    e.handle(Request::NewSession { name: Some("Work".into()) });
    assert_eq!(e.hypr().spawned.borrow().len(), 1);
}

#[test]
fn monitor_main_from_invariant_gets_placeholder() {
    let mut e = engine("mon");
    e.hypr().monitors.borrow_mut().push(monitor("DP-2", "Dell B", 2560, false, 7));
    e.enforce_invariant().unwrap();
    assert_eq!(e.hypr().spawned.borrow().len(), 1);
}

#[test]
fn busy_workspace_is_not_deleted_mid_job() {
    let mut e = engine("busy");
    let id = e.state.create_workspace(1);
    e.state.workspaces.get_mut(&id).unwrap().tiles.push(Tile { address: "0xz".into(), class: "foot".into(), ..Default::default() });
    e.spawn_fill_placeholder(id).unwrap();
    e.on_event(Event::CloseWindow { address: "0xz".into() }).unwrap();
    assert!(e.state.workspaces.contains_key(&id));
    e.on_deadline(Instant::now() + Duration::from_secs(6)).unwrap();
    assert!(!e.state.workspaces.contains_key(&id));
}

#[test]
fn kill_tile_on_placeholder_closes_at_once() {
    let mut e = engine("killph");
    e.state.workspaces.get_mut(&100001).unwrap().tiles.push(Tile { address: "0xp".into(), class: "ymux-placeholder".into(), ..Default::default() });
    e.hypr().clients.borrow_mut().push(client("0xp", 9, "ymux-placeholder", 100001));
    *e.hypr().active.borrow_mut() = Some("0xp".into());
    e.handle(Request::Action { name: "kill-tile".into() });
    assert_eq!(e.hypr().dispatched(), vec![close_window("0xp")]);
    assert!(e.hypr().spawned.borrow().is_empty());
}

#[test]
fn kill_tile_on_app_closes_without_asking() {
    let mut e = engine("killapp");
    *e.hypr().active.borrow_mut() = Some("0xa1".into());
    e.handle(Request::Action { name: "kill-tile".into() });
    // no confirmation card, and the app is asked where it stands
    assert_eq!(e.hypr().dispatched(), vec![close_window("0xa1")]);
}

#[test]
fn kill_tile_on_floating_app_closes_at_once() {
    let mut e = engine("killfloat");
    e.state.workspaces.get_mut(&100001).unwrap().tiles[0].floating = true;
    *e.hypr().active.borrow_mut() = Some("0xa1".into());
    e.handle(Request::Action { name: "kill-tile".into() });
    assert!(e.hypr().spawned.borrow().is_empty());
    assert_eq!(e.hypr().dispatched(), vec![close_window("0xa1")]);
}

#[test]
fn kill_tile_on_untracked_window_closes_at_once() {
    let mut e = engine("killuntracked");
    *e.hypr().active.borrow_mut() = Some("0xzz".into());
    e.handle(Request::Action { name: "kill-tile".into() });
    assert!(e.hypr().spawned.borrow().is_empty());
    assert_eq!(e.hypr().dispatched(), vec![close_window("0xzz")]);
}


#[test]
fn closing_placeholder_of_empty_unshown_workspace_deletes_it() {
    let mut e = engine("phlife");
    let id = e.state.create_workspace(1);
    e.state.workspaces.get_mut(&id).unwrap().tiles.push(Tile { address: "0xp".into(), class: "ymux-placeholder".into(), ..Default::default() });
    e.on_event(Event::CloseWindow { address: "0xp".into() }).unwrap();
    assert!(!e.state.workspaces.contains_key(&id));
}

#[test]
fn stash_orphans_closed_at_start() {
    let mut e = engine("orphan");
    let mut c = client("0xold", 77, "ymux-placeholder", -99);
    c.workspace_name = "special:ymux-stash".into();
    e.hypr().clients.borrow_mut().push(c);
    e.start().unwrap();
    assert!(e.hypr().dispatched().contains(&close_window("0xold")));
}

use ymux::hypr::{focus_window, swap_windows};

fn ph_tile(addr: &str, at: (i32, i32)) -> Tile {
    Tile { address: addr.into(), class: "ymux-placeholder".into(), at, ..Default::default() }
}

#[test]
fn move_from_dwindle_leaves_placeholder_then_moves_app() {
    let mut e = engine("leave");
    *e.hypr().active.borrow_mut() = Some("0xa1".into());
    e.handle(Request::Move { n: 2, follow: true });
    assert_eq!(e.hypr().spawned.borrow().clone(), vec![ph_argv()]);
    open_ph(&mut e, 9000, "0xp");
    assert_eq!(e.hypr().dispatched(), vec![swap_windows("0xp", "0xa1"), move_window("0xa1", 100002, true)]);
    assert!(e.state.workspaces[&100001].tiles.iter().any(|t| t.address == "0xp"));
    assert!(e.state.workspaces[&100002].tiles.iter().any(|t| t.address == "0xa1"));
    assert!(!e.state.workspaces[&100001].tiles.iter().any(|t| t.address == "0xa1"));
}

#[test]
fn move_from_floating_leaves_nothing() {
    let mut e = engine("leavefloat");
    fs::write(e.layouts_dir().join("100001.lua"), "o.workspace_mode({ workspace = \"100001\", mode = \"floating\" })\n").unwrap();
    *e.hypr().active.borrow_mut() = Some("0xa1".into());
    e.handle(Request::Move { n: 2, follow: false });
    assert!(e.hypr().spawned.borrow().is_empty());
    assert_eq!(e.hypr().dispatched(), vec![move_window("0xa1", 100002, false)]);
}

#[test]
fn move_into_workspace_with_placeholder_fills_top_left_first() {
    let mut e = engine("fill");
    fs::write(e.layouts_dir().join("100001.lua"), "o.workspace_mode({ workspace = \"100001\", mode = \"floating\" })\n").unwrap();
    let t = e.state.create_workspace(1);
    e.state.workspaces.get_mut(&t).unwrap().tiles = vec![ph_tile("0xp2", (900, 0)), ph_tile("0xp1", (0, 0))];
    *e.hypr().active.borrow_mut() = Some("0xa1".into());
    e.handle(Request::Move { n: 2, follow: true });
    assert_eq!(
        e.hypr().dispatched(),
        vec![swap_windows("0xp1", "0xa1"), close_window("0xp1"), focus_window("0xa1")]
    );
}

#[test]
fn move_on_placeholder_does_nothing() {
    let mut e = engine("moveph");
    e.state.workspaces.get_mut(&100001).unwrap().tiles.push(ph_tile("0xp", (0, 0)));
    e.hypr().clients.borrow_mut().push(client("0xp", 9, "ymux-placeholder", 100001));
    *e.hypr().active.borrow_mut() = Some("0xp".into());
    e.handle(Request::Move { n: 2, follow: true });
    assert!(e.hypr().spawned.borrow().is_empty());
    assert!(e.hypr().dispatched().is_empty());
}

#[test]
fn move_to_own_workspace_does_nothing() {
    let mut e = engine("moveown");
    *e.hypr().active.borrow_mut() = Some("0xa1".into());
    e.handle(Request::Move { n: 1, follow: true });
    assert!(e.hypr().spawned.borrow().is_empty());
    assert!(e.hypr().dispatched().is_empty());
}

#[test]
fn source_kept_while_app_swaps_through_the_stash() {
    let mut e = engine("transit");
    let src = e.state.create_workspace(1); // unshown source holding only 0xs
    e.state.workspaces.get_mut(&src).unwrap().tiles.push(Tile { address: "0xs".into(), class: "foot".into(), ..Default::default() });
    e.hypr().clients.borrow_mut().push(client("0xs", 5, "foot", src));
    e.move_app("0xs", src, 100001, false).unwrap();
    e.on_event(Event::MoveWindow { address: "0xs".into(), workspace: -96, workspace_name: "special:ymux-stash".into() }).unwrap();
    assert!(e.state.workspaces.contains_key(&src));
}

#[test]
fn opened_app_takes_focused_placeholder_spot() {
    let mut e = engine("fillopen");
    e.state.workspaces.get_mut(&100001).unwrap().tiles.push(ph_tile("0xp", (0, 0)));
    e.on_event(Event::ActiveWindow { address: Some("0xp".into()) }).unwrap();
    e.hypr().clients.borrow_mut().push(client("0xn", 50, "kitty", 100001));
    e.on_event(Event::OpenWindow { address: "0xn".into(), workspace_name: "100001".into(), class: "kitty".into(), title: "k".into() }).unwrap();
    assert_eq!(e.hypr().dispatched(), vec![swap_windows("0xp", "0xn"), close_window("0xp")]);
}

#[test]
fn opened_app_after_focus_event_still_fills() {
    let mut e = engine("fillopen2");
    e.state.workspaces.get_mut(&100001).unwrap().tiles.push(ph_tile("0xp", (0, 0)));
    e.on_event(Event::ActiveWindow { address: Some("0xp".into()) }).unwrap();
    e.on_event(Event::ActiveWindow { address: Some("0xn".into()) }).unwrap();
    e.hypr().clients.borrow_mut().push(client("0xn", 50, "kitty", 100001));
    e.on_event(Event::OpenWindow { address: "0xn".into(), workspace_name: "100001".into(), class: "kitty".into(), title: "k".into() }).unwrap();
    assert_eq!(e.hypr().dispatched(), vec![swap_windows("0xp", "0xn"), close_window("0xp")]);
}

#[test]
fn dialog_or_spawned_or_other_workspace_never_fills() {
    for case in ["floating", "spawned", "elsewhere"] {
        let mut e = engine(&format!("nofill-{case}"));
        let ph_ws = if case == "elsewhere" { e.state.create_workspace(1) } else { 100001 };
        e.state.workspaces.get_mut(&ph_ws).unwrap().tiles.push(ph_tile("0xp", (0, 0)));
        e.on_event(Event::ActiveWindow { address: Some("0xp".into()) }).unwrap();
        let mut c = client("0xn", 50, "kitty", 100001);
        c.floating = case == "floating";
        e.hypr().clients.borrow_mut().push(c);
        if case == "spawned" {
            e.note_spawned(50);
        }
        e.on_event(Event::OpenWindow { address: "0xn".into(), workspace_name: "100001".into(), class: "kitty".into(), title: "k".into() }).unwrap();
        assert!(!e.hypr().dispatched().iter().any(|d| d.contains("swap")), "{case}");
    }
}

#[test]
fn mark_toggles_and_shows_in_view() {
    let mut e = engine("mark");
    *e.hypr().active.borrow_mut() = Some("0xa1".into());
    assert!(e.handle(Request::Action { name: "mark".into() }).ok);
    assert_eq!(e.state.mark.as_deref(), Some("0xa1"));
    assert!(e.view().unwrap().marked);
    assert!(e.take_dirty());
    e.handle(Request::Action { name: "mark".into() });
    assert_eq!(e.state.mark, None);
}

#[test]
fn swap_with_mark_across_workspaces() {
    let mut e = engine("swap");
    let other = e.state.create_workspace(1);
    e.state.workspaces.get_mut(&other).unwrap().tiles.push(Tile { address: "0xa2".into(), class: "kitty".into(), at: (5, 5), ..Default::default() });
    e.hypr().clients.borrow_mut().push(client("0xa2", 2, "kitty", other));
    e.state.mark = Some("0xa2".into());
    *e.hypr().active.borrow_mut() = Some("0xa1".into());
    assert!(e.handle(Request::Action { name: "swap".into() }).ok);
    assert_eq!(e.hypr().dispatched(), vec![swap_windows("0xa2", "0xa1")]);
    assert!(e.state.workspaces[&100001].tiles.iter().any(|t| t.address == "0xa2"));
    assert!(e.state.workspaces[&other].tiles.iter().any(|t| t.address == "0xa1"));
    assert_eq!(e.state.mark, None);
}

#[test]
fn swap_without_mark_notifies() {
    let mut e = engine("swapnone");
    *e.hypr().active.borrow_mut() = Some("0xa1".into());
    let r = e.handle(Request::Action { name: "swap".into() });
    assert!(!r.ok);
    assert_eq!(r.message.unwrap(), "ymux: no marked tile");
    assert!(e.hypr().dispatched().is_empty());
}

#[test]
fn swap_never_closes_a_placeholder() {
    let mut e = engine("swapph");
    let other = e.state.create_workspace(1);
    e.state.workspaces.get_mut(&other).unwrap().tiles.push(ph_tile("0xp", (0, 0)));
    e.hypr().clients.borrow_mut().push(client("0xp", 9, "ymux-placeholder", other));
    e.state.mark = Some("0xp".into());
    *e.hypr().active.borrow_mut() = Some("0xa1".into());
    e.handle(Request::Action { name: "swap".into() });
    assert_eq!(e.hypr().dispatched(), vec![swap_windows("0xp", "0xa1")]);
}

#[test]
fn closing_marked_window_clears_mark() {
    let mut e = engine("markclose");
    e.state.mark = Some("0xa1".into());
    e.on_event(Event::CloseWindow { address: "0xa1".into() }).unwrap();
    assert_eq!(e.state.mark, None);
}

// ---- Leader + x / Super + W: ask in place, keep the spot once closed --------------
// (user decision 2026-11-16: Brave's "close 2 tabs?" must show where Brave is)

#[test]
fn closing_an_app_asks_it_in_place_first() {
    let mut e = engine("askinplace");
    e.handle(Request::CloseWindow { address: "0xa1".into() });
    assert_eq!(e.hypr().dispatched(), vec![close_window("0xa1")], "nothing hidden, nothing swapped");
    assert!(e.hypr().spawned.borrow().is_empty(), "no spot until it really closes");
}

#[test]
fn an_app_that_does_not_close_stays_where_it_is() {
    let mut e = engine("staysput");
    e.handle(Request::CloseWindow { address: "0xa1".into() });
    e.on_deadline(Instant::now() + Duration::from_secs(3600)).unwrap();
    assert_eq!(e.hypr().dispatched(), vec![close_window("0xa1")]);
    assert!(e.hypr().notified.borrow().is_empty());
    assert!(e.state.workspaces[&100001].tiles.iter().any(|t| t.address == "0xa1"));
}

fn really_close(e: &mut Engine<FakeHypr>, addr: &str) {
    e.hypr().clients.borrow_mut().retain(|c| c.address != addr);
    e.on_event(Event::CloseWindow { address: addr.into() }).unwrap();
}

#[test]
fn once_the_app_closes_a_placeholder_takes_its_spot() {
    let mut e = engine("spotafter");
    e.handle(Request::CloseWindow { address: "0xa1".into() });
    really_close(&mut e, "0xa1");
    assert_eq!(e.hypr().spawned.borrow().clone(), vec![ph_argv()]);
    open_ph(&mut e, 9000, "0xp");
    assert!(e.hypr().dispatched().contains(&move_window("0xp", 100001, false)));
    assert!(e.state.workspaces[&100001].tiles.iter().any(|t| t.address == "0xp" && t.is_placeholder()));
}

#[test]
fn the_spot_is_restored_from_the_layout_recorded_at_close() {
    let mut e = engine("spotlayout");
    e.handle(Request::CloseWindow { address: "0xa1".into() });
    really_close(&mut e, "0xa1");
    open_ph(&mut e, 9000, "0xp");
    // the layout to restore names the placeholder where the app was
    let w = &e.state.workspaces[&100001];
    assert!(w.archived_layout.iter().any(|t| t.address == "0xp"));
    assert!(!w.archived_layout.iter().any(|t| t.address == "0xa1"));
    e.on_deadline(Instant::now() + Duration::from_secs(2)).unwrap();
    assert!(e.state.workspaces[&100001].archived_layout.is_empty(), "restored");
}

#[test]
fn the_last_app_of_an_unshown_workspace_keeps_it_for_its_spot() {
    let mut e = engine("keepws");
    e.state.create_workspace(1); // 100002, not shown
    e.state.workspaces.get_mut(&100002).unwrap().tiles.push(Tile {
        address: "0xb".into(),
        pid: Some(2),
        class: "foot".into(),
        ..Default::default()
    });
    e.hypr().workspaces.borrow_mut().push(workspace(100002, "DP-1", 1));
    e.hypr().clients.borrow_mut().push(client("0xb", 2, "foot", 100002));
    e.handle(Request::CloseWindow { address: "0xb".into() });
    really_close(&mut e, "0xb");
    assert!(e.state.workspaces.contains_key(&100002));
    assert_eq!(e.hypr().spawned.borrow().len(), 1);
}

#[test]
fn an_app_moved_away_before_it_closes_leaves_no_spot() {
    let mut e = engine("movedaway");
    e.state.create_workspace(1); // 100002
    e.hypr().workspaces.borrow_mut().push(workspace(100002, "DP-1", 0));
    e.handle(Request::CloseWindow { address: "0xa1".into() });
    e.hypr().clients.borrow_mut()[0].workspace = 100002;
    e.on_event(Event::MoveWindow { address: "0xa1".into(), workspace: 100002, workspace_name: "100002".into() }).unwrap();
    really_close(&mut e, "0xa1");
    assert!(e.hypr().spawned.borrow().is_empty());
}

#[test]
fn closing_app_on_floating_workspace_just_closes_it() {
    let mut e = engine("closefloat");
    fs::write(e.layouts_dir().join("100001.lua"), "o.workspace_mode({ workspace = \"100001\", mode = \"floating\" })\n").unwrap();
    e.handle(Request::CloseWindow { address: "0xa1".into() });
    really_close(&mut e, "0xa1");
    assert!(e.hypr().spawned.borrow().is_empty());
    assert_eq!(e.hypr().dispatched(), vec![close_window("0xa1")]);
}

// ---- review minors 5 and 8 --------------------------------------------------------

#[test]
fn move_from_a_special_workspace_leaves_no_placeholder() {
    let mut e = engine("leavespecial");
    let mut c = client("0xq", 5, "foot", -98);
    c.workspace_name = "special:scratchpad".into();
    e.hypr().clients.borrow_mut().push(c);
    *e.hypr().active.borrow_mut() = Some("0xq".into());
    e.handle(Request::Move { n: 1, follow: false });
    assert!(e.hypr().spawned.borrow().is_empty());
    assert!(e.hypr().dispatched().contains(&move_window("0xq", 100001, false)));
}

#[test]
fn spawned_pids_are_forgotten_once_their_windows_are_gone() {
    let mut e = engine("prune");
    e.note_spawned(4242);
    e.tick().unwrap();
    // pid 4242 has no window and no job: a later window reusing it is recorded
    let c = client("0xr", 4242, "foot", 100001);
    e.hypr().clients.borrow_mut().push(c);
    e.tick().unwrap();
    assert!(e.state.workspaces[&100001].tiles.iter().any(|t| t.address == "0xr"));
}
