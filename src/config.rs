//! `~/.ymux.conf`: tmux-style config (spec 6.3).

use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    NewWorkspace,
    CommandPrompt,
    RenameSession,
    RenameWorkspace,
    Tree,
    Options,
    DisplayPanes,
    Mark,
    Swap,
    KillTile,
    PreviousWorkspace,
    NextWorkspace,
    MoveWorkspace,
}

impl Action {
    pub const ALL: [Action; 13] = [
        Action::NewWorkspace,
        Action::CommandPrompt,
        Action::RenameSession,
        Action::RenameWorkspace,
        Action::Tree,
        Action::Options,
        Action::DisplayPanes,
        Action::Mark,
        Action::Swap,
        Action::KillTile,
        Action::PreviousWorkspace,
        Action::NextWorkspace,
        Action::MoveWorkspace,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Action::NewWorkspace => "new-workspace",
            Action::CommandPrompt => "command-prompt",
            Action::RenameSession => "rename-session",
            Action::RenameWorkspace => "rename-workspace",
            Action::Tree => "tree",
            Action::Options => "options",
            Action::DisplayPanes => "display-panes",
            Action::Mark => "mark",
            Action::Swap => "swap",
            Action::KillTile => "kill-tile",
            Action::PreviousWorkspace => "previous-workspace",
            Action::NextWorkspace => "next-workspace",
            Action::MoveWorkspace => "move-workspace",
        }
    }

    pub fn from_name(name: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.name() == name)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub leader: String,
    pub binds: Vec<(String, Action)>,
    pub global_binds: Vec<(String, Action)>,
    pub relaunch: bool,
    pub leader_timeout_ms: u32,
    pub display_panes_ms: u32,
    pub resize_step: u32,
    pub launch_overrides: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        let binds = [
            ("c", Action::NewWorkspace),
            (":", Action::CommandPrompt),
            ("$", Action::RenameSession),
            (",", Action::RenameWorkspace),
            ("w", Action::Tree),
            ("o", Action::Options),
            ("q", Action::DisplayPanes),
            ("m", Action::Mark),
            ("v", Action::Swap),
            ("x", Action::KillTile),
            (".", Action::MoveWorkspace),
        ];
        Config {
            leader: "SUPER+B".into(),
            binds: binds.iter().map(|(k, a)| (k.to_string(), *a)).collect(),
            global_binds: vec![
                ("SHIFT+CTRL+ALT+H".into(), Action::PreviousWorkspace),
                ("SHIFT+CTRL+ALT+L".into(), Action::NextWorkspace),
            ],
            relaunch: true,
            leader_timeout_ms: 2000,
            display_panes_ms: 2000,
            resize_step: 50,
            launch_overrides: BTreeMap::new(),
        }
    }
}

pub fn config_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".ymux.conf")
}

/// The file `ymux install` writes when none exists.
pub fn default_text() -> String {
    let c = Config::default();
    let mut s = String::from("# ymux config (like ~/.tmux.conf)\n\n# leader and leader actions\n");
    s += &format!("set leader {}\n", c.leader);
    for (k, a) in &c.binds {
        s += &format!("bind {k} {}\n", a.name());
    }
    for (k, a) in &c.global_binds {
        s += &format!("bind-global {k} {}\n", a.name());
    }
    s += "\n# options\n";
    s += &format!("set relaunch {}\n", if c.relaunch { "on" } else { "off" });
    s += &format!("set leader-timeout {}\n", c.leader_timeout_ms);
    s += &format!("set display-panes-time {}\n", c.display_panes_ms);
    s += &format!("set resize-step {}\n", c.resize_step);
    s += "\n# relaunch overrides for wrapped apps, e.g.\n# launch-override <class> \"<command>\"\n";
    s
}

/// Parses config text over the defaults. Returns the config and (1-based line, message)
/// for every line that could not be applied.
pub fn parse(text: &str) -> (Config, Vec<(usize, String)>) {
    let mut c = Config::default();
    let mut errors = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Err(msg) = apply_line(&mut c, line) {
            errors.push((i + 1, msg));
        }
    }
    (c, errors)
}

