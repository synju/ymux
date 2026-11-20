mod common;
use ymux::engine::Engine;
use ymux::model::{State, Tile};
use common::*;
use std::fs;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ymux-geo-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// One monitor "LG A" (DP-1) showing 100001 (tile 0xa1); 100002 holds 0xa2.
fn engine(layouts: PathBuf) -> Engine<FakeHypr> {
    let mut s = State::default();
    s.ensure_monitor("LG A", "DP-1");
    let a = s.create_session("LG A", None);
    s.create_workspace(a);
    s.monitor_mut("LG A").unwrap().active_session = Some(a);
    s.monitor_mut("LG A").unwrap().shown = Some(100001);
    for (id, addr) in [(100001, "0xa1"), (100002, "0xa2")] {
        let w = s.workspaces.get_mut(&id).unwrap();
        w.tiles.push(Tile { address: addr.into(), pid: Some(1), class: "foot".into(), size: (100, 100), ..Default::default() });
        w.mode = Some("dwindle".into());
    }
    let h = FakeHypr::default();
    *h.monitors.borrow_mut() = vec![monitor("DP-1", "LG A", 0, true, 100001)];
    *h.workspaces.borrow_mut() = vec![workspace(100001, "DP-1", 1), workspace(100002, "DP-1", 1)];
    *h.clients.borrow_mut() = vec![client("0xa1", 1, "foot", 100001), client("0xa2", 1, "foot", 100002)];
    Engine::new(s, auto_close(), h, "sig".into(), layouts)
}

fn set_live(e: &Engine<FakeHypr>, addr: &str, at: (i32, i32), size: (i32, i32), floating: bool) {
    let mut c = e.hypr().clients.borrow_mut();
    let w = c.iter_mut().find(|c| c.address == addr).unwrap();
    w.at = at;
    w.size = size;
    w.floating = floating;
}

#[test]
fn capture_updates_geometry_and_reports_change() {
    let mut e = engine(dir("cap"));
    set_live(&e, "0xa1", (10, 20), (300, 400), false);
    assert!(e.capture_geometry().unwrap());
    let t = &e.state.workspaces[&100001].tiles[0];
    assert_eq!((t.at, t.size), ((10, 20), (300, 400)));
    assert!(!e.capture_geometry().unwrap());
}

#[test]
fn capture_skips_workspace_whose_mode_changed() {
    let d = dir("skip");
    fs::write(d.join("100002.lua"), "o.workspace_mode({ workspace = \"100002\", mode = \"floating\" })\n").unwrap();
    let mut e = engine(d);
    set_live(&e, "0xa2", (50, 50), (900, 700), true);
    e.capture_geometry().unwrap();
    let t = &e.state.workspaces[&100002].tiles[0];
    assert_eq!((t.at, t.size, t.floating), ((0, 0), (100, 100), false));
}

#[test]
fn hand_floated_window_on_tiled_workspace_is_captured() {
    let mut e = engine(dir("float"));
    set_live(&e, "0xa1", (40, 40), (500, 300), true);
    e.capture_geometry().unwrap();
    let t = &e.state.workspaces[&100001].tiles[0];
    assert!(t.floating);
    assert_eq!(t.at, (40, 40));
}

#[test]
fn tick_runs_mode_check_before_capture() {
    let d = dir("tick");
    fs::write(d.join("100001.lua"), "o.workspace_mode({ workspace = \"100001\", mode = \"scrolling\" })\n").unwrap();
    let mut e = engine(d);
    e.state.workspaces.get_mut(&100001).unwrap().mode = None;
    set_live(&e, "0xa1", (7, 7), (200, 200), false);
    e.tick().unwrap();
    let w = &e.state.workspaces[&100001];
    assert_eq!(w.mode.as_deref(), Some("scrolling"));
    assert_eq!(w.tiles[0].at, (7, 7));
}

#[test]
fn capture_resyncs_tile_workspace() {
    let mut e = engine(dir("resync"));
    e.hypr().clients.borrow_mut()[1].workspace = 100001;
    e.capture_geometry().unwrap();
    assert!(e.state.workspaces[&100001].tiles.iter().any(|t| t.address == "0xa2"));
    assert!(!e.state.workspaces.contains_key(&100002));
}

