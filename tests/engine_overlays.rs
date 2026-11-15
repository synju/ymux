mod common;
use ymux::config::Config;
use ymux::engine::{Engine, Request};
use ymux::hypr::Hypr;
use ymux::model::State;
use ymux::overlay::summon_argv;
use common::*;
use serde_json::json;

/// One monitor "LG A" (DP-1) showing 100001; session 1 "main" owns 100001 and 100002.
fn engine() -> Engine<FakeHypr> {
    let mut s = State::default();
    s.ensure_monitor("LG A", "DP-1");
    let a = s.create_session("LG A", Some("main"));
    s.create_workspace(a);
    s.monitor_mut("LG A").unwrap().active_session = Some(a);
    s.monitor_mut("LG A").unwrap().shown = Some(100001);
    let h = FakeHypr::default();
    *h.monitors.borrow_mut() = vec![monitor("DP-1", "LG A", 0, true, 100001)];
    *h.workspaces.borrow_mut() = vec![workspace(100001, "DP-1", 0)];
    Engine::new(s, Config::default(), h, "sig".into(), unique_dir("test-ov"))
}

#[test]
fn fake_spawn_records_argv_and_returns_pids() {
    let h = FakeHypr::default();
    assert_eq!(h.spawn(&["a".into()], None).unwrap(), 9000);
    assert_eq!(h.spawn(&["b".into()], None).unwrap(), 9001);
    assert_eq!(h.spawned.borrow()[0], vec!["a".to_string()]);
}

#[test]
fn rename_session_action_summons_prompt_with_current_name() {
    let mut e = engine();
    assert!(e.handle(Request::Action { name: "rename-session".into() }).ok);
    let p = json!({"view":"rename","target":"session","id":1,"current":"main"});
    assert_eq!(e.hypr().spawned.borrow().clone(), vec![summon_argv(&p)]);
}

#[test]
fn rename_workspace_action_targets_shown_workspace() {
    let mut e = engine();
    e.state.workspaces.get_mut(&100001).unwrap().name = Some("notes".into());
    e.handle(Request::Action { name: "rename-workspace".into() });
    let p = json!({"view":"rename","target":"workspace","id":100001,"current":"notes"});
    assert_eq!(e.hypr().spawned.borrow().clone(), vec![summon_argv(&p)]);
}

#[test]
fn rename_workspace_sets_and_clears_name() {
    let mut e = engine();
    assert!(e.handle(Request::RenameWorkspace { id: 100002, name: "  Net Extraction ".into() }).ok);
    assert_eq!(e.state.workspaces[&100002].name.as_deref(), Some("Net Extraction"));
    assert!(e.take_dirty());
    assert!(e.handle(Request::RenameWorkspace { id: 100002, name: "".into() }).ok);
    assert_eq!(e.state.workspaces[&100002].name, None);
}

#[test]
fn rename_session_rejects_empty_name() {
    let mut e = engine();
    let r = e.handle(Request::RenameSession { number: 1, name: "  ".into() });
    assert!(!r.ok);
    assert_eq!(r.message.unwrap(), "session name cannot be empty");
    assert_eq!(e.state.sessions[&1].name, "main");
    assert!(e.handle(Request::RenameSession { number: 1, name: "Work".into() }).ok);
    assert_eq!(e.state.sessions[&1].name, "Work");
}

#[test]
fn rename_unknown_targets_fail() {
    let mut e = engine();
    assert!(!e.handle(Request::RenameWorkspace { id: 7, name: "x".into() }).ok);
    assert!(!e.handle(Request::RenameSession { number: 9, name: "x".into() }).ok);
}

#[test]
fn move_workspace_action_summons_number_prompt() {
    let mut e = engine();
    e.handle(Request::Action { name: "move-workspace".into() });
    let p = json!({"view":"renumber","id":100001,"current":"1"});
    assert_eq!(e.hypr().spawned.borrow().clone(), vec![summon_argv(&p)]);
}

#[test]
fn renumber_workspace_moves_it_and_keeps_order() {
    let mut e = engine();
    assert!(e.handle(Request::RenumberWorkspace { id: 100001, number: 5 }).ok);
    assert_eq!(e.state.workspaces[&100001].number, 5);
    let order: Vec<u32> = e.state.session_workspaces(1).iter().map(|w| w.number).collect();
    assert_eq!(order, [2, 5]);
    assert!(e.take_dirty());
}

