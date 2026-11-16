//! `ymux daemon`: single instance, one thread per input, one loop that owns the
//! engine (spec 3 item 2, 3.1, 6; docs/spikes.md).

use crate::config::{config_path, parse, Config};
use crate::engine::Engine;
use crate::events::parse_line;
use crate::hypr::Hyprctl;
use crate::ipc::{self, Msg};
use crate::{layout, luagen, store};
use anyhow::{bail, Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

const LOCK_WAIT: Duration = Duration::from_secs(5);
const SETTLE: Duration = Duration::from_millis(300);
const TICK: Duration = Duration::from_secs(30);
const CONFIG_POLL: Duration = Duration::from_secs(2);

/// How long the loop may block: until the 300 ms settle after the last monitor event
/// or the engine's next deadline, whichever is first; `None` blocks indefinitely.
pub fn loop_timeout(pending_since: Option<Instant>, deadline: Option<Instant>, now: Instant) -> Option<Duration> {
    let settle = pending_since.map(|t| (t + SETTLE).saturating_duration_since(now));
    let due = deadline.map(|d| d.saturating_duration_since(now));
    match (settle, due) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// How long the daemon waits at boot for Hyprland to answer (its top-level start
/// runs while Hyprland is still reading its config, before its sockets exist).
const READY_WAIT: Duration = Duration::from_secs(30);
const READY_STEP: Duration = Duration::from_millis(250);

/// Polls `ready` every `step` until it is true or `timeout` has been slept away.
pub fn wait_until(mut ready: impl FnMut() -> bool, timeout: Duration, step: Duration, mut sleep: impl FnMut(Duration)) -> bool {
    let mut waited = Duration::ZERO;
    loop {
        if ready() {
            return true;
        }
        if waited >= timeout {
            return false;
        }
        sleep(step);
        waited += step;
    }
}

/// Hyprland is ready once its event socket accepts and it reports a monitor.
fn hyprland_ready(run_dir: &Path, signature: &str) -> bool {
    use crate::hypr::Hypr;
    UnixStream::connect(run_dir.join("hypr").join(signature).join(".socket2.sock")).is_ok()
        && Hyprctl.monitors().is_ok_and(|m| !m.is_empty())
}

/// Hyprland drops the daemon's output, so errors also go to
/// `~/.local/state/ymux/daemon.log` (one line each, unix time first).
pub fn log_error(path: &Path, msg: &str) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let secs = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{secs} {msg}");
    }
}

pub fn log_path() -> PathBuf {
    store::state_path().with_file_name("daemon.log")
}

/// The view as JSON for the bar, or `None` (logged) when it cannot be read right now:
/// a hyprctl hiccup must never end the daemon (final review).
pub fn view_json<H: crate::hypr::Hypr>(engine: &Engine<H>) -> Option<String> {
    match engine.view().and_then(|v| Ok(serde_json::to_string(&v)?)) {
        Ok(json) => Some(json),
        Err(e) => {
            eprintln!("ymux: view: {e:#}");
            None
        }
    }
}

pub fn notify(msg: &str) {
    crate::messages::notify(msg);
}

/// Where install puts the binary; the generated Lua always points here, never at the
/// build that happens to be running (review finding 5).
/// Where the package puts the binary.
pub const SYSTEM_EXE: &str = "/usr/bin/ymux";

pub fn user_exe() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/bin/ymux")
}

/// The binary the generated Lua and the daemon run: a user's own copy (a build
/// installed with `ymux install`) wins, else the package's, else the user path.
pub fn pick_exe(local: &Path, local_exists: bool, system_exists: bool) -> PathBuf {
    if !local_exists && system_exists { PathBuf::from(SYSTEM_EXE) } else { local.to_path_buf() }
}

pub fn installed_exe() -> PathBuf {
    let local = user_exe();
    pick_exe(&local, local.exists(), Path::new(SYSTEM_EXE).exists())
}

pub fn lua_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/hypr/ymux.lua")
}

