//! Summoning the plugin's overlays (spec 5): the daemon launches
//! `omarchy-shell shell summon synju.ymux '<json>'` and the menu root picks the
//! view from the payload.

use crate::hypr::{HClient, HMonitor};
use crate::model::reading_key;
use serde::Serialize;

/// One numbered tile for Leader + `q` (spec 5.5); coordinates relative to its monitor.
#[derive(Serialize, Debug, PartialEq)]
pub struct Pane {
    pub key: String,
    pub address: String,
    pub monitor: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// `1`-`9`, then `a`, `b`, `c`, ... so one keypress always picks a tile.
pub fn pane_key(i: usize) -> String {
    if i < 9 { (i + 1).to_string() } else { ((b'a' + (i - 9) as u8) as char).to_string() }
}

/// Visible tiles on every monitor: monitors left to right (then top to bottom), and
/// within a monitor left to right, top to bottom. Placeholders count (spec 5.5).
pub fn number_panes(monitors: &[HMonitor], clients: &[HClient]) -> Vec<Pane> {
    let mut mons: Vec<&HMonitor> = monitors.iter().collect();
    mons.sort_by_key(|m| (m.x, m.y));
    let mut out = Vec::new();
    for m in mons {
        if m.active_workspace <= 0 {
            continue;
        }
        let mut here: Vec<&HClient> = clients.iter().filter(|c| c.workspace == m.active_workspace).collect();
        here.sort_by_key(|c| reading_key(c.at));
        for c in here {
            out.push(Pane {
                key: pane_key(out.len()),
                address: c.address.clone(),
                monitor: m.name.clone(),
                x: c.at.0 - m.x,
                y: c.at.1 - m.y,
                w: c.size.0,
                h: c.size.1,
            });
        }
    }
    out
}

pub fn summon_argv(payload: &serde_json::Value) -> Vec<String> {
    vec![
        "omarchy-shell".into(),
        "shell".into(),
        "summon".into(),
        "synju.ymux".into(),
        payload.to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::hypr::{HClient, HMonitor};

    fn mon(name: &str, x: i32, active: i64) -> HMonitor {
        HMonitor { name: name.into(), description: name.into(), focused: false, active_workspace: active, x, y: 0 }
    }
    fn cl(addr: &str, ws: i64, at: (i32, i32)) -> HClient {
        HClient { address: addr.into(), pid: 1, class: "c".into(), title: "t".into(), workspace: ws, workspace_name: ws.to_string(), floating: false, at, size: (100, 100), tags: vec![], hidden: false }
    }

    #[test]
    fn pane_keys_digits_then_letters() {
        assert_eq!((0..11).map(pane_key).collect::<Vec<_>>(), ["1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b"]);
    }

    #[test]
    fn panes_number_monitors_left_to_right_then_reading_order() {
        let mons = [mon("DP-1", 1920, 100001), mon("DP-2", 0, 200001)];
        let clients = [cl("0xr", 200001, (960, 0)), cl("0xl", 200001, (0, 0)), cl("0xm", 100001, (2000, 50))];
        let p = number_panes(&mons, &clients);
        let got: Vec<(&str, &str, &str, i32)> = p.iter().map(|p| (p.key.as_str(), p.address.as_str(), p.monitor.as_str(), p.x)).collect();
        assert_eq!(got, [("1", "0xl", "DP-2", 0), ("2", "0xr", "DP-2", 960), ("3", "0xm", "DP-1", 80)]);
    }

    #[test]
    fn panes_skip_unshown_and_special_workspaces() {
        let mons = [mon("DP-1", 0, 100001)];
        let clients = [cl("0xa", 100001, (0, 0)), cl("0xb", 100002, (0, 0)), cl("0xq", -98, (0, 0))];
        assert_eq!(number_panes(&mons, &clients).len(), 1);
    }

    #[test]
    fn summon_argv_shape() {
        assert_eq!(
            summon_argv(&json!({"view":"tree"})),
            ["omarchy-shell", "shell", "summon", "synju.ymux", "{\"view\":\"tree\"}"]
        );
    }
}