#[test]
fn renumber_to_taken_or_zero_is_refused() {
    let mut e = engine();
    let r = e.handle(Request::RenumberWorkspace { id: 100001, number: 2 });
    assert!(!r.ok);
    assert_eq!(r.message.unwrap(), "workspace 2 already exists");
    let r = e.handle(Request::RenumberWorkspace { id: 100001, number: 0 });
    assert!(!r.ok);
    assert_eq!(r.message.unwrap(), "the number must be 1 or higher");
    assert_eq!(e.state.workspaces[&100001].number, 1);
    assert!(e.handle(Request::RenumberWorkspace { id: 100001, number: 1 }).ok);
}

use ymux::events::Event;
use ymux::hypr::{close_window, focus_monitor, focus_window, focus_workspace};
use ymux::model::Tile;
use std::time::{Duration, Instant};

fn tile(addr: &str, class: &str) -> Tile {
    Tile { address: addr.into(), pid: Some(1), class: class.into(), title: "t".into(), floating: false, at: (0, 0), size: (1, 1), ..Default::default() }
}

#[test]
fn tree_command_options_actions_summon_their_views() {
    let mut e = engine();
    for (action, view) in [("tree", "tree"), ("command-prompt", "command"), ("options", "options")] {
        e.handle(Request::Action { name: action.into() });
        assert_eq!(e.hypr().spawned.borrow().last().unwrap(), &summon_argv(&json!({ "view": view })));
    }
}

#[test]
fn display_panes_uses_configured_timeout() {
    let mut e = engine();
    e.config.display_panes_ms = 1500;
    e.hypr().clients.borrow_mut().push(client("0xa1", 1, "foot", 100001));
    e.handle(Request::Action { name: "display-panes".into() });
    let argv = e.hypr().spawned.borrow().last().unwrap().clone();
    let payload: serde_json::Value = serde_json::from_str(&argv[4]).unwrap();
    assert_eq!(payload["view"], "panes");
    assert_eq!(payload["timeout"], 1500);
    assert_eq!(payload["panes"][0]["key"], "1");
    assert_eq!(payload["panes"][0]["address"], "0xa1");
}

#[test]
fn command_new_with_name_creates_session() {
    let mut e = engine();
    assert!(e.handle(Request::Command { line: "new Work".into() }).ok);
    let s = e.state.sessions.values().find(|s| s.name == "Work").unwrap().number;
    assert_eq!(e.state.monitor("LG A").unwrap().active_session, Some(s));
    assert!(e.handle(Request::Command { line: "new".into() }).ok);
    assert!(e.handle(Request::Command { line: "   ".into() }).ok);
}

#[test]
fn command_unknown_is_reported() {
    let r = engine().handle(Request::Command { line: "frob".into() });
    assert!(!r.ok);
    assert_eq!(r.message.unwrap(), "unknown command: frob");
}