/// The regenerated Lua if it differs from what is on disk; writing it is the reload,
/// since Hyprland reloads by itself when a config file changes (spikes.md S3).
pub fn lua_update(current: &str, cfg: &Config, exe: &str) -> Option<String> {
    let new = luagen::generate(cfg, exe);
    (new != current).then_some(new)
}

/// True while the Hyprland instance this daemon belongs to still exists.
pub fn instance_alive(run_dir: &Path, signature: &str) -> bool {
    run_dir.join("hypr").join(signature).exists()
}

fn try_lock(file: &File, wait: Duration) -> bool {
    let deadline = Instant::now() + wait;
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn load_config(notify_errors: bool) -> Config {
    let text = fs::read_to_string(config_path()).unwrap_or_default();
    let (cfg, errors) = parse(&text);
    if notify_errors && !errors.is_empty() {
        let lines: Vec<String> = errors.iter().map(|(n, m)| format!("line {n}: {m}")).collect();
        notify(&format!("~/.ymux.conf: skipped {}", lines.join("; ")));
    }
    cfg
}

fn mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn spawn_socket2(run_dir: &Path, signature: &str, tx: Sender<Msg>) -> Result<()> {
    let path = run_dir.join("hypr").join(signature).join(".socket2.sock");
    let stream = UnixStream::connect(&path).with_context(|| format!("connecting to {}", path.display()))?;
    thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            if tx.send(Msg::Event(parse_line(&line))).is_err() {
                return;
            }
        }
        let _ = tx.send(Msg::Socket2Closed);
    });
    Ok(())
}

fn spawn_timers(tx: Sender<Msg>) {
    let t = tx.clone();
    thread::spawn(move || {
        loop {
            thread::sleep(TICK);
            if t.send(Msg::Tick).is_err() {
                return;
            }
        }
    });
    thread::spawn(move || {
        let path = config_path();
        let mut last = mtime(&path);
        loop {
            thread::sleep(CONFIG_POLL);
            let now = mtime(&path);
            if now != last {
                last = now;
                if tx.send(Msg::ConfigChanged).is_err() {
                    return;
                }
            }
        }
    });
}

/// A watcher gets a 200 ms write timeout, so a stalled client is dropped instead of
/// blocking the daemon loop.
pub fn register_watcher(stream: UnixStream) -> UnixStream {
    let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
    stream
}

fn send_state(stream: &mut UnixStream, json: &str) -> bool {
    stream.write_all(json.as_bytes()).and_then(|_| stream.write_all(b"\n")).is_ok()
}