/// One key: an ASCII letter or digit, one supported symbol, or an xkb key name.
fn valid_key(k: &str) -> bool {
    const SYMBOLS: &str = "$:,./;'-=[]\\`";
    (k.chars().count() == 1 && (k.chars().all(|c| c.is_ascii_alphanumeric()) || SYMBOLS.contains(k)))
        || (!k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

/// `MOD(+MOD)*+KEY`, e.g. `SUPER+B` or `SHIFT+CTRL+ALT+H`.
fn valid_combo(keys: &str) -> bool {
    let parts: Vec<&str> = keys.split('+').collect();
    parts.len() >= 2 && parts.iter().all(|p| valid_key(p) && !p.is_empty())
}

fn apply_line(c: &mut Config, line: &str) -> Result<(), String> {
    let mut parts = line.splitn(3, char::is_whitespace);
    let cmd = parts.next().unwrap_or("");
    let a = parts.next().unwrap_or("").trim();
    let b = parts.next().unwrap_or("").trim();
    let action = |name: &str| Action::from_name(name).ok_or(format!("unknown action `{name}`"));
    let number = |v: &str| v.parse::<u32>().map_err(|_| format!("`{v}` is not a number"));
    match cmd {
        "set" => match a {
            "leader" if b.is_empty() => return Err("set leader needs keys, e.g. SUPER+B".into()),
            "leader" if !valid_combo(b) => return Err(format!("unsupported key `{b}`")),
            "leader" => c.leader = b.to_string(),
            "relaunch" => {
                c.relaunch = match b {
                    "on" => true,
                    "off" => false,
                    _ => return Err(format!("relaunch must be on or off, got `{b}`")),
                }
            }
            "leader-timeout" => c.leader_timeout_ms = number(b)?,
            "display-panes-time" => c.display_panes_ms = number(b)?,
            "resize-step" => c.resize_step = number(b)?,
            _ => return Err(format!("unknown setting `{a}`")),
        },
        "bind" if !a.is_empty() => {
            if !valid_key(a) {
                return Err(format!("unsupported key `{a}`"));
            }
            let act = action(b)?;
            c.binds.retain(|(k, _)| k != a);
            c.binds.push((a.to_string(), act));
        }
        "bind-global" if !a.is_empty() => {
            if !valid_combo(a) {
                return Err(format!("unsupported key `{a}`"));
            }
            let act = action(b)?;
            // one global key per action: a config line replaces the default key
            c.global_binds.retain(|(k, x)| k != a && *x != act);
            c.global_binds.push((a.to_string(), act));
        }
        "launch-override" if !a.is_empty() => {
            let cmdline = b
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .ok_or("launch-override command must be in double quotes")?;
            c.launch_overrides.insert(a.to_string(), cmdline.to_string());
        }
        _ => return Err(format!("cannot read `{line}`")),
    }
    Ok(())
}

/// Rewrites only the `set relaunch` line (appending one if absent), keeping the rest.
pub fn set_relaunch_line(text: &str, on: bool) -> String {
    let want = format!("set relaunch {}", if on { "on" } else { "off" });
    let mut found = false;
    let mut out: Vec<String> = text
        .lines()
        .map(|l| {
            let t: Vec<&str> = l.split_whitespace().collect();
            if t.len() >= 2 && t[0] == "set" && t[1] == "relaunch" {
                found = true;
                want.clone()
            } else {
                l.to_string()
            }
        })
        .collect();
    if !found {
        out.push(want);
    }
    out.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_text_round_trips() {
        let (c, e) = parse(&default_text());
        assert!(e.is_empty(), "{e:?}");
        assert_eq!(c, Config::default());
    }

    #[test]
    fn bad_line_is_skipped_and_named_rest_applies() {
        let (c, e) = parse("set leader SUPER+N\nfrobnicate x\nset relaunch off\n");
        assert_eq!(c.leader, "SUPER+N");
        assert!(!c.relaunch);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].0, 2);
    }

    #[test]
    fn bind_replaces_default_for_that_key() {
        let (c, _) = parse("bind c tree\n");
        assert_eq!(c.binds.iter().find(|(k, _)| k == "c").unwrap().1, Action::Tree);
        assert_eq!(c.binds.iter().filter(|(k, _)| k == "c").count(), 1);
    }

    #[test]
    fn bind_global_replaces_the_actions_previous_keys() {
        let (c, _) = parse("bind-global SUPER+F11 previous-workspace\n");
        let prev: Vec<&str> = c.global_binds.iter().filter(|(_, a)| *a == Action::PreviousWorkspace).map(|(k, _)| k.as_str()).collect();
        assert_eq!(prev, ["SUPER+F11"]);
        assert_eq!(c.global_binds.len(), 2);
    }

    #[test]
    fn empty_leader_says_what_to_write() {
        let (_, e) = parse("set leader\n");
        assert!(e[0].1.contains("SUPER+B"), "{e:?}");
    }

    #[test]
    fn quote_in_bind_key_is_rejected() {
        let (c, e) = parse("bind \" tree\n");
        assert_eq!(e.len(), 1);
        assert_eq!(c, Config::default());
        let (_, e) = parse("set leader SUPER+\"x\n");
        assert_eq!(e.len(), 1);
        let (_, e) = parse("bind-global SUPER+F11 tree\nbind-global SUPER+a\"b tree\n");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].0, 2);
    }

    #[test]
    fn unknown_action_is_an_error() {
        let (_, e) = parse("bind c fly\n");
        assert_eq!(e[0].0, 1);
    }

    #[test]
    fn non_numeric_timeout_is_an_error_and_default_kept() {
        let (c, e) = parse("set leader-timeout soon\n");
        assert_eq!(e.len(), 1);
        assert_eq!(c.leader_timeout_ms, 2000);
    }

    #[test]
    fn launch_override_keeps_quoted_command() {
        let (c, _) = parse("launch-override steam \"steam -silent\"\n");
        assert_eq!(c.launch_overrides["steam"], "steam -silent");
    }

    #[test]
    fn set_relaunch_line_preserves_comments() {
        let out = set_relaunch_line("# mine\nset relaunch on\nset leader SUPER+B\n", false);
        assert_eq!(out, "# mine\nset relaunch off\nset leader SUPER+B\n");
    }

    #[test]
    fn set_relaunch_line_appends_when_absent() {
        assert_eq!(set_relaunch_line("set leader SUPER+B\n", true), "set leader SUPER+B\nset relaunch on\n");
    }

    #[test]
    fn action_names_round_trip() {
        for a in Action::ALL {
            assert_eq!(Action::from_name(a.name()), Some(a));
        }
    }
}
