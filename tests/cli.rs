use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn client_without_daemon_fails_and_notifies() {
    let root = std::env::temp_dir().join(format!("ymux-cli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let run = root.join("run");
    let bin = root.join("bin");
    fs::create_dir_all(&run).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let log = root.join("notify.log");
    let stub = bin.join("notify-send");
    fs::write(&stub, format!("#!/bin/sh\necho \"$@\" >> {}\n", log.display())).unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ymux"))
        .args(["focus", "1"])
        .env("XDG_RUNTIME_DIR", &run)
        .env("HOME", &root)
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(fs::read_to_string(&log).unwrap().contains("daemon not running"));
}

#[test]
fn no_notify_flag_suppresses_the_notification() {
    let root = std::env::temp_dir().join(format!("ymux-cli-nn-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let run = root.join("run");
    let bin = root.join("bin");
    fs::create_dir_all(&run).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let log = root.join("notify.log");
    let stub = bin.join("notify-send");
    fs::write(&stub, format!("#!/bin/sh\necho \"$@\" >> {}\n", log.display())).unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ymux"))
        .args(["--no-notify", "focus", "1"])
        .env("XDG_RUNTIME_DIR", &run)
        .env("HOME", &root)
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(!log.exists());
    assert!(String::from_utf8_lossy(&out.stdout).contains("daemon not running"));
}

#[test]
fn a_failure_can_be_read_back_with_ymux_errors() {
    let root = std::env::temp_dir().join(format!("ymux-cli-errors-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let run = root.join("run");
    let bin = root.join("bin");
    fs::create_dir_all(&run).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let stub = bin.join("notify-send");
    fs::write(&stub, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let ymux = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ymux")).args(args).env("XDG_RUNTIME_DIR", &run).env("HOME", &root).env("PATH", &path).output().unwrap()
    };
    let none = ymux(&["errors"]);
    assert!(String::from_utf8_lossy(&none.stdout).contains("no ymux messages yet"));
    ymux(&["focus", "1"]); // no daemon: fails with a toast
    let out = String::from_utf8_lossy(&ymux(&["errors"]).stdout).to_string();
    assert!(out.contains("ymux: daemon not running"), "{out}");
}