pub fn run() -> Result<()> {
    let run_dir = ipc::runtime_dir();
    fs::create_dir_all(&run_dir).ok();
    let lock = OpenOptions::new().create(true).truncate(false).write(true).open(run_dir.join("ymux.lock"))?;
    if !try_lock(&lock, Duration::ZERO) {
        // Every config reload runs the top-level start (spikes.md S3): a live daemon
        // that answers means there is nothing to do.
        if ipc::send(&crate::engine::Request::Status).is_ok() {
            return Ok(());
        }
        if !try_lock(&lock, LOCK_WAIT) {
            notify("ymux: another daemon is still running");
            return Ok(());
        }
    }
    // We hold the lock: any socket file is stale. Never removed on exit (spec 3 item 2).
    let sock = ipc::socket_path();
    let _ = fs::remove_file(&sock);
    let listener = UnixListener::bind(&sock).with_context(|| format!("binding {}", sock.display()))?;

    let Ok(signature) = std::env::var("HYPRLAND_INSTANCE_SIGNATURE") else {
        bail!("HYPRLAND_INSTANCE_SIGNATURE is not set; ymux runs under Hyprland");
    };
    let exe = installed_exe().display().to_string();
    let state_path = store::state_path();
    // a new Hyprland starts from the restore snapshot: the old one's shutdown closed
    // every window and stripped state.json (spec 6.1)
    let snapshot = store::load(&store::snapshot_path()).unwrap_or_else(|e| {
        eprintln!("ymux: ignoring the restore snapshot: {e:#}");
        None
    });
    let state = store::start_state(store::load(&state_path)?, snapshot, &signature).unwrap_or_default();
    if !wait_until(|| hyprland_ready(&run_dir, &signature), READY_WAIT, READY_STEP, thread::sleep) {
        bail!("Hyprland did not answer within {} s", READY_WAIT.as_secs());
    }
    // Started at boot, before Hyprland exported WAYLAND_DISPLAY, the daemon inherits it
    // empty, and every app or placeholder it starts could not reach the display. Take
    // it from Hyprland (spikes.md S3).
    if std::env::var("WAYLAND_DISPLAY").map_or(true, |v| v.is_empty())
        && let Some(socket) = crate::hypr::wayland_socket(&signature)
    {
        // SAFETY: no other thread has been started yet
        unsafe { std::env::set_var("WAYLAND_DISPLAY", socket) };
    }
    let (tx, rx) = channel::<Msg>();
    // connect first so no window mapping during the start scan is missed
    spawn_socket2(&run_dir, &signature, tx.clone())?;
    let mut engine = Engine::new(state, load_config(true), Hyprctl, signature.clone(), layout::layouts_dir());
    engine.start()?;
    store::save_atomic(&engine.state, &state_path)?;

    {
        let tx = tx.clone();
        thread::spawn(move || ipc::serve(listener, tx));
    }
    spawn_timers(tx);

    let mut watchers: Vec<UnixStream> = Vec::new();
    let mut pending_since: Option<Instant> = None;
    // the snapshot lags state.json while windows are closing; it catches up on the next
    // write after 5 quiet seconds (or the 30 s tick)
    let mut stale_snapshot = true;
    loop {
        let msg = match loop_timeout(pending_since, engine.next_deadline(), Instant::now()) {
            Some(wait) => match rx.recv_timeout(wait) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => break,
            },
        };
        let result = match msg {
            None => {
                let mut r = Ok(());
                if pending_since.is_some_and(|t| t.elapsed() >= SETTLE) {
                    pending_since = None;
                    r = engine.enforce_invariant();
                }
                r.and_then(|_| engine.on_deadline(Instant::now()))
            }
            Some(Msg::Event(ev)) => engine.on_event(ev),
            Some(Msg::Request(crate::engine::Request::Shutdown, reply)) => {
                let _ = reply.send(crate::engine::Response::ok());
                break;
            }
            Some(Msg::Request(req, reply)) => {
                let _ = reply.send(engine.handle(req));
                Ok(())
            }
            Some(Msg::Watch(s)) => {
                // a watcher whose first view cannot be read is dropped; the shell
                // reconnects in 2 s
                let mut s = register_watcher(s);
                if let Some(json) = view_json(&engine)
                    && send_state(&mut s, &json)
                {
                    watchers.push(s);
                }
                Ok(())
            }
            Some(Msg::Tick) => {
                if !instance_alive(&run_dir, &signature) {
                    break;
                }
                engine.tick()
            }
            Some(Msg::ConfigChanged) => {
                engine.config = load_config(true);
                let lua = lua_path();
                let current = fs::read_to_string(&lua).unwrap_or_default();
                match lua_update(&current, &engine.config, &exe) {
                    Some(new) => store::write_atomic(&lua, &new).context("writing ymux.lua"),
                    None => Ok(()),
                }
            }
            Some(Msg::Socket2Closed) => break,
        };
        if let Err(e) = result {
            eprintln!("ymux: {e:#}");
        }
        // a monitor or workspace-move event (re)starts the 300 ms settle window
        if engine.invariant_pending {
            engine.invariant_pending = false;
            pending_since = Some(Instant::now());
        }
        let mut dirty = engine.take_dirty();
        // geometry is captured whenever the state file is written (spec 6.2)
        if dirty {
            let _ = engine.capture_geometry();
            dirty |= engine.take_dirty();
        }
        let view_changed = engine.take_view_changed();
        if dirty && let Err(e) = store::save_atomic(&engine.state, &state_path) {
            eprintln!("ymux: saving state: {e:#}");
        }
        if (dirty || stale_snapshot) && !engine.restoring() && store::snapshot_due(engine.last_close, Instant::now()) {
            stale_snapshot = store::save_atomic(&engine.state, &store::snapshot_path()).is_err();
        } else if dirty {
            stale_snapshot = true;
        }
        if (dirty || view_changed)
            && !watchers.is_empty()
            && let Some(json) = view_json(&engine)
        {
            watchers.retain_mut(|w| send_state(w, &json));
        }
    }
    drop(lock);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{parse, Config};

    #[test]
    fn config_change_without_lua_change_does_not_reload() {
        let lua = crate::luagen::generate(&Config::default(), "x");
        let (relaunch_off, _) = parse("set relaunch off\n");
        assert!(!lua_update(&lua, &relaunch_off, "x").is_some());
        let (new_leader, _) = parse("set leader SUPER+N\n");
        assert!(lua_update(&lua, &new_leader, "x").is_some());
    }

    #[test]
    fn lua_points_at_installed_binary() {
        assert!(installed_exe().ends_with("bin/ymux"));
    }

    #[test]
    fn stalled_watcher_is_dropped_within_a_second() {
        use std::time::{Duration, Instant};
        let (a, _b_never_reads) = UnixStream::pair().unwrap();
        let mut w = register_watcher(a);
        let t = Instant::now();
        let big = "x".repeat(8 << 20);
        assert!(!send_state(&mut w, &big));
        assert!(t.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn loop_timeout_picks_the_earliest() {
        use std::time::{Duration, Instant};
        let now = Instant::now();
        assert_eq!(loop_timeout(None, None, now), None);
        assert_eq!(loop_timeout(Some(now), None, now), Some(Duration::from_millis(300)));
        assert_eq!(loop_timeout(Some(now), Some(now + Duration::from_millis(50)), now), Some(Duration::from_millis(50)));
        assert_eq!(loop_timeout(None, Some(now - Duration::from_secs(1)), now), Some(Duration::ZERO));
    }

    #[test]
    fn signature_dir_check() {
        let d = std::env::temp_dir().join(format!("ymux-sig-{}", std::process::id()));
        std::fs::create_dir_all(d.join("hypr/abc")).unwrap();
        assert!(instance_alive(&d, "abc"));
        assert!(!instance_alive(&d, "gone"));
    }

    #[test]
    fn wait_retries_until_ready() {
        let mut calls = 0;
        let mut slept = Duration::ZERO;
        let ok = wait_until(|| { calls += 1; calls == 3 }, Duration::from_secs(30), Duration::from_millis(250), |d| slept += d);
        assert!(ok);
        assert_eq!(calls, 3);
        assert_eq!(slept, Duration::from_millis(500));
    }

    #[test]
    fn wait_gives_up_after_timeout() {
        let mut slept = Duration::ZERO;
        let ok = wait_until(|| false, Duration::from_secs(1), Duration::from_millis(250), |d| slept += d);
        assert!(!ok);
        assert_eq!(slept, Duration::from_secs(1));
    }

    #[test]
    fn errors_are_appended_to_the_log() {
        let d = std::env::temp_dir().join(format!("ymux-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let log = d.join("daemon.log");
        log_error(&log, "first");
        log_error(&log, "second");
        let text = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("first") && lines[1].ends_with("second"));
    }

    #[test]
    fn a_user_copy_wins_then_the_package_then_the_user_path() {
        let local = std::path::Path::new("/home/u/.local/bin/ymux");
        assert_eq!(pick_exe(local, true, true), local);
        assert_eq!(pick_exe(local, false, true), std::path::Path::new(SYSTEM_EXE));
        assert_eq!(pick_exe(local, false, false), local);
    }
}