#[test]
fn old_tiles_without_new_fields_load() {
    let old = r#"{"version":1,"signature":"s","monitors":[],"sessions":{},
      "workspaces":{"100001":{"real_id":100001,"session":1,"number":1,"name":null,"previous":null,
      "tiles":[{"address":"0x1","pid":1,"class":"foot","title":"t","floating":false,"at":[0,0],"size":[1,1]}]}}}"#;
    assert!(serde_json::from_str::<State>(old).is_ok());
}

// ---- Floating round trip: parking (spec 5.7, redesign 2026-11-15) -------------

use ymux::engine::Request;
use ymux::events::Event;
use ymux::hypr::{focus_window, layout_msg, move_exact, move_to, move_window, resize_exact, swap_windows};
use ymux::model::SavedTile;

const FLOAT: &str = "o.workspace_mode({ workspace = \"100001\", mode = \"floating\" })\n";

/// 100001 also holds placeholder 0xp at (960,0) 960x1080, foot 0xa1 at (0,0) 960x1080.
fn with_placeholder(d: PathBuf) -> Engine<FakeHypr> {
    let mut e = engine(d);
    let w = e.state.workspaces.get_mut(&100001).unwrap();
    w.tiles[0].size = (960, 1080);
    w.tiles.push(Tile {
        address: "0xp".into(),
        pid: Some(77),
        class: "ymux-placeholder".into(),
        at: (960, 0),
        size: (960, 1080),
        ..Default::default()
    });
    e.hypr().clients.borrow_mut().push(client("0xp", 77, "ymux-placeholder", 100001));
    e
}

/// Lets the 300 ms after a return pass, so the deferred rearrange runs.
fn settle(e: &mut Engine<FakeHypr>) {
    e.on_deadline(std::time::Instant::now() + std::time::Duration::from_secs(1)).unwrap();
}

fn saved(addr: &str, at: (i32, i32), size: (i32, i32)) -> SavedTile {
    SavedTile { address: addr.into(), at, size }
}

#[test]
fn floating_switch_parks_placeholders_and_saves_layout() {
    let d = dir("park");
    let mut e = with_placeholder(d.clone());
    set_live(&e, "0xa1", (400, 300), (1100, 716), true);
    set_live(&e, "0xp", (400, 300), (1100, 716), true);
    let _ = d;
    tag_all(&e, 100001); // Omarchy tags the windows as it floats them
    e.on_event(Event::FloatingChanged { address: "0xp".into(), floating: true }).unwrap();
    let d = e.hypr().dispatched();
    // parked in the top-left corner of DP-1 (at 0,0), 1 x 1, never closed
    assert!(d.contains(&resize_exact("0xp", 1, 1)));
    assert!(d.contains(&move_exact("0xp", 0, 0)));
    assert!(!d.iter().any(|x| x.contains("window.close")));
    let w = &e.state.workspaces[&100001];
    assert!(w.tiles.iter().any(|t| t.address == "0xp"), "a parked placeholder is still a tile");
    assert_eq!(w.archived_layout, vec![saved("0xa1", (0, 0), (960, 1080)), saved("0xp", (960, 0), (960, 1080))]);
    assert_eq!(w.mode.as_deref(), Some("floating"));
    // the next placeholder's event parks nothing twice
    e.hypr().clear();
    e.on_event(Event::FloatingChanged { address: "0xp".into(), floating: true }).unwrap();
    assert!(e.hypr().dispatched().is_empty());
}

#[test]
fn thirty_second_check_parks_when_event_missed() {
    let d = dir("parktick");
    let mut e = with_placeholder(d.clone());
    fs::write(d.join("100001.lua"), FLOAT).unwrap();
    set_live(&e, "0xp", (400, 300), (1100, 716), true);
    e.tick().unwrap();
    assert!(e.hypr().dispatched().contains(&resize_exact("0xp", 1, 1)));
    assert_eq!(e.state.workspaces[&100001].archived_layout.len(), 2);
}

/// Parked, then back: 0xa1 and 0xp were side by side (saved), mode dwindle.
fn returned(name: &str) -> Engine<FakeHypr> {
    let mut e = with_placeholder(dir(name));
    let w = e.state.workspaces.get_mut(&100001).unwrap();
    w.mode = Some("floating".into());
    w.archived_layout = vec![saved("0xa1", (0, 0), (960, 1080)), saved("0xp", (960, 0), (960, 1080))];
    e
}

fn back(e: &mut Engine<FakeHypr>, addr: &str) {
    e.on_event(Event::FloatingChanged { address: addr.into(), floating: false }).unwrap();
    settle(e);
}

