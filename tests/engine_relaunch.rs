//! Relaunch on Hyprland start (spec 7).
mod common;
use ymux::config::Config;
use ymux::engine::Engine;
use ymux::events::Event;
use ymux::hypr::ProcInfo;
use ymux::model::State;
use common::*;
use std::fs;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ymux-relaunch-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn new_tiles_record_cmdline_and_cwd() {
    let mut s = State::default();
    s.ensure_monitor("LG A", "DP-1");
    let a = s.create_session("LG A", None);
    s.monitor_mut("LG A").unwrap().active_session = Some(a);
    let h = FakeHypr::default();
    *h.monitors.borrow_mut() = vec![monitor("DP-1", "LG A", 0, true, 100001)];
    *h.workspaces.borrow_mut() = vec![workspace(100001, "DP-1", 1)];
    *h.clients.borrow_mut() = vec![client("0xc1", 31, "kitty", 100001)];
    h.procs.borrow_mut().insert(31, ProcInfo { cmdline: vec!["kitty".into()], cwd: Some("/home/u".into()) });
    let mut e = Engine::new(s, Config::default(), h, "sig".into(), dir("record"));
    e.on_event(Event::OpenWindow { address: "0xc1".into(), workspace_name: "100001".into(), class: "kitty".into(), title: "k".into() }).unwrap();
    let t = &e.state.workspaces[&100001].tiles[0];
    assert_eq!(t.cmdline, vec!["kitty".to_string()]);
    assert_eq!(t.cwd.as_deref(), Some("/home/u"));
}

// ---- The restore (Tasks 23-24; tiles wait on the stash, the layout step builds) --

use ymux::engine::StartKind;
use ymux::hypr::{focus_monitor, move_exact, move_to, move_window, resize_exact};
use ymux::model::Tile;
use std::time::{Duration, Instant};

const STASH: &str = "special:ymux-stash";

fn t(class: &str, cmd: &[&str], at: (i32, i32), size: (i32, i32)) -> Tile {
    Tile {
        address: format!("0xold-{class}"),
        pid: Some(1),
        class: class.into(),
        cmdline: cmd.iter().map(|s| s.to_string()).collect(),
        cwd: Some("/home/u/Projects".into()),
        at,
        size,
        ..Default::default()
    }
}

/// Saved before a reboot: 100001 dwindle with foot (left) and a placeholder (right);
/// 100002 with kitty. The new Hyprland has no windows yet.
fn rebooted(name: &str, relaunch: bool) -> Engine<FakeHypr> {
    let mut s = State::default();
    s.ensure_monitor("LG A", "DP-1");
    let a = s.create_session("LG A", None);
    s.create_workspace(a);
    s.monitor_mut("LG A").unwrap().active_session = Some(a);
    s.signature = Some("old".into());
    let w = s.workspaces.get_mut(&100001).unwrap();
    w.mode = Some("dwindle".into());
    w.tiles = vec![
        t("foot", &["foot"], (0, 0), (960, 1080)),
        Tile { address: "0xold-ph".into(), class: "ymux-placeholder".into(), at: (960, 0), size: (960, 1080), ..Default::default() },
    ];
    let w2 = s.workspaces.get_mut(&100002).unwrap();
    w2.mode = Some("dwindle".into());
    w2.tiles = vec![t("kitty", &["kitty"], (0, 0), (1920, 1080))];
    let h = FakeHypr::default();
    *h.monitors.borrow_mut() = vec![monitor("DP-1", "LG A", 0, true, 1)];
    *h.workspaces.borrow_mut() = vec![workspace(1, "DP-1", 0), workspace(100001, "DP-1", 0), workspace(100002, "DP-1", 0)];
    let config = Config { relaunch, ..Default::default() };
    let mut e = Engine::new(s, config, h, "new".into(), dir(name));
    e.exe = "cm".into();
    e
}

/// A window appears (where Hyprland puts a new window, or the stash for a placeholder).
fn appear(e: &mut Engine<FakeHypr>, addr: &str, pid: u32, class: &str) {
    let mut c = client(addr, pid, class, 1);
    if class == "ymux-placeholder" {
        c.workspace = -99;
        c.workspace_name = STASH.into();
    }
    let ws_name = c.workspace_name.clone();
    e.hypr().clients.borrow_mut().push(c);
    e.on_event(Event::OpenWindow { address: addr.into(), workspace_name: ws_name, class: class.into(), title: class.into() }).unwrap();
}

fn spawned(e: &Engine<FakeHypr>) -> Vec<Vec<String>> {
    e.hypr().spawned.borrow().clone()
}

