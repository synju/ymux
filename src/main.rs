//! ymux CLI: thin client subcommands, plus `daemon` and `install` (spec 3 item 1).

use ymux::engine::{Request, Response};
use ymux::ipc;
use std::io::BufRead;
use std::process::{exit, Command};

const USAGE: &str = "usage: ymux <focus N | move N [--silent] | next | prev | previous | new-workspace | new [NAME] | switch N | session-move l|r|u|d | action NAME | status | watch | daemon | install | uninstall>";

fn notify(msg: &str) {
    let _ = Command::new("notify-send").args(["ymux", msg]).status();
}

fn parse_request(args: &[String]) -> Option<Request> {
    let n = |i: usize| args.get(i).and_then(|s| s.parse::<u32>().ok());
    Some(match args.first()?.as_str() {
        "focus" => Request::Focus { n: n(1)? },
        "move" => Request::Move { n: n(1)?, follow: !args.iter().any(|a| a == "--silent") },
        "next" => Request::Next,
        "prev" => Request::Prev,
        "previous" => Request::Previous,
        "new-workspace" => Request::NewWorkspace,
        "new" => Request::NewSession { name: (args.len() > 1).then(|| args[1..].join(" ")) },
        "switch" => Request::SwitchSession { number: n(1)? },
        "session-move" => Request::SessionMove { dir: args.get(1)?.chars().next()? },
        "action" => Request::Action { name: args.get(1)?.clone() },
        "status" => Request::Status,
        "focus-id" => Request::FocusId { id: args.get(1)?.parse().ok()? },
        "cycle" => Request::Cycle { next: args.get(1).map(String::as_str) != Some("prev") },
        "close-window" => Request::CloseWindow { address: args.get(1)?.clone() },
        "focus-window" => Request::FocusWindow { address: args.get(1)?.clone() },
        "focus-monitor" => Request::FocusMonitor { connector: args.get(1)?.clone() },
        "command" => Request::Command { line: args.get(1..).unwrap_or_default().join(" ") },
        "set-relaunch" => Request::SetRelaunch { on: args.get(1)? == "on" },
        "fold" => Request::Fold { key: args.get(1)?.clone(), folded: args.get(2)? == "on" },
        "kill-session" => Request::KillSession { number: n(1)? },
        "kill-workspace" => Request::KillWorkspace { id: args.get(1)?.parse().ok()? },
        "rename-session" => Request::RenameSession { number: n(1)?, name: args.get(2..).unwrap_or_default().join(" ") },
        "renumber-workspace" => Request::RenumberWorkspace { id: args.get(1)?.parse().ok()?, number: n(2)? },
        "rename-monitor" => Request::RenameMonitor { key: args.get(1)?.clone(), name: args.get(2..).unwrap_or_default().join(" ") },
        "rename-tile" => Request::RenameTile { address: args.get(1)?.clone(), name: args.get(2..).unwrap_or_default().join(" ") },
        "rename-workspace" => Request::RenameWorkspace { id: args.get(1)?.parse().ok()?, name: args.get(2..).unwrap_or_default().join(" ") },
        "shutdown" => Request::Shutdown,
        _ => return None,
    })
}

/// `quiet` (`--no-notify`, used by the plugin's prompts): errors go to stdout for
/// the prompt to show instead of a desktop notification.
fn client(req: Request, quiet: bool) -> i32 {
    let fail = |msg: &str| {
        if quiet {
            println!("{msg}");
        } else {
            notify(msg);
            eprintln!("{msg}");
        }
        1
    };
    match ipc::send(&req) {
        Err(_) => fail("ymux: daemon not running"),
        Ok(Response { ok: false, message }) => fail(&message.unwrap_or_else(|| "ymux: request failed".into())),
        Ok(Response { ok: true, message }) => {
            if let Some(m) = message {
                print!("{m}");
            }
            0
        }
    }
}

fn watch() -> i32 {
    match ipc::watch() {
        Err(_) => {
            eprintln!("ymux: daemon not running");
            1
        }
        Ok(reader) => {
            for line in reader.lines().map_while(Result::ok) {
                println!("{line}");
            }
            0
        }
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let quiet = args.first().is_some_and(|a| a == "--no-notify");
    if quiet {
        args.remove(0);
    }
    let code = match args.first().map(String::as_str) {
        Some("watch") => watch(),
        Some("placeholder") => match ymux::placeholder::run() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("ymux placeholder: {e:#}");
                1
            }
        },
        Some("uninstall") => match ymux::install::uninstall() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("ymux uninstall: {e:#}");
                1
            }
        },
        Some("install") => match ymux::install::install() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("ymux install: {e:#}");
                1
            }
        },
        Some("daemon") => match ymux::daemon::run() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("ymux daemon: {e:#}");
                ymux::daemon::log_error(&ymux::daemon::log_path(), &format!("ymux daemon: {e:#}"));
                1
            }
        },
        _ => match parse_request(&args) {
            Some(req) => client(req, quiet),
            None => {
                eprintln!("{USAGE}");
                2
            }
        },
    };
    exit(code);
}
