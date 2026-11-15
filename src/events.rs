//! socket2 event parsing (formats recorded in docs/spikes.md S1, S4).

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    OpenWindow { address: String, workspace_name: String, class: String, title: String },
    CloseWindow { address: String },
    MoveWindow { address: String, workspace: i64, workspace_name: String },
    CreateWorkspace { id: i64 },
    DestroyWorkspace { id: i64 },
    MoveWorkspace { id: i64, monitor: String },
    MonitorAdded { name: String, description: String },
    MonitorRemoved { name: String, description: String },
    Workspace { id: i64 },
    ChangeWorkspaceId { old: i64, new: i64 },
    FloatingChanged { address: String, floating: bool },
    Submap { name: String },
    ActiveWindow { address: Option<String> },
    ConfigReloaded,
    Other,
}

/// socket2 omits the `0x` that `hyprctl -j` uses; ymux always stores `0x...`.
pub fn normalise_address(a: &str) -> String {
    let a = a.trim().to_ascii_lowercase();
    if a.starts_with("0x") { a } else { format!("0x{a}") }
}

pub fn parse_line(line: &str) -> Event {
    let Some((name, data)) = line.split_once(">>") else { return Event::Other };
    parse(name, data).unwrap_or(Event::Other)
}

fn parse(name: &str, data: &str) -> Option<Event> {
    let id = |s: &str| s.trim().parse::<i64>().ok();
    Some(match name {
        "openwindow" => {
            let mut f = data.splitn(4, ',');
            Event::OpenWindow {
                address: normalise_address(f.next()?),
                workspace_name: f.next()?.to_string(),
                class: f.next()?.to_string(),
                title: f.next().unwrap_or("").to_string(),
            }
        }
        "closewindow" => Event::CloseWindow { address: normalise_address(data) },
        "movewindowv2" => {
            let mut f = data.splitn(3, ',');
            Event::MoveWindow {
                address: normalise_address(f.next()?),
                workspace: id(f.next()?)?,
                workspace_name: f.next()?.to_string(),
            }
        }
        "createworkspacev2" => Event::CreateWorkspace { id: id(data.split(',').next()?)? },
        "destroyworkspacev2" => Event::DestroyWorkspace { id: id(data.split(',').next()?)? },
        "workspacev2" => Event::Workspace { id: id(data.split(',').next()?)? },
        "moveworkspacev2" => {
            let mut f = data.splitn(3, ',');
            let ws = id(f.next()?)?;
            f.next()?;
            Event::MoveWorkspace { id: ws, monitor: f.next()?.to_string() }
        }
        "monitoraddedv2" | "monitorremovedv2" => {
            let mut f = data.splitn(3, ',');
            f.next()?;
            let name_ = f.next()?.to_string();
            let description = f.next()?.to_string();
            if name == "monitoraddedv2" {
                Event::MonitorAdded { name: name_, description }
            } else {
                Event::MonitorRemoved { name: name_, description }
            }
        }
        "changeworkspaceid" => {
            let (a, b) = data.split_once(',')?;
            Event::ChangeWorkspaceId { old: id(a)?, new: id(b)? }
        }
        "changefloatingmode" => {
            let (a, b) = data.split_once(',')?;
            Event::FloatingChanged { address: normalise_address(a), floating: b.trim() == "1" }
        }
        "submap" => Event::Submap { name: data.to_string() },
        "activewindowv2" => Event::ActiveWindow {
            address: (!data.trim().is_empty()).then(|| normalise_address(data)),
        },
        "configreloaded" => Event::ConfigReloaded,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openwindow_and_normalises_address() {
        assert_eq!(
            parse_line("openwindow>>560362726330,12,ymux-spike,foot"),
            Event::OpenWindow {
                address: "0x560362726330".into(),
                workspace_name: "12".into(),
                class: "ymux-spike".into(),
                title: "foot".into()
            }
        );
    }

    #[test]
    fn title_with_commas_is_kept_whole() {
        let Event::OpenWindow { title, .. } = parse_line("openwindow>>ab,1,kitty,a, b, c") else { panic!() };
        assert_eq!(title, "a, b, c");
    }

    #[test]
    fn parses_closewindow() {
        assert_eq!(parse_line("closewindow>>560362726330"), Event::CloseWindow { address: "0x560362726330".into() });
    }

    #[test]
    fn parses_movewindow_v2_only() {
        assert_eq!(
            parse_line("movewindowv2>>560362726330,100001,100001"),
            Event::MoveWindow { address: "0x560362726330".into(), workspace: 100001, workspace_name: "100001".into() }
        );
        assert_eq!(parse_line("movewindow>>560362726330,100001"), Event::Other);
    }

    #[test]
    fn parses_special_workspace_move() {
        assert_eq!(
            parse_line("movewindowv2>>ab,-98,special:qconsole"),
            Event::MoveWindow { address: "0xab".into(), workspace: -98, workspace_name: "special:qconsole".into() }
        );
    }

    #[test]
    fn parses_workspace_lifecycle_v2() {
        assert_eq!(parse_line("createworkspacev2>>100001,100001"), Event::CreateWorkspace { id: 100001 });
        assert_eq!(parse_line("destroyworkspacev2>>100002,100002"), Event::DestroyWorkspace { id: 100002 });
        assert_eq!(parse_line("workspacev2>>100002,100002"), Event::Workspace { id: 100002 });
        assert_eq!(parse_line("moveworkspacev2>>12,12,eDP-2"), Event::MoveWorkspace { id: 12, monitor: "eDP-2".into() });
        assert_eq!(parse_line("createworkspace>>100001"), Event::Other);
    }

    #[test]
    fn parses_monitor_events_v2() {
        let desc = "HKC OVERSEAS LIMITED GN02 0000000000001";
        assert_eq!(
            parse_line(&format!("monitorremovedv2>>1,DP-1,{desc}")),
            Event::MonitorRemoved { name: "DP-1".into(), description: desc.into() }
        );
        assert_eq!(
            parse_line(&format!("monitoraddedv2>>1,DP-1,{desc}")),
            Event::MonitorAdded { name: "DP-1".into(), description: desc.into() }
        );
        assert_eq!(parse_line("monitorremoved>>DP-1"), Event::Other);
    }

    #[test]
    fn parses_misc_events() {
        assert_eq!(parse_line("configreloaded>>"), Event::ConfigReloaded);
        assert_eq!(parse_line("changeworkspaceid>>100001,100002"), Event::ChangeWorkspaceId { old: 100001, new: 100002 });
        assert_eq!(
            parse_line("changefloatingmode>>560362726330,1"),
            Event::FloatingChanged { address: "0x560362726330".into(), floating: true }
        );
        assert_eq!(parse_line("submap>>ymux"), Event::Submap { name: "ymux".into() });
        assert_eq!(parse_line("submap>>"), Event::Submap { name: String::new() });
    }

    #[test]
    fn parses_activewindow_v2() {
        assert_eq!(parse_line("activewindowv2>>560362726330"), Event::ActiveWindow { address: Some("0x560362726330".into()) });
        assert_eq!(parse_line("activewindowv2>>"), Event::ActiveWindow { address: None });
    }

    #[test]
    fn unknown_or_malformed_is_other() {
        assert_eq!(parse_line("bell>>x"), Event::Other);
        assert_eq!(parse_line("workspacev2>>notanumber,x"), Event::Other);
        assert_eq!(parse_line("garbage"), Event::Other);
    }
}
