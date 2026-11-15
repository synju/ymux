use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn placeholder_without_wayland_fails_cleanly() {
    let run = std::env::temp_dir().join(format!("ymux-ph-{}", std::process::id()));
    std::fs::create_dir_all(&run).unwrap();
    let t = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_ymux"))
        .arg("placeholder")
        .env_remove("WAYLAND_DISPLAY")
        .env("XDG_RUNTIME_DIR", &run)
        .output()
        .unwrap();
    assert!(t.elapsed() < Duration::from_secs(2));
    assert_ne!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stderr).to_lowercase().contains("wayland"));
}

#[test]
fn buffer_len_is_never_zero() {
    assert_eq!(ymux::placeholder::buffer_len(0, 0), 4);
    assert_eq!(ymux::placeholder::buffer_len(10, 2), 80);
}