/// Lets every launched window appear, in order, until the restore is done.
fn run_restore(e: &mut Engine<FakeHypr>) {
    let mut n = 0;
    while e.restoring() && n < 10 {
        let argv = spawned(e)[n].clone();
        let pid = 9000 + n as u32;
        let class = if argv.get(1).map(String::as_str) == Some("placeholder") { "ymux-placeholder".to_string() } else { argv[0].clone() };
        appear(e, &format!("0xnew{n}"), pid, &class);
        n += 1;
    }
}

#[test]
fn hyprland_start_lands_then_launches_the_first_tile() {
    let mut e = rebooted("lands", true);
    assert_eq!(e.start().unwrap(), StartKind::HyprlandStarted);
    assert!(e.hypr().dispatched().contains(&focus_monitor("DP-1")), "landed");
    assert_eq!(spawned(&e), vec![vec!["foot".to_string()]]);
    assert!(e.restoring());
}

#[test]
fn launches_one_tile_at_a_time_and_parks_each_on_the_stash() {
    let mut e = rebooted("order", true);
    e.start().unwrap();
    appear(&mut e, "0xf", 9000, "foot");
    assert!(e.hypr().dispatched().contains(&move_to("0xf", STASH)));
    assert_eq!(spawned(&e)[1], vec!["cm".to_string(), "placeholder".into()]);
    appear(&mut e, "0xp", 9001, "ymux-placeholder");
    // 100001 complete: laid out; then 100002's kitty is launched
    assert!(e.hypr().dispatched().contains(&move_window("0xf", 100001, false)));
    assert!(e.hypr().dispatched().contains(&move_window("0xp", 100001, false)));
    assert_eq!(spawned(&e)[2], vec!["kitty".to_string()]);
}

#[test]
fn dwindle_layout_rebuilds_and_resizes_to_saved_sizes() {
    let mut e = rebooted("dwindle", true);
    e.start().unwrap();
    appear(&mut e, "0xf", 9000, "foot");
    appear(&mut e, "0xp", 9001, "ymux-placeholder");
    let d = e.hypr().dispatched();
    let first_in = d.iter().position(|x| *x == move_window("0xf", 100001, false)).unwrap();
    let second_in = d.iter().position(|x| *x == move_window("0xp", 100001, false)).unwrap();
    assert!(first_in < second_in);
    assert!(d.contains(&resize_exact("0xf", 960, 1080)));
    assert!(d.contains(&resize_exact("0xp", 960, 1080)));
}

#[test]
fn floating_layout_moves_and_resizes_each_window() {
    let mut e = rebooted("floating", true);
    let w = e.state.workspaces.get_mut(&100002).unwrap();
    w.mode = Some("floating".into());
    w.tiles[0].at = (300, 200);
    w.tiles[0].size = (800, 600);
    e.start().unwrap();
    run_restore(&mut e);
    let d = e.hypr().dispatched();
    let m = d.iter().position(|x| *x == move_exact("0xnew2", 300, 200)).expect("moved");
    let r = d.iter().position(|x| *x == resize_exact("0xnew2", 800, 600)).expect("resized");
    assert!(m < r);
}

#[test]
fn scrolling_layout_only_keeps_the_order() {
    let mut e = rebooted("scrolling", true);
    e.state.workspaces.get_mut(&100001).unwrap().mode = Some("scrolling".into());
    e.start().unwrap();
    run_restore(&mut e);
    let d = e.hypr().dispatched();
    let a = d.iter().position(|x| *x == move_window("0xnew0", 100001, false)).unwrap();
    let b = d.iter().position(|x| *x == move_window("0xnew1", 100001, false)).unwrap();
    assert!(a < b);
    assert!(!d.iter().any(|x| x.contains("window.resize") && (x.contains("0xnew0") || x.contains("0xnew1"))));
}

#[test]
fn claims_an_autostarted_window_instead_of_launching() {
    let mut e = rebooted("claim", true);
    e.hypr().clients.borrow_mut().push(client("0xk", 50, "kitty", 1));
    e.hypr().procs.borrow_mut().insert(50, ProcInfo { cmdline: vec!["kitty".into()], cwd: None });
    e.start().unwrap();
    run_restore(&mut e);
    assert!(!spawned(&e).iter().any(|a| a == &vec!["kitty".to_string()]));
    assert!(e.hypr().dispatched().contains(&move_window("0xk", 100002, false)));
}

#[test]
fn a_new_window_of_the_same_class_is_accepted_another_class_is_not() {
    let mut e = rebooted("sameclass", true);
    e.start().unwrap();
    appear(&mut e, "0xother", 7, "firefox"); // the user's own window: not the foot
    assert_eq!(spawned(&e).len(), 1, "still waiting for foot");
    appear(&mut e, "0xf2", 8, "foot"); // a foot from another pid (e.g. a server): accepted
    assert_eq!(spawned(&e).len(), 2);
}