fn stashed(e: &Engine<FakeHypr>, addr: &str) -> bool {
    e.hypr().dispatched().contains(&move_to(addr, "special:ymux-stash"))
}

#[test]
fn kill_drops_saved_layout() {
    let mut e = returned("killlayout");
    e.handle(Request::KillWorkspace { id: 100002 });
    e.handle(Request::KillWorkspace { id: 100001 });
    assert!(e.state.workspaces.get(&100001).is_none_or(|w| w.archived_layout.is_empty()));
}

fn restarted(d: PathBuf) -> Engine<FakeHypr> {
    // saved state has the placeholder; it is gone from Hyprland
    let mut e = with_placeholder(d);
    e.state.signature = Some("sig".into());
    e.hypr().clients.borrow_mut().retain(|c| c.address != "0xp");
    e
}

#[test]
fn restart_recreates_missing_placeholder_on_tiled_workspace() {
    let mut e = restarted(dir("rstile"));
    e.start().unwrap();
    assert_eq!(e.hypr().spawned.borrow().len(), 1);
}

#[test]
fn restarted_placeholder_is_not_resized() {
    let mut e = restarted(dir("rsnoresize"));
    e.start().unwrap();
    e.hypr().clear();
    let mut c = client("0xn", 9000, "ymux-placeholder", -99);
    c.workspace_name = "special:ymux-stash".into();
    e.hypr().clients.borrow_mut().push(c);
    e.on_event(Event::OpenWindow {
        address: "0xn".into(),
        workspace_name: "special:ymux-stash".into(),
        class: "ymux-placeholder".into(),
        title: "ymux placeholder".into(),
    })
    .unwrap();
    assert!(!e.hypr().dispatched().iter().any(|x| x.contains("window.resize")));
}

#[test]
fn restart_drops_missing_placeholder_on_floating_workspace() {
    let d = dir("rsfloat");
    fs::write(d.join("100001.lua"), FLOAT).unwrap();
    let mut e = restarted(d);
    e.start().unwrap();
    assert!(e.hypr().spawned.borrow().is_empty());
}

// ---- Detection by Omarchy's tag, and the size step (option 1, 2026-11-15) -------

fn tag_all(e: &Engine<FakeHypr>, ws: i64) {
    for c in e.hypr().clients.borrow_mut().iter_mut().filter(|c| c.workspace == ws) {
        c.tags.push("omarchy-floating-workspace".into());
    }
}

#[test]
fn floating_tag_parks_before_the_mode_file_exists() {
    let mut e = with_placeholder(dir("tagpark"));
    tag_all(&e, 100001);
    set_live(&e, "0xp", (400, 300), (1100, 716), true);
    e.on_event(Event::FloatingChanged { address: "0xp".into(), floating: true }).unwrap();
    assert!(e.hypr().dispatched().contains(&resize_exact("0xp", 1, 1)));
}

#[test]
fn return_is_seen_by_the_tag_going_even_with_the_mode_file_still_there() {
    let mut e = returned("tagreturn");
    fs::write(e.layouts_dir().join("100001.lua"), FLOAT).unwrap(); // not removed yet
    set_live(&e, "0xa1", (0, 0), (1920, 540), false);
    set_live(&e, "0xp", (0, 540), (1920, 540), false);
    back(&mut e, "0xa1");
    assert!(stashed(&e, "0xa1"), "restored");
    assert_eq!(e.state.workspaces[&100001].mode.as_deref(), Some("dwindle"));
}

#[test]
fn restore_waits_for_the_layout_to_settle() {
    let mut e = returned("defer");
    set_live(&e, "0xa1", (0, 0), (1920, 540), false);
    set_live(&e, "0xp", (0, 540), (1920, 540), false);
    e.on_event(Event::FloatingChanged { address: "0xa1".into(), floating: false }).unwrap();
    assert!(!stashed(&e, "0xa1"), "not at the event");
    e.on_event(Event::FloatingChanged { address: "0xp".into(), floating: false }).unwrap();
    settle(&mut e);
    assert_eq!(e.hypr().dispatched().iter().filter(|x| **x == move_to("0xa1", "special:ymux-stash")).count(), 1);
}

