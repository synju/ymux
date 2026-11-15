//! Leader + `?`: every ymux key in one modal, built from the live config (user request
//! 2026-11-16), so rebinds show as they are.

use crate::config::{Action, Config};
use serde_json::{json, Value};

/// `SUPER+B` as the docs write it: `Super + B`.
pub fn pretty_combo(combo: &str) -> String {
    combo
        .split('+')
        .map(|p| {
            let p = p.trim();
            let mut c = p.chars();
            match c.next() {
                Some(f) if p.len() > 1 => f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect(),
                _ => p.to_uppercase(),
            }
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

fn describe(a: Action) -> &'static str {
    match a {
        Action::NewWorkspace => "New workspace in the current session",
        Action::CommandPrompt => "Command prompt (new <name> makes a session)",
        Action::RenameSession => "Rename the session",
        Action::RenameWorkspace => "Rename the workspace",
        Action::Tree => "Tree of monitors, sessions, workspaces and windows",
        Action::Options => "Options (relaunch on or off)",
        Action::DisplayPanes => "Number every visible tile; press one to focus it",
        Action::Mark => "Mark the focused tile",
        Action::Swap => "Swap the focused tile with the marked one",
        Action::KillTile => "Close the focused window, keeping its spot",
        Action::PreviousWorkspace => "Previous workspace",
        Action::NextWorkspace => "Next workspace",
        Action::MoveWorkspace => "Renumber the workspace",
        Action::Help => "This list of keys",
    }
}

/// The `help` view's payload: groups of `[keys, what they do]`.
pub fn help_payload(cfg: &Config) -> Value {
    let mut leader: Vec<Value> = cfg.binds.iter().map(|(k, a)| json!([k, describe(*a)])).collect();
    leader.push(json!(["arrows, Super held", "Resize the focused tile"]));
    leader.push(json!(["Escape", "Leave leader mode"]));
    let mut anywhere: Vec<Value> = cfg.global_binds.iter().map(|(k, a)| json!([pretty_combo(k), describe(*a)])).collect();
    // the takeovers in ymux.lua (spec 2), always present
    for (k, d) in [
        ("Super + 1-0", "Workspace 1-10 of the current session"),
        ("Super + Shift + 1-0", "Move the window there, keeping its spot"),
        ("Super + Shift + Alt + 1-0", "Move the window there without following"),
        ("Super + Tab / Super + Shift + Tab", "Next / previous workspace"),
        ("Super + Ctrl + Tab", "The workspace you were on before"),
        ("Super + scroll", "Next / previous workspace"),
        ("Super + Shift + Alt + arrows", "Move the session to another monitor"),
        ("Alt + Tab / Alt + Shift + Tab", "Next / previous window, skipping empty spots"),
        ("Super + W", "Close the focused window, keeping its spot"),
    ] {
        anywhere.push(json!([k, d]));
    }
    let tree: Vec<Value> = [
        ("j / k or arrows", "Move"),
        ("Enter", "Switch to the selected row"),
        ("r", "Rename the selected row"),
        ("x", "Kill the selected session or workspace"),
        ("h / l", "Fold / unfold"),
        ("Escape", "Close"),
    ]
    .iter()
    .map(|(k, d)| json!([k, d]))
    .collect();
    json!({
        "view": "help",
        "groups": [
            { "title": format!("Leader: {}, then", pretty_combo(&cfg.leader)), "rows": leader },
            { "title": "Anywhere", "rows": anywhere },
            { "title": "In the tree (Leader, w)", "rows": tree },
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{parse, Config};

    fn rows<'a>(p: &'a serde_json::Value, group: &str) -> Vec<(&'a str, &'a str)> {
        p["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["title"].as_str().unwrap().starts_with(group))
            .unwrap()["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r[0].as_str().unwrap(), r[1].as_str().unwrap()))
            .collect()
    }

    #[test]
    fn payload_is_a_help_view_with_three_groups() {
        let p = help_payload(&Config::default());
        assert_eq!(p["view"], "help");
        let titles: Vec<&str> = p["groups"].as_array().unwrap().iter().map(|g| g["title"].as_str().unwrap()).collect();
        assert_eq!(titles, vec!["Leader: Super + B, then", "Anywhere", "In the tree (Leader, w)"]);
    }

    #[test]
    fn leader_rows_follow_the_config() {
        let (c, _) = parse("bind t tree\n");
        let p = help_payload(&c);
        let leader = rows(&p, "Leader");
        assert!(leader.contains(&("t", "Tree of monitors, sessions, workspaces and windows")));
        // like tmux `bind`, a new key is added; `w` keeps working too
        assert!(leader.contains(&("w", "Tree of monitors, sessions, workspaces and windows")));
        assert!(leader.contains(&("?", "This list of keys")));
    }

    #[test]
    fn global_binds_and_fixed_keys_are_listed() {
        let p = help_payload(&Config::default());
        let any = rows(&p, "Anywhere");
        assert!(any.contains(&("Shift + Ctrl + Alt + L", "Next workspace")));
        assert!(any.contains(&("Super + 1-0", "Workspace 1-10 of the current session")));
        assert!(any.contains(&("Super + W", "Close the focused window, keeping its spot")));
        let tree = rows(&p, "In the tree");
        assert!(tree.contains(&("r", "Rename the selected row")));
    }

    #[test]
    fn combos_read_like_the_docs() {
        assert_eq!(pretty_combo("SUPER+B"), "Super + B");
        assert_eq!(pretty_combo("SHIFT+CTRL+ALT+H"), "Shift + Ctrl + Alt + H");
    }
}
