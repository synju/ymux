//! Test double for the `Hypr` trait: returns its fields and records what ymux sends.
#![allow(dead_code)]

use anyhow::Result;
use ymux::hypr::{HClient, HMonitor, HWorkspace, Hypr, ProcInfo};
use std::collections::HashMap;
use std::cell::{Cell, RefCell};

#[derive(Default)]
pub struct FakeHypr {
    pub monitors: RefCell<Vec<HMonitor>>,
    pub workspaces: RefCell<Vec<HWorkspace>>,
    pub clients: RefCell<Vec<HClient>>,
    pub active: RefCell<Option<String>>,
    pub dispatched: RefCell<Vec<String>>,
    pub evaled: RefCell<Vec<String>>,
    pub spawned: RefCell<Vec<Vec<String>>>,
    pub next_pid: Cell<u32>,
    pub notified: RefCell<Vec<String>>,
    pub procs: RefCell<HashMap<u32, ProcInfo>>,
    pub spawned_cwds: RefCell<Vec<Option<String>>>,
    /// Dispatches containing any of these fail (a dead window, a hyprctl hiccup).
    pub fail_dispatch: RefCell<Vec<String>>,
    pub workspaces_calls: Cell<u32>,
    /// `hyprctl -j monitors` fails (a transient hiccup).
    pub fail_monitors: Cell<bool>,
}

impl FakeHypr {
    pub fn dispatched(&self) -> Vec<String> {
        self.dispatched.borrow().clone()
    }
    pub fn clear(&self) {
        self.dispatched.borrow_mut().clear();
        self.evaled.borrow_mut().clear();
    }
}

impl Hypr for FakeHypr {
    fn monitors(&self) -> Result<Vec<HMonitor>> {
        if self.fail_monitors.get() {
            anyhow::bail!("hyprctl failed");
        }
        Ok(self.monitors.borrow().clone())
    }
    fn workspaces(&self) -> Result<Vec<HWorkspace>> {
        self.workspaces_calls.set(self.workspaces_calls.get() + 1);
        Ok(self.workspaces.borrow().clone())
    }
    fn clients(&self) -> Result<Vec<HClient>> {
        Ok(self.clients.borrow().clone())
    }
    fn active_address(&self) -> Result<Option<String>> {
        Ok(self.active.borrow().clone())
    }
    fn dispatch(&self, lua: &str) -> Result<()> {
        self.dispatched.borrow_mut().push(lua.to_string());
        if self.fail_dispatch.borrow().iter().any(|f| lua.contains(f.as_str())) {
            anyhow::bail!("dispatch failed: {lua}");
        }
        Ok(())
    }
    fn eval(&self, lua: &str) -> Result<()> {
        self.evaled.borrow_mut().push(lua.to_string());
        Ok(())
    }
    fn spawn(&self, argv: &[String], cwd: Option<&std::path::Path>) -> Result<u32> {
        self.spawned.borrow_mut().push(argv.to_vec());
        self.spawned_cwds.borrow_mut().push(cwd.map(|p| p.display().to_string()));
        let pid = if self.next_pid.get() == 0 { 9000 } else { self.next_pid.get() };
        self.next_pid.set(pid + 1);
        Ok(pid)
    }
    fn proc_info(&self, pid: u32) -> ProcInfo {
        self.procs.borrow().get(&pid).cloned().unwrap_or_default()
    }
    fn notify(&self, msg: &str) {
        self.notified.borrow_mut().push(msg.to_string());
    }
}

pub fn monitor(name: &str, description: &str, x: i32, focused: bool, active: i64) -> HMonitor {
    HMonitor { name: name.into(), description: description.into(), focused, active_workspace: active, x, y: 0 }
}

pub fn workspace(id: i64, monitor: &str, windows: u32) -> HWorkspace {
    HWorkspace { id, name: id.to_string(), monitor: monitor.into(), windows, tiled_layout: "dwindle".into() }
}

pub fn client(address: &str, pid: u32, class: &str, workspace: i64) -> HClient {
    HClient {
        address: address.into(),
        pid,
        class: class.into(),
        title: class.into(),
        workspace,
        workspace_name: workspace.to_string(),
        floating: false,
        at: (0, 0),
        size: (100, 100),
        tags: vec![],
        hidden: false,
    }
}