#[test]
fn right_arrangement_only_gets_its_sizes_back() {
    let mut e = returned("sizes");
    let w = e.state.workspaces.get_mut(&100001).unwrap();
    w.archived_layout = vec![saved("0xa1", (0, 0), (1200, 1080)), saved("0xp", (1200, 0), (720, 1080))];
    set_live(&e, "0xa1", (0, 0), (960, 1080), false);
    set_live(&e, "0xp", (960, 0), (960, 1080), false);
    back(&mut e, "0xa1");
    let d = e.hypr().dispatched();
    assert!(!stashed(&e, "0xa1"), "no rebuild needed");
    let a = d.iter().position(|x| *x == resize_exact("0xa1", 1200, 1080)).expect("0xa1 resized");
    let p = d.iter().position(|x| *x == resize_exact("0xp", 720, 1080)).expect("0xp resized");
    assert!(a < p);
}

#[test]
fn right_arrangement_and_sizes_is_left_alone() {
    let mut e = returned("inplace");
    set_live(&e, "0xa1", (0, 0), (960, 1080), false);
    set_live(&e, "0xp", (960, 0), (960, 1080), false);
    back(&mut e, "0xp");
    assert!(e.hypr().dispatched().iter().all(|x| !x.contains("window.move") && !x.contains("window.resize") && !x.contains("swap")));
    assert!(e.state.workspaces[&100001].archived_layout.is_empty());
}

#[test]
fn thirty_second_check_restores_when_back() {
    let mut e = returned("backtick");
    set_live(&e, "0xa1", (0, 0), (1920, 540), false);
    set_live(&e, "0xp", (0, 540), (1920, 540), false);
    e.tick().unwrap();
    settle(&mut e);
    assert!(stashed(&e, "0xa1"));
}

/// Saved: 0xa1 full left, 0xb right-top, 0xp right-bottom (a vertical cut, then a
/// horizontal one in the right half). Omarchy re-tiled something else.
fn three(name: &str) -> Engine<FakeHypr> {
    let mut e = returned(name);
    e.hypr().clients.borrow_mut().push(client("0xb", 2, "foot", 100001));
    let w = e.state.workspaces.get_mut(&100001).unwrap();
    w.tiles.push(Tile { address: "0xb".into(), pid: Some(2), class: "foot".into(), ..Default::default() });
    w.archived_layout = vec![
        saved("0xa1", (0, 0), (960, 1080)),
        saved("0xb", (960, 0), (960, 540)),
        saved("0xp", (960, 540), (960, 540)),
    ];
    set_live(&e, "0xp", (0, 0), (960, 1080), false);
    set_live(&e, "0xa1", (960, 0), (960, 540), false);
    set_live(&e, "0xb", (960, 540), (960, 540), false);
    e
}

#[test]
fn dwindle_layout_is_rebuilt_window_by_window() {
    let mut e = three("rebuild");
    *e.hypr().active.borrow_mut() = Some("0xb".into());
    e.on_event(Event::ActiveWindow { address: Some("0xb".into()) }).unwrap();
    e.hypr().clear();
    back(&mut e, "0xb");
    let d = e.hypr().dispatched();
    let tail: Vec<String> = d.iter().skip_while(|x| !x.contains("special:ymux-stash")).cloned().collect();
    assert_eq!(
        tail,
        vec![
            move_to("0xa1", "special:ymux-stash"),
            move_to("0xb", "special:ymux-stash"),
            move_to("0xp", "special:ymux-stash"),
            // the whole workspace is wider than tall: dwindle splits side by side, as saved
            move_window("0xa1", 100001, false),
            focus_window("0xa1"),
            move_window("0xb", 100001, false),
            // the right half is taller than wide: dwindle stacks, as saved
            focus_window("0xb"),
            move_window("0xp", 100001, false),
            resize_exact("0xa1", 960, 1080),
            resize_exact("0xb", 960, 540),
            resize_exact("0xp", 960, 540),
            focus_window("0xb"),
        ]
    );
}

#[test]
fn split_that_dwindle_would_turn_is_toggled() {
    // saved: 0xa1 full-width top, 0xb bottom-left, 0xp bottom-right
    let mut e = three("toggle");
    let w = e.state.workspaces.get_mut(&100001).unwrap();
    w.archived_layout = vec![
        saved("0xa1", (0, 0), (1920, 540)),
        saved("0xb", (0, 540), (960, 540)),
        saved("0xp", (960, 540), (960, 540)),
    ];
    back(&mut e, "0xb");
    let d = e.hypr().dispatched();
    let at = d.iter().position(|x| *x == move_window("0xb", 100001, false)).expect("0xb in");
    // the workspace is wider than tall, so dwindle would put 0xb beside 0xa1: flip it
    assert_eq!(d[at + 1..at + 3], [focus_window("0xb"), layout_msg("togglesplit")]);
    // the bottom half is wider than tall: side by side, as saved, no flip
    let p = d.iter().position(|x| *x == move_window("0xp", 100001, false)).expect("0xp in");
    assert_ne!(d.get(p + 2), Some(&layout_msg("togglesplit")));
}