#[test]
fn set_relaunch_rewrites_only_that_line() {
    let d = std::env::temp_dir().join(format!("ymux-relaunch-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("ymux.conf");
    std::fs::write(&f, "# mine\nset relaunch on\n").unwrap();
    let mut e = engine();
    e.config_path = f.clone();
    assert!(e.handle(Request::SetRelaunch { on: false }).ok);
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "# mine\nset relaunch off\n");
    assert!(!e.config.relaunch);
    assert!(e.take_view_changed());
}

#[test]
fn focus_window_on_other_monitor_focuses_monitor_first() {
    let mut e = engine();
    e.hypr().monitors.borrow_mut().push(monitor("DP-2", "Dell B", 2560, false, 200001));
    e.hypr().clients.borrow_mut().push(client("0xb1", 2, "foot", 200001));
    e.hypr().workspaces.borrow_mut().push(workspace(200001, "DP-2", 1));
    e.handle(Request::FocusWindow { address: "0xb1".into() });
    assert_eq!(e.hypr().dispatched(), vec![focus_monitor("DP-2"), focus_window("0xb1")]);
}

#[test]
fn focus_monitor_request() {
    let mut e = engine();
    e.handle(Request::FocusMonitor { connector: "DP-1".into() });
    assert_eq!(e.hypr().dispatched(), vec![focus_monitor("DP-1")]);
}

#[test]
fn kill_workspace_closes_every_window() {
    let mut e = engine();
    e.state.workspaces.get_mut(&100002).unwrap().tiles.push(tile("0xa2", "foot"));
    e.handle(Request::KillWorkspace { id: 100002 });
    assert!(e.hypr().dispatched().contains(&close_window("0xa2")));
}

#[test]
fn kill_deletes_unshown_empty_workspaces_at_once() {
    let mut e = engine();
    e.handle(Request::KillSession { number: 1 });
    // 100002 was deleted at once (its id may be reused by the new `main`)
    assert_ne!(e.state.workspaces.get(&100002).map(|w| w.session), Some(1));
}

#[test]
fn kill_names_windows_still_open_after_10s() {
    let mut e = engine();
    e.state.workspaces.get_mut(&100002).unwrap().tiles.push(tile("0xa2", "foot"));
    e.hypr().clients.borrow_mut().push(client("0xa2", 2, "foot", 100002));
    e.handle(Request::KillWorkspace { id: 100002 });
    assert!(e.next_deadline().is_some());
    e.on_deadline(Instant::now() + Duration::from_secs(11)).unwrap();
    assert!(e.hypr().notified.borrow()[0].contains("foot"));
    assert_eq!(e.next_deadline(), None);
}

#[test]
fn killing_active_session_switches_monitor_once_its_windows_are_gone() {
    let mut e = engine();
    e.state.workspaces.get_mut(&100001).unwrap().tiles.push(tile("0xa1", "foot"));
    let other = e.state.create_session("LG A", Some("other"));
    let other_ws = e.state.session_workspaces(other)[0].real_id;
    e.handle(Request::KillSession { number: 1 });
    assert_eq!(e.state.monitor("LG A").unwrap().active_session, Some(1));
    e.hypr().monitors.borrow_mut()[0].active_workspace = other_ws;
    e.on_event(Event::CloseWindow { address: "0xa1".into() }).unwrap();
    assert_eq!(e.state.monitor("LG A").unwrap().active_session, Some(other));
    assert_eq!(e.hypr().dispatched().last().unwrap(), &focus_workspace(other_ws));
}

#[test]
fn killing_last_session_of_monitor_makes_main() {
    let mut e = engine();
    e.state.workspaces.get_mut(&100001).unwrap().tiles.push(tile("0xa1", "foot"));
    e.state.sessions.get_mut(&1).unwrap().name = "old".into();
    e.handle(Request::KillSession { number: 1 });
    e.on_event(Event::CloseWindow { address: "0xa1".into() }).unwrap();
    let active = e.state.monitor("LG A").unwrap().active_session.unwrap();
    assert_eq!(e.state.sessions[&active].name, "main");
}

#[test]
fn no_deadline_when_idle() {
    assert_eq!(engine().next_deadline(), None);
}

#[test]
fn cycle_skips_placeholders_and_hidden_windows_and_wraps() {
    let mut e = engine();
    {
        let mut c = e.hypr().clients.borrow_mut();
        c.push(client("0x1", 1, "foot", 100001));
        c.push(client("0xp", 2, "ymux-placeholder", 100001));
        let mut hidden = client("0xh", 3, "foot", 100001);
        hidden.hidden = true;
        c.push(hidden);
        c.push(client("0x2", 4, "kitty", 100001));
        c.push(client("0xother", 5, "foot", 100002));
    }
    *e.hypr().active.borrow_mut() = Some("0x1".into());
    e.handle(Request::Cycle { next: true });
    assert_eq!(e.hypr().dispatched(), vec![focus_window("0x2"), "hl.dsp.window.bring_to_top()".to_string()]);
    e.hypr().clear();
    *e.hypr().active.borrow_mut() = Some("0x2".into());
    e.handle(Request::Cycle { next: true });
    assert_eq!(e.hypr().dispatched()[0], focus_window("0x1"));
    e.hypr().clear();
    *e.hypr().active.borrow_mut() = Some("0x1".into());
    e.handle(Request::Cycle { next: false });
    assert_eq!(
        e.hypr().dispatched(),
        vec!["hl.dsp.window.alter_zorder({ mode = \"bottom\" })".to_string(), focus_window("0x2")]
    );
}
