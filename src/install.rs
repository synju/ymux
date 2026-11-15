//! `ymux install` (spec 8.1, phase 2 subset) and the uninstaller it places.

use crate::config::{config_path, default_text};
use crate::daemon::lua_path;
use crate::engine::Request;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const UNINSTALLER: &str = include_str!("../packaging/ymux-uninstall.sh");

/// Every file of the shell plugin (tests excluded), installed to
/// `~/.config/omarchy/plugins/synju.ymux/`.
pub const PLUGIN_FILES: &[(&str, &str)] = &[
    ("manifest.json", include_str!("../plugin/synju.ymux/manifest.json")),
    ("Service.qml", include_str!("../plugin/synju.ymux/Service.qml")),
    ("BarWidget.qml", include_str!("../plugin/synju.ymux/BarWidget.qml")),
    ("Ymux.js", include_str!("../plugin/synju.ymux/Ymux.js")),
    ("Menu.qml", include_str!("../plugin/synju.ymux/Menu.qml")),
    ("views/Prompt.qml", include_str!("../plugin/synju.ymux/views/Prompt.qml")),
    ("views/Tree.qml", include_str!("../plugin/synju.ymux/views/Tree.qml")),
    ("views/Options.qml", include_str!("../plugin/synju.ymux/views/Options.qml")),
    ("views/Panes.qml", include_str!("../plugin/synju.ymux/views/Panes.qml")),
];
const PLUGIN_ID: &str = "synju.ymux";
const OMARCHY_WORKSPACES: &str = "omarchy.workspaces";

/// (section, index) of a widget in `bar.layout`, sections in file order.
pub fn find_bar_widget(shell_json: &str, id: &str) -> Option<(String, usize)> {
    let v: serde_json::Value = serde_json::from_str(shell_json).ok()?;
    let layout = v.get("bar")?.get("layout")?.as_object()?;
    for (section, items) in layout {
        if let Some(i) = items.as_array()?.iter().position(|w| w.get("id").and_then(|x| x.as_str()) == Some(id)) {
            return Some((section.clone(), i));
        }
    }
    None
}

/// The spec 8.1 fallback: drop `from` from every section and, if `to` is not on the
/// bar, put `{ "id": to }` at `index` of `left` (clamped). Key order is kept.
pub fn replace_bar_widget(shell_json: &str, from: &str, to: &str, index: usize) -> Result<String> {
    let mut v: serde_json::Value = serde_json::from_str(shell_json).context("parsing shell.json")?;
    let layout = v
        .get_mut("bar")
        .and_then(|b| b.get_mut("layout"))
        .and_then(|l| l.as_object_mut())
        .context("shell.json has no bar.layout")?;
    for items in layout.values_mut() {
        if let Some(a) = items.as_array_mut() {
            a.retain(|w| w.get("id").and_then(|x| x.as_str()) != Some(from));
        }
    }
    let present = layout
        .values()
        .any(|items| items.as_array().is_some_and(|a| a.iter().any(|w| w.get("id").and_then(|x| x.as_str()) == Some(to))));
    if !present {
        let left = layout.entry("left").or_insert_with(|| serde_json::json!([]));
        if let Some(a) = left.as_array_mut() {
            let i = index.min(a.len());
            a.insert(i, serde_json::json!({ "id": to }));
        }
    }
    Ok(serde_json::to_string_pretty(&v)? + "\n")
}

fn quiet(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd).args(args).output().map(|o| o.status.success()).unwrap_or(false)
}