#[test]
fn gives_up_after_15_s_and_moves_on() {
    let mut e = rebooted("timeout", true);
    e.start().unwrap();
    e.on_deadline(Instant::now() + Duration::from_secs(16)).unwrap();
    assert!(e.hypr().notified.borrow().iter().any(|n| n.contains("foot") && n.contains("launch-override")));
    assert_eq!(spawned(&e).len(), 2, "went on to the placeholder");
}

#[test]
fn windows_opened_during_the_restore_are_recorded_once_afterwards() {
    let mut e = rebooted("userwin", true);
    e.start().unwrap();
    let mut u = client("0xu", 77, "firefox", 100001);
    u.workspace_name = "100001".into();
    e.hypr().clients.borrow_mut().push(u);
    e.on_event(Event::OpenWindow { address: "0xu".into(), workspace_name: "100001".into(), class: "firefox".into(), title: "f".into() }).unwrap();
    assert!(!e.state.workspaces[&100001].tiles.iter().any(|t| t.address == "0xu"), "not during");
    run_restore(&mut e);
    let n = e.state.workspaces.values().flat_map(|w| &w.tiles).filter(|t| t.address == "0xu").count();
    assert_eq!(n, 1);
}

#[test]
fn launched_app_gets_its_saved_working_directory() {
    // the fake records argv only; the cwd goes through Hypr::spawn's second argument
    let mut e = rebooted("cwd", true);
    e.start().unwrap();
    assert_eq!(e.hypr().spawned_cwds.borrow().first().cloned().flatten().as_deref(), Some("/home/u/Projects"));
}

#[test]
fn relaunch_off_restores_nothing_and_keeps_empty_workspaces() {
    let mut e = rebooted("off", false);
    e.start().unwrap();
    assert!(spawned(&e).is_empty());
    assert!(!e.restoring());
    assert!(e.state.workspaces.contains_key(&100001) && e.state.workspaces.contains_key(&100002));
    assert!(e.state.workspaces[&100001].tiles.is_empty());
}

#[test]
fn daemon_restart_never_relaunches() {
    let mut e = rebooted("restart", true);
    e.state.signature = Some("new".into());
    e.start().unwrap();
    assert!(!spawned(&e).iter().any(|a| a == &vec!["foot".to_string()]));
}

#[test]
fn set_mode_step_writes_or_deletes_the_mode_file() {
    let mut e = rebooted("modefile", true);
    e.state.workspaces.get_mut(&100001).unwrap().mode = Some("scrolling".into());
    fs::write(e.layouts_dir().join("100002.lua"), "o.workspace_mode({ workspace = \"100002\", mode = \"floating\" })\n").unwrap();
    e.start().unwrap();
    run_restore(&mut e);
    assert_eq!(
        fs::read_to_string(e.layouts_dir().join("100001.lua")).unwrap(),
        "o.workspace_mode({ workspace = \"100001\", mode = \"scrolling\" })\n"
    );
    assert!(!e.layouts_dir().join("100002.lua").exists(), "dwindle is the default: no file");
}

// ---- review fix 1: a failing step never wedges the restore -------------------

#[test]
fn a_failing_step_is_skipped_and_the_restore_finishes() {
    let mut e = rebooted("wedge", true);
    e.start().unwrap();
    appear(&mut e, "0xf", 9000, "foot");
    // the foot died: every dispatch naming it fails from now on
    e.hypr().fail_dispatch.borrow_mut().push("0xf".into());
    appear(&mut e, "0xp", 9001, "ymux-placeholder");
    // 100001's layout failed on 0xf, yet the restore went on to kitty
    assert_eq!(spawned(&e)[2], vec!["kitty".to_string()]);
    appear(&mut e, "0xk", 9002, "kitty");
    assert!(!e.restoring(), "finished");
    // and windows opened afterwards are recorded again
    let mut u = client("0xu", 77, "firefox", 100002);
    u.workspace_name = "100002".into();
    e.hypr().clients.borrow_mut().push(u);
    e.on_event(Event::OpenWindow { address: "0xu".into(), workspace_name: "100002".into(), class: "firefox".into(), title: "f".into() }).unwrap();
    assert!(e.state.workspaces[&100002].tiles.iter().any(|t| t.address == "0xu"));
}

#[test]
fn the_30_s_check_records_nothing_during_a_restore() {
    let mut e = rebooted("tickrestore", true);
    e.start().unwrap();
    let mut u = client("0xu", 77, "firefox", 100001);
    u.workspace_name = "100001".into();
    e.hypr().clients.borrow_mut().push(u);
    e.tick().unwrap();
    assert!(!e.state.workspaces[&100001].tiles.iter().any(|t| t.address == "0xu"));
}
