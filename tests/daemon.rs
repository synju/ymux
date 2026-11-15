use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

fn sandbox(name: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("ymux-daemon-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let (run, bin, home) = (root.join("run"), root.join("bin"), root.join("home"));
    for d in [&run, &bin, &home] {
        fs::create_dir_all(d).unwrap();
    }
    let log = root.join("notify.log");
    stub(&bin, "notify-send", &format!("echo \"$@\" >> {}", log.display()));
    (run, bin, home, log)
}

fn stub(bin: &Path, name: &str, body: &str) {
    let p = bin.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
}

fn hyprctl_stub(bin: &Path) {
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    stub(
        bin,
        "hyprctl",
        &format!(
            "case \"$1 $2\" in\n\"-j monitors\") cat {f}/monitors.json;;\n\"-j workspaces\") cat {f}/workspaces.json;;\n\"-j clients\") cat {f}/clients.json;;\n\"-j activewindow\") cat {f}/activewindow.json;;\n*) echo ok;;\nesac",
            f = fx.display()
        ),
    );
}

fn ymux(run: &Path, bin: &Path, home: &Path, sig: &str) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_ymux"));
    c.arg("daemon")
        .env("XDG_RUNTIME_DIR", run)
        .env("HOME", home)
        .env("HYPRLAND_INSTANCE_SIGNATURE", sig)
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()));
    c
}

#[test]
fn second_daemon_gives_up_after_lock_wait_and_notifies() {
    let (run, bin, home, log) = sandbox("lock");
    let f = fs::File::create(run.join("ymux.lock")).unwrap();
    use std::os::fd::AsRawFd;
    assert_eq!(unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
    let t = Instant::now();
    let out = ymux(&run, &bin, &home, "sig").output().unwrap();
    assert!(t.elapsed() < Duration::from_millis(6500), "{:?}", t.elapsed());
    assert!(t.elapsed() >= Duration::from_millis(4500));
    assert_eq!(out.status.code(), Some(0));
    assert!(fs::read_to_string(&log).unwrap().contains("another daemon is still running"));
}

#[test]
fn exiting_daemon_leaves_socket_file_and_saves_state() {
    let (run, bin, home, _log) = sandbox("exit");
    hyprctl_stub(&bin);
    let sig = "testsig";
    let hdir = run.join("hypr").join(sig);
    fs::create_dir_all(&hdir).unwrap();
    let listener = UnixListener::bind(hdir.join(".socket2.sock")).unwrap();
    let server = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        std::thread::sleep(Duration::from_millis(300));
        drop(s); // Hyprland went away
    });
    let out = ymux(&run, &bin, &home, sig).output().unwrap();
    server.join().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(run.join("ymux.sock").exists());
    let state = fs::read_to_string(home.join(".local/state/ymux/state.json")).unwrap();
    assert!(state.contains("\"main\""));
}

fn running_daemon(name: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf, std::process::Child, std::thread::JoinHandle<()>) {
    let (run, bin, home, log) = sandbox(name);
    hyprctl_stub(&bin);
    let sig = "testsig";
    let hdir = run.join("hypr").join(sig);
    fs::create_dir_all(&hdir).unwrap();
    let listener = UnixListener::bind(hdir.join(".socket2.sock")).unwrap();
    let server = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        std::thread::sleep(Duration::from_secs(8)); // Hyprland stays up
        drop(s);
    });
    let child = ymux(&run, &bin, &home, sig).spawn().unwrap();
    let t = Instant::now();
    while !run.join("ymux.sock").exists() && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(300));
    (run, bin, home, log, child, server)
}

#[test]
fn second_daemon_steps_aside_quietly_when_first_answers() {
    let (run, bin, home, log, mut first, server) = running_daemon("aside");
    let t = Instant::now();
    let out = ymux(&run, &bin, &home, "testsig").output().unwrap();
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    assert_eq!(out.status.code(), Some(0));
    assert!(!fs::read_to_string(&log).unwrap_or_default().contains("another daemon"));
    let _ = first.kill();
    let _ = server.join();
}

#[test]
fn shutdown_request_stops_the_daemon() {
    let (run, bin, home, _log, mut first, server) = running_daemon("shutdown");
    let out = Command::new(env!("CARGO_BIN_EXE_ymux"))
        .arg("shutdown")
        .env("XDG_RUNTIME_DIR", &run)
        .env("HOME", &home)
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let t = Instant::now();
    let mut exited = false;
    while t.elapsed() < Duration::from_secs(3) {
        if first.try_wait().unwrap().is_some() {
            exited = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(exited, "daemon still running after shutdown");
    let _ = server.join();
}