/// Rescanning is asynchronous (docs/spikes.md phase 3): wait until the shell knows
/// the plugin before enabling it.
fn wait_until_known(id: &str) -> bool {
    for _ in 0..30 {
        if let Ok(o) = Command::new("omarchy").args(["plugin", "list"]).output()
            && String::from_utf8_lossy(&o.stdout).lines().any(|l| l.split_whitespace().next() == Some(id))
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    false
}

/// True when the installed plugin files are missing or differ from this build's.
pub fn plugin_differs(dir: &std::path::Path) -> bool {
    PLUGIN_FILES.iter().any(|(rel, text)| fs::read_to_string(dir.join(rel)).ok().as_deref() != Some(*text))
}

fn install_plugin() -> Result<()> {
    let dir = home().join(".config/omarchy/plugins").join(PLUGIN_ID);
    let was_installed = dir.join("manifest.json").exists();
    let changed = plugin_differs(&dir);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    for (rel, text) in PLUGIN_FILES {
        crate::store::write_atomic(&dir.join(rel), text)?;
    }
    println!("installed plugin {}", dir.display());

    let shell = home().join(".config/omarchy/shell.json");
    let text = fs::read_to_string(&shell).unwrap_or_default();
    if let Some((section, index)) = find_bar_widget(&text, OMARCHY_WORKSPACES) {
        crate::store::write_atomic(&home().join(".local/state/ymux/bar-place"), &format!("{section} {index}\n"))?;
    }
    let ok = quiet("omarchy-shell", &["shell", "rescanPlugins"])
        && wait_until_known(PLUGIN_ID)
        && (find_bar_widget(&text, OMARCHY_WORKSPACES).is_none() || quiet("omarchy", &["plugin", "disable", OMARCHY_WORKSPACES]))
        && quiet("omarchy", &["plugin", "enable", PLUGIN_ID, "--section", "left", "--index", "1"]);
    // the shell caches `.pragma library` JS across plugin reloads (phase 3
    // acceptance), so an updated plugin only takes effect after a shell restart
    if was_installed && changed {
        let _ = Command::new("omarchy-restart-shell").status();
        println!("restarted the Omarchy shell to load the updated plugin");
    }
    if ok {
        println!("bar: {OMARCHY_WORKSPACES} replaced by {PLUGIN_ID}");
    } else {
        let current = fs::read_to_string(&shell).unwrap_or(text);
        crate::store::write_atomic(&shell, &replace_bar_widget(&current, OMARCHY_WORKSPACES, PLUGIN_ID, 1)?)?;
        println!("bar: omarchy plugin commands failed; edited {} directly", shell.display());
    }
    Ok(())
}
const BINDINGS: &str = "require(\"hypr.bindings\")";
const YMUX: &str = "require(\"hypr.ymux\")";

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

/// Inserts `require("hypr.ymux")` right after `require("hypr.bindings")`, once.
pub fn add_require_line(text: &str) -> Result<String> {
    if text.lines().any(|l| l.trim() == YMUX) {
        return Ok(text.to_string());
    }
    if !text.lines().any(|l| l.trim() == BINDINGS) {
        bail!("no `{BINDINGS}` line in hyprland.lua to put ymux after");
    }
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        out.push_str(line);
        if line.trim() == BINDINGS {
            if !line.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(YMUX);
            out.push('\n');
        }
    }
    Ok(out)
}

pub fn remove_require_line(text: &str) -> String {
    text.split_inclusive('\n').filter(|l| l.trim() != YMUX).collect()
}

/// `ymux uninstall`: runs the embedded uninstaller (spec 8.2).
pub fn uninstall() -> Result<()> {
    let script = std::env::temp_dir().join(format!("ymux-uninstall-{}.sh", std::process::id()));
    fs::write(&script, UNINSTALLER)?;
    let status = Command::new("bash").arg(&script).status().context("running the uninstaller")?;
    let _ = fs::remove_file(&script);
    if !status.success() {
        anyhow::bail!("the uninstaller failed");
    }
    Ok(())
}