#[test]
fn window_opened_while_floating_comes_back_last_and_sizes_are_left() {
    let mut e = three("extra");
    e.hypr().clients.borrow_mut().push(client("0xnew", 9, "foot", 100001));
    back(&mut e, "0xb");
    let d = e.hypr().dispatched();
    let last_in = d.iter().rposition(|x| x.starts_with("hl.dsp.window.move({ workspace = \"100001\"")).unwrap();
    assert_eq!(d[last_in], move_window("0xnew", 100001, false));
    assert!(!d.iter().any(|x| x.contains("window.resize")));
}

#[test]
fn saved_window_closed_while_floating_is_left_out() {
    let mut e = three("gone");
    e.hypr().clients.borrow_mut().retain(|c| c.address != "0xb");
    back(&mut e, "0xa1");
    assert!(!e.hypr().dispatched().iter().any(|x| x.contains("address:0xb")));
}

#[test]
fn scrolling_workspace_uses_swaps() {
    let mut e = returned("scroll");
    e.hypr().workspaces.borrow_mut()[0].tiled_layout = "scrolling".into();
    set_live(&e, "0xp", (0, 0), (960, 1080), false);
    set_live(&e, "0xa1", (960, 0), (960, 1080), false);
    back(&mut e, "0xa1");
    assert!(e.hypr().dispatched().contains(&swap_windows("0xa1", "0xp")));
    assert!(!stashed(&e, "0xa1"));
}

#[test]
fn workspace_without_placeholders_saves_its_layout_too() {
    let mut e = engine(dir("noph"));
    e.state.workspaces.get_mut(&100001).unwrap().tiles[0].size = (960, 1080);
    tag_all(&e, 100001);
    set_live(&e, "0xa1", (400, 300), (1100, 716), true);
    e.on_event(Event::FloatingChanged { address: "0xa1".into(), floating: true }).unwrap();
    assert_eq!(e.state.workspaces[&100001].archived_layout, vec![saved("0xa1", (0, 0), (960, 1080))]);
}

// ---- review fixes 3, 4, 6 ------------------------------------------------------

#[test]
fn a_failed_rebuild_brings_every_window_back_from_the_stash() {
    let mut e = three("rebuildfail");
    e.hypr().fail_dispatch.borrow_mut().push("hl.dsp.focus({ window = \"address:0xa1\" })".into());
    back(&mut e, "0xb");
    let d = e.hypr().dispatched();
    for a in ["0xa1", "0xb", "0xp"] {
        let last_stash = d.iter().rposition(|x| *x == move_to(a, "special:ymux-stash")).unwrap();
        assert!(d[last_stash..].contains(&move_window(a, 100001, false)), "{a} back on its workspace");
    }
}

#[test]
fn start_rescues_app_windows_left_on_the_stash() {
    let mut e = with_placeholder(dir("rescue"));
    e.state.signature = Some("sig".into());
    {
        let mut c = e.hypr().clients.borrow_mut();
        let a = c.iter_mut().find(|c| c.address == "0xa1").unwrap();
        a.workspace = -99;
        a.workspace_name = "special:ymux-stash".into();
    }
    e.start().unwrap();
    assert!(e.hypr().dispatched().contains(&move_window("0xa1", 100001, false)), "back to its recorded workspace");
}

#[test]
fn geometry_capture_reads_the_workspace_list_once() {
    let mut e = engine(dir("calls"));
    e.hypr().workspaces_calls.set(0);
    e.capture_geometry().unwrap();
    assert_eq!(e.hypr().workspaces_calls.get(), 1);
}

#[test]
fn thirty_second_check_sees_floating_by_the_tag_before_the_file() {
    let mut e = with_placeholder(dir("tagtick"));
    tag_all(&e, 100001);
    set_live(&e, "0xp", (400, 300), (1100, 716), true);
    e.tick().unwrap();
    assert!(e.hypr().dispatched().contains(&resize_exact("0xp", 1, 1)), "parked");
    // 0xp is floated live at 1100x716; the saved size is its recorded tiled one
    assert_eq!(e.state.workspaces[&100001].archived_layout[1].size, (960, 1080), "saved from the recorded tiled geometry");
}