pub fn install() -> Result<()> {
    // a daemon from an earlier build must not keep running old code (review finding 5)
    if crate::ipc::send(&Request::Shutdown).is_ok() {
        println!("stopped the running ymux daemon");
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    // the package's binary is used where it is; any other build (e.g. from source)
    // is copied to ~/.local/bin, which then takes precedence
    let me = std::env::current_exe()?;
    let bin = if me == Path::new(crate::daemon::SYSTEM_EXE) {
        let local = crate::daemon::user_exe();
        if local.exists() {
            fs::remove_file(&local).context("removing an older ~/.local/bin/ymux")?;
            println!("removed an older {}", local.display());
        }
        me
    } else {
        let bin = crate::daemon::user_exe();
        if me != bin {
            fs::create_dir_all(bin.parent().expect("has parent"))?;
            let tmp = bin.with_extension("new");
            fs::copy(&me, &tmp).context("copying the ymux binary")?;
            fs::rename(&tmp, &bin)?;
        }
        bin
    };
    println!("using {}", bin.display());

    let conf = config_path();
    if conf.exists() {
        println!("using your existing {}", conf.display());
    } else {
        fs::write(&conf, default_text())?;
        println!("wrote default {}", conf.display());
    }
    let (cfg, errors) = crate::config::parse(&fs::read_to_string(&conf)?);
    for (n, m) in errors {
        println!("~/.ymux.conf line {n} skipped: {m}");
    }

    // ymux.lua first, so the reload that the hyprland.lua edit triggers finds it
    crate::store::write_atomic(&lua_path(), &crate::luagen::generate(&cfg, &bin.display().to_string()))?;
    println!("wrote {}", lua_path().display());

    let hypr = home().join(".config/hypr/hyprland.lua");
    let text = fs::read_to_string(&hypr).with_context(|| format!("reading {}", hypr.display()))?;
    let patched = add_require_line(&text)?;
    if patched != text {
        fs::write(&hypr, patched)?;
        println!("added require(\"hypr.ymux\") to {}", hypr.display());
    }

    install_plugin()?;

    // Hyprland reloads on the file change (spikes.md S3); reload explicitly as well so
    // the switch-off of Omarchy's renumbering is in place before the daemon adopts.
    let _ = Command::new("hyprctl").arg("reload").status();
    println!("done: ymux.lua switches off Omarchy's renumbering and starts the daemon");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "require(\"hypr.monitors\")\nrequire(\"hypr.bindings\")\nrequire(\"hypr.looknfeel\")\n";

    #[test]
    fn require_inserted_after_bindings_once() {
        let once = add_require_line(T).unwrap();
        assert_eq!(once, "require(\"hypr.monitors\")\nrequire(\"hypr.bindings\")\nrequire(\"hypr.ymux\")\nrequire(\"hypr.looknfeel\")\n");
        assert_eq!(add_require_line(&once).unwrap(), once);
        assert_eq!(remove_require_line(&once), T);
    }

    #[test]
    fn missing_bindings_line_is_an_error() {
        assert!(add_require_line("require(\"hypr.monitors\")\n").is_err());
    }

    const SHELL: &str = r#"{"bar":{"layout":{"left":[{"id":"omarchy.menu"},{"id":"omarchy.workspaces"}],"right":[]}},"version":1}"#;

    #[test]
    fn finds_workspaces_widget_place() {
        assert_eq!(find_bar_widget(SHELL, "omarchy.workspaces"), Some(("left".into(), 1)));
        assert_eq!(find_bar_widget(SHELL, "nope"), None);
    }

    #[test]
    fn fallback_swaps_widget_in_place_and_keeps_key_order() {
        let out = replace_bar_widget(SHELL, "omarchy.workspaces", "synju.ymux", 1).unwrap();
        assert_eq!(find_bar_widget(&out, "synju.ymux"), Some(("left".into(), 1)));
        assert_eq!(find_bar_widget(&out, "omarchy.workspaces"), None);
        assert!(out.find("\"bar\"").unwrap() < out.find("\"version\"").unwrap());
    }

    #[test]
    fn fallback_is_idempotent() {
        let once = replace_bar_widget(SHELL, "omarchy.workspaces", "synju.ymux", 1).unwrap();
        let twice = replace_bar_widget(&once, "omarchy.workspaces", "synju.ymux", 1).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn plugin_change_detection() {
        let d = std::env::temp_dir().join(format!("ymux-pd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert!(plugin_differs(&d));
        std::fs::create_dir_all(&d).unwrap();
        for (rel, text) in PLUGIN_FILES {
            std::fs::create_dir_all(d.join(rel).parent().unwrap()).unwrap();
            std::fs::write(d.join(rel), text).unwrap();
        }
        assert!(!plugin_differs(&d));
        std::fs::write(d.join("Ymux.js"), "old").unwrap();
        assert!(plugin_differs(&d));
    }

    #[test]
    fn plugin_files_cover_the_plugin_dir() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plugin/synju.ymux");
        let mut on_disk: Vec<String> = Vec::new();
        fn walk(base: &std::path::Path, d: &std::path::Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                let p = e.path();
                let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
                if rel == "tests" { continue; }
                if p.is_dir() { walk(base, &p, out) } else { out.push(rel) }
            }
        }
        walk(&dir, &dir, &mut on_disk);
        on_disk.sort();
        let mut listed: Vec<String> = PLUGIN_FILES.iter().map(|(p, _)| p.to_string()).collect();
        listed.sort();
        assert_eq!(on_disk, listed);
    }

    #[test]
    fn uninstaller_closes_placeholders_after_stopping_daemon() {
        let stop = UNINSTALLER.find("pkill -x ymux").unwrap();
        let close = UNINSTALLER.find("select(.class == \"ymux-placeholder\")").unwrap();
        assert!(stop < close);
    }

    #[test]
    fn uninstaller_points_package_users_at_pacman() {
        assert!(UNINSTALLER.contains("pacman -R ymux"));
        assert!(!UNINSTALLER.contains("AppUninstallers"));
    }

    #[test]
    fn uninstaller_restores_workspaces_widget() {
        assert!(UNINSTALLER.contains("omarchy plugin enable omarchy.workspaces"));
        let read = UNINSTALLER.find("bar-place").unwrap();
        let wipe = UNINSTALLER.find("rm -rf \"$HOME/.local/state/ymux\"").unwrap();
        assert!(read < wipe);
        assert!(UNINSTALLER.contains("rm -rf \"$HOME/.config/omarchy/plugins/synju.ymux\""));
    }

    #[test]
    fn uninstaller_keeps_config_and_parses() {
        assert!(UNINSTALLER.contains(".ymux.conf"));
        assert!(!UNINSTALLER.contains("rm -f \"$HOME/.ymux.conf\""));
        let out = std::process::Command::new("bash").args(["-n", "-c", UNINSTALLER]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
}
