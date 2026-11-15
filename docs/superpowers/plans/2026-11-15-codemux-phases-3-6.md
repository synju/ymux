# codemux Implementation Plan (phases 3-6: bar, overlays, tiles, relaunch)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build everything codemux still lacks, straight through in one plan: the
tmux-style bar widget with its plugin service (phase 3), every overlay (phase 4),
placeholders, mark / swap / kill, resize mode and the Alt + Tab takeover (phase 5),
and relaunch with layout restore (phase 6). Each phase ends usable on the real
desktop with a short live acceptance by the user, but no new plan is written between
phases.

**Architecture:** The Rust daemon stays the single owner of state and logic. It
publishes a derived **view** (full state plus per-monitor bar rows, the tree, the
leader/resize mode and the mark) on the existing `codemux watch` stream. The
`synju.codemux` Omarchy shell plugin is thin: one `service` holds the watch
connection, one `barWidget` per monitor renders that monitor's row, and one `menu`
root renders every overlay from a summon payload the daemon sends with
`omarchy-shell shell summon synju.codemux '<json>'`. Overlay choices call the CLI.
Placeholders are a `codemux placeholder` subcommand (a transparent Wayland toplevel,
`smithay-client-toolkit`). Asynchronous work (placeholder jobs, kill checks, the
relaunch queue) is a set of engine state machines driven by socket2 events and
deadlines the daemon loop waits on. New engine code goes into new files
(`src/view.rs`, `src/overlay.rs`, `src/tiles.rs`, `src/geometry.rs`,
`src/relaunch.rs`) as extra `impl<H: Hypr> Engine<H>` blocks, so `src/engine.rs`
(889 lines today) does not grow into a monolith.

**Tech Stack:** Rust 1.99.1 (edition 2024), std threads and channels (no async),
`anyhow`, `serde`, `serde_json` (feature `preserve_order` added in Task 5),
`libc`, `smithay-client-toolkit` (Task 14, no default features). QML on Quickshell
inside omarchy-shell (Omarchy 4.5.0), with plain JS logic in a `.pragma library`
file tested by `/usr/lib/qt6/bin/qmltestrunner`. Lua (Hyprland 0.56.2 config API).
Bash for the uninstaller.

**Spec:** `docs/spec.md` (draft 13, including the acceptance decisions: plain-only
leader keys, H / L and Super + Tab wrap, Super + Shift + N creates a missing
workspace, new workspaces start with a placeholder). Facts from `docs/spikes.md`.
Section numbers below (`spec 5.7`) point into the spec.

**Builds on (phases 1-2, done and installed):** `Engine`, `Request` / `Response`,
`ipc::Msg`, the `Hypr` trait with `Hyprctl` and `tests/common::FakeHypr`,
`luagen::generate(cfg, exe)`, `config::Config`, `store`, the watch stream in
`src/daemon.rs`, `install::install`, `packaging/codemux-uninstall.sh`. 114 tests
pass today. Do not rebuild any of it; tasks below name every existing function they
change.

**Carry-overs folded in** (from `.superpowers/sdd/2026-11-14-codemux-core/progress.md`):
watcher write timeout, atomic `codemux.lua` write, `adopt_unknown` duplicate `main`,
`workspacev2` attribution race (Task 1); Lua escaping of config values, leader keys
such as `period`, the misleading empty-leader message (Task 15); the "30 s tick
re-syncs tile workspaces" proposal (Task 13).

## Rulings on spec gaps

Each is decided here so executors do not stop on it; the user can overturn any.

1. **Watch stream content:** `codemux watch` emits the derived `View` (Task 3), not
   the raw `State`. It still carries everything the plugin needs and is sent in full
   on connect and on every change (spec 3.1).
2. **Overlay routing:** every leader action, overlays included, still calls
   `codemux action <name>`; the daemon answers an overlay action by summoning the
   plugin with a payload (prefills, pane positions). Only the daemon knows the
   focused tile and live geometry.
3. **Markers:** `LEADER`, `RESIZE` and `MARK` show on every monitor's bar (the submap
   and the mark are global).
4. **Workspace label:** a user-set name always shows (`name number`). The automatic
   name (class of the app last focused on it; falling back to the class of its first
   app tile) shows only while the workspace holds an app tile; otherwise the label is
   the number only (spec 4.1 "holding only placeholders shows as empty").
5. **Reading order** ("left to right, top to bottom", spec 5.5; "top-left first",
   spec 2 takeover 2; "saved order", spec 7.3): sort by `y`, then `x`, of the window's
   top-left corner. One helper, `model::reading_key`, used everywhere.
6. **Tree:** starts fully expanded; Left / `h` collapses the selected node, Right /
   `l` expands it. Enter on a monitor focuses that monitor; on a session switches to
   it; on a workspace focuses it; on a tile focuses it.
7. **Killing the active session:** codemux closes its windows; once they are all
   gone it switches the monitor to its most recently used other session (or a new
   `main`), which leaves the killed session's shown workspace empty, so the normal
   rules delete it (spec 5.6). Killing a shown workspace does not switch away; it is
   deleted when left. Workspaces of a killed session or a killed workspace that hold
   no windows and are not shown are deleted at once (no event would ever come).
8. **Command prompt:** empty input closes the prompt with no message.
9. **Leader + `v` with no usable mark** (none, gone, or the focused tile itself):
   nothing moves, notification `codemux: no marked tile`. A swap clears the mark.
10. **Resize mode exit:** besides releasing Super, Escape and any other key
    (catch-all) leave resize mode.
11. **Alt + Tab order:** windows of the focused workspace in `hyprctl -j clients`
    order (Hyprland's window list, the list `cycle_next` walks and `bring_to_top`
    reorders, `tiling.lua:55-56`), placeholders and hidden windows skipped.
12. **Placeholder identity:** a window is a placeholder iff its class is
    `codemux-placeholder` (survives daemon restarts; pids do not). Tile records keep
    the class, which is the "is a placeholder" record of spec 7.2.
13. **Orphans:** at every daemon start, placeholder windows left on
    `special:codemux-stash` (a job interrupted by a daemon exit) are closed.
14. **Recreating archives** also runs on `changefloatingmode ...,0` for a window on a
    workspace with archives (faster than the 30 s check), and focus is given back to
    the window focused before the recreation. Archives are kept when relaunch is off
    (they are state, not relaunch).
15. **Fill-on-open's "focused placeholder"** is the window focused just before the
    new window took focus: the engine keeps the current and previous active window,
    because `activewindowv2` may arrive before or after `openwindow` (order recorded
    in Task 2).
16. **Plugin service** runs `$HOME/.local/bin/codemux watch` by absolute path (the
    shell's `PATH` is not guaranteed to hold `~/.local/bin`).
17. **Old bar place:** install records where `omarchy.workspaces` sat in
    `~/.local/state/codemux/bar-place` (`<section> <index>`); the uninstaller reads
    it before deleting state (default `left 1`).
18. **Relaunch spawning:** a saved `/proc/<pid>/cmdline` is executed directly (argv,
    no shell); a `launch-override` string runs through `sh -c`. No `uwsm-app`
    wrapper, since the daemon needs the app's own pid (spec 7.3). A relaunched window
    is moved with `follow = false`; if Hyprland creates its workspace on the wrong
    monitor, the existing invariant moves it back.
19. **"Logs wrapped apps"** (spec 7.4): when a relaunched app's window does not
    appear within 15 s, codemux logs and notifies
    `codemux: <class> did not come back; a launch-override line may help`.

## Global Constraints

- No em dash character anywhere: code, comments, QML, docs, commit messages.
- Never edit files under `/usr/share/omarchy/`. Omarchy behaviour is changed only
  from `~/.config/hypr/codemux.lua` and the plugin.
- Paths (unchanged from phase 2): binary `~/.local/bin/codemux`, socket
  `$XDG_RUNTIME_DIR/codemux.sock`, state `~/.local/state/codemux/state.json`, config
  `~/.codemux.conf`, generated `~/.config/hypr/codemux.lua`, uninstaller
  `~/Scripts/AppUninstallers/codemux-uninstall.sh`. New: plugin source
  `plugin/synju.codemux/` in the repo, installed to
  `~/.config/omarchy/plugins/synju.codemux/`; bar-place record
  `~/.local/state/codemux/bar-place`.
- Names: plugin id `synju.codemux`; placeholder class (Wayland app id)
  `codemux-placeholder`; stash `special:codemux-stash`; submaps `codemux` and
  `codemux_resize`; summon form
  `omarchy-shell shell summon synju.codemux '<json>'`.
- Real workspace ids, session numbering, leader defaults: as in the phase 1-2 plan.
- Every file written while Hyprland or the shell watches it (`codemux.lua`,
  `state.json`, `~/.codemux.conf`, plugin files) is written atomically (temp file in
  the same directory, then rename).
- Engine side effects (dispatch, eval, spawn, notify, `/proc` reads) go only through
  the `Hypr` trait, so every engine behaviour is tested against `FakeHypr`.
- Rust checks after every task: `cargo clippy --all-targets -- -D warnings` and
  `cargo test` pass.
- QML checks after every task touching `plugin/`: every `.qml` file parses
  (`/usr/lib/qt6/bin/qmlformat <file> >/dev/null` exits 0) and
  `/usr/lib/qt6/bin/qmltestrunner -input plugin/synju.codemux/tests` passes.
- Live-desktop steps (spikes, acceptance) are reversible and say how to revert. A
  spike records its results in `docs/spikes.md` under its own heading.
- Commits: the folder is still not a git repository. "Commit" steps stage nothing
  until the user approves `git init` and committing (global rule); until then they
  record the task in the ledger only.

## Review Focus

1. A placeholder job never lets codemux delete the workspace an app is leaving or
   entering half-way through a swap (the `busy` guard). Tests in Tasks 16-17.
2. Fill-on-open fires only for a tiled, user-opened window on the focused
   placeholder's workspace: never for spawned pids, dialogs, floating windows or
   during a restore (spec 5.7). Test in Task 17.
3. Archiving reads the last recorded tiled geometry, and a geometry capture never
   overwrites a workspace whose recorded mode differs from its live mode; the 30 s
   order is mode check, archive, then capture (spec 6.2). Tests in Tasks 13, 19.
4. Relaunch claims an existing window before launching, opens one tile at a time,
   gives up after 15 s, and records leftover windows only after the restore
   (spec 3.2.8, 7.3). Tests in Tasks 23-24.
5. A stalled or dead watcher can never block the daemon loop; the bar shows a dim
   `codemux` while the daemon is down and recovers within 2 s of it returning
   (spec 3.1). Tests in Tasks 1, 4.
6. Install and uninstall leave the bar sane both ways: `omarchy.workspaces` back in
   its old place, no stray placeholder windows, the window-switcher filter removed
   (spec 8). Tests in Tasks 5, 20.

---

## File structure (new and changed)

```
src/view.rs              View model for the watch stream (pure)            [Task 3]
src/overlay.rs           Summon payloads, pane numbering; Engine::summon   [Task 8]
src/tiles.rs             Placeholder jobs, kill-tile, move/fill, mark,
                         swap, Alt+Tab cycle (impl Engine)                 [Tasks 16-18]
src/geometry.rs          Mode tracking, geometry capture, archives
                         (impl Engine)                                     [Tasks 13, 19]
src/placeholder.rs       `codemux placeholder` Wayland client              [Task 14]
src/relaunch.rs          Restore planner (pure) + restore machine
                         (impl Engine)                                     [Tasks 22-24]
src/engine.rs            fields become pub(crate); new Request variants
src/model.rs             Workspace.auto_name/mode/archived, State.mark,
                         Tile cmdline/cwd, reading_key, PLACEHOLDER_CLASS
src/hypr.rs              spawn/notify/proc_info, new Lua builders
src/events.rs            ActiveWindow event
src/daemon.rs            view broadcast, write timeouts, deadlines, capture
src/store.rs             write_atomic
src/install.rs           plugin files, bar placement, switcher filter
src/luagen.rs            resize mode, Alt+Tab, window rules, escaping
src/config.rs            key validation, empty leader message
plugin/synju.codemux/manifest.json
plugin/synju.codemux/Service.qml
plugin/synju.codemux/BarWidget.qml
plugin/synju.codemux/Menu.qml
plugin/synju.codemux/Codemux.js
plugin/synju.codemux/views/{Tree,Prompt,Options,Confirm,Panes}.qml
plugin/synju.codemux/tests/tst_codemux.qml
tests/engine_view.rs, tests/engine_overlays.rs, tests/engine_geometry.rs,
tests/engine_tiles.rs, tests/engine_relaunch.rs, tests/placeholder.rs
```

---

## Phase 3: Bar widget (with the plugin service)

### Task 1: Carry-over fixes before the stream gets a real consumer

**Files:**
- Modify: `src/store.rs`, `src/daemon.rs`, `src/install.rs`, `src/engine.rs`
- Test: `src/store.rs`, `src/daemon.rs`, `tests/engine_events.rs`

**Interfaces:**
- Produces:
  - `store::write_atomic(path: &Path, text: &str) -> Result<()>` (temp file
    `.<name>.tmp` in the same directory, fsync, rename). `save_atomic` calls it.
  - `daemon::register_watcher(stream: UnixStream) -> UnixStream` sets a 200 ms write
    timeout; `send_state` (existing, `src/daemon.rs`) drops a watcher whose write
    errors or times out.
- Changes:
  - `daemon::run` `Msg::ConfigChanged` branch and `install::install` write
    `codemux.lua` with `write_atomic` instead of `fs::write`.
  - `Engine::adopt_unknown` (`src/engine.rs:367`): when the monitor has sessions but
    `active_session` is `None`, use `most_recent_session_on` (and activate it); create
    `main` only when the monitor has no session at all.
  - `Engine::on_shown` (`src/engine.rs:520`): the monitor that now shows `id` is the
    monitor the live workspace `id` is on (`hypr.workspaces()`), falling back to the
    focused monitor only if `id` is not live. Fixes attribution during session-move
    and landing bursts.

- [ ] **Step 1: Write failing tests**

```rust
// src/store.rs
#[test] fn write_atomic_replaces_file_and_leaves_no_tmp() {
    /* write "a" then "b" to d/codemux.lua; read == "b"; no file named .codemux.lua.tmp */ }
// src/daemon.rs
#[test] fn stalled_watcher_is_dropped_within_a_second() {
    let (a, _b_never_reads) = UnixStream::pair().unwrap();
    let mut w = register_watcher(a);
    let t = Instant::now();
    let big = "x".repeat(8 << 20);
    assert!(!send_state(&mut w, &big));
    assert!(t.elapsed() < Duration::from_secs(1));
}
// tests/engine_events.rs
#[test] fn adopting_while_active_session_unset_reuses_existing_session() {
    /* two(): LG A active_session = None; OpenWindow on unknown id 5 (DP-1):
       no session named "main" created; the new workspace joins session 1 */ }
#[test] fn shown_event_attributed_to_the_workspaces_own_monitor() {
    /* two(): DP-1 focused; Workspace{200001} (live on DP-2):
       Dell B .shown == Some(200001); LG A .shown unchanged */ }
```

- [ ] **Step 2: Run** `cargo test write_atomic stalled_watcher adopting_while shown_event_attributed`.
  Expected: FAIL (missing functions / wrong attribution).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings`.
  Expected: PASS, 118 tests.
- [ ] **Step 5: Commit** (`fix: atomic lua write, watcher timeout, adoption and focus attribution`).

### Task 2: Focus, submap and automatic workspace names

**Files:**
- Modify: `src/events.rs`, `src/model.rs`, `src/engine.rs`, `src/main.rs`
- Test: `src/events.rs`, `tests/engine_view.rs` (new)

**Interfaces:**
- Consumes: `Event::Submap` (exists), `Engine::on_event`.
- Produces:
  - `Event::ActiveWindow { address: Option<String> }` from `activewindowv2>><addr>`
    (empty data -> `None`; address normalised with `normalise_address`).
  - `model::PLACEHOLDER_CLASS: &str = "codemux-placeholder"`,
    `impl Tile { pub fn is_placeholder(&self) -> bool }`,
    `pub fn reading_key(at: (i32, i32)) -> (i32, i32)` (returns `(y, x)`).
  - `Workspace.auto_name: Option<String>` (`#[serde(default)]`); `Workspace` derives
    `Default`, and the three literal sites (`src/model.rs:171`,
    `src/engine.rs:386`, `src/engine.rs:826`) use `..Default::default()`.
  - Engine fields: `pub(crate) submap: String`, `pub(crate) focused: Option<String>`,
    `pub(crate) prev_focused: Option<String>`, `view_changed: bool`, and
    `pub fn take_view_changed(&mut self) -> bool`. All existing private fields of
    `Engine` become `pub(crate)` (later tasks add `impl` blocks in other files).
  - `Request::FocusId { id: i64 }` (`{"cmd":"focus-id","id":100003}`), CLI
    `codemux focus-id ID`: known id: if its session's monitor is not focused,
    dispatch `focus_monitor(connector)` first, then `focus_workspace(id)`; unknown
    id: nothing.

Behaviour:
- `Submap{name}`: store it, set `view_changed`.
- `ActiveWindow{address}`: shift `focused` into `prev_focused`, store the new one,
  set `view_changed`. If the window is a recorded non-placeholder tile, set its
  workspace's `auto_name` to its class (dirty only if it changed). A focused
  placeholder never changes `auto_name` (spec 1 item 3).

- [ ] **Step 1: Micro-spike (live, read-only).** Run
  `socat -u UNIX-CONNECT:$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock - | grep -E '^(openwindow|activewindowv2)'`,
  open a terminal, focus another window, close the terminal.
  Record in `docs/spikes.md` ("Phase 3 spike: active window"): the exact
  `activewindowv2` line (with and without a focused window) and whether
  `openwindow` comes before or after the new window's `activewindowv2`. Nothing to
  revert.
- [ ] **Step 2: Write failing tests**

```rust
// src/events.rs (lines as recorded in Step 1)
#[test] fn parses_activewindow_v2() {
    assert_eq!(parse_line("activewindowv2>>560362726330"), Event::ActiveWindow { address: Some("0x560362726330".into()) });
    assert_eq!(parse_line("activewindowv2>>"), Event::ActiveWindow { address: None });
}
// tests/engine_view.rs (fixture: one monitor, session 1 with 100001 holding foot 0xa1)
#[test] fn focused_app_names_its_workspace() {
    /* ActiveWindow{0xa1} -> workspaces[100001].auto_name == Some("foot"), take_dirty() */ }
#[test] fn focused_placeholder_keeps_the_name() {
    /* add tile 0xp1 class codemux-placeholder; ActiveWindow{0xp1} -> auto_name unchanged */ }
#[test] fn submap_event_changes_view_not_state() {
    /* Submap{"codemux"} -> take_view_changed() true, take_dirty() false */ }
#[test] fn focus_id_on_other_monitor_focuses_monitor_first() {
    /* two monitors; FocusId{200001} -> dispatched == [focus_monitor("DP-2"), focus_workspace(200001)] */ }
#[test] fn focus_id_unknown_does_nothing() { /* FocusId{7} -> dispatched empty */ }
#[test] fn old_state_without_auto_name_loads() {
    /* serde_json::from_str::<State>(phase-2 shaped json) is Ok */ }
```

- [ ] **Step 3: Run** `cargo test --test engine_view && cargo test events`. Expected: FAIL.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 6: Commit** (`feat: track focus, submap and automatic workspace names`).

### Task 3: View model on the watch stream

**Files:**
- Create: `src/view.rs`
- Modify: `src/lib.rs`, `src/engine.rs` (`pub fn view`), `src/daemon.rs`
- Test: `src/view.rs`, `tests/engine_view.rs`

**Interfaces:**
- Consumes: `State`, `Tile::is_placeholder`, `reading_key`, engine `submap`,
  `focused`, `config.relaunch`.
- Produces (all `Serialize, Debug, PartialEq`, field names as written, snake_case JSON):
  - `pub struct View { pub mode: String /* "", "LEADER", "RESIZE" */, pub marked: bool, pub relaunch: bool, pub monitors: Vec<MonitorView>, pub tree: Vec<TreeMonitor> }`
  - `pub struct MonitorView { pub connector: String, pub description: String, pub focused: bool, pub session: Option<u32>, pub session_label: String, pub workspaces: Vec<WsView> }`
  - `pub struct WsView { pub id: i64, pub number: u32, pub label: String, pub current: bool, pub empty: bool }`
  - `pub struct TreeMonitor { pub connector: String, pub description: String, pub focused: bool, pub sessions: Vec<TreeSession> }`
  - `pub struct TreeSession { pub number: u32, pub label: String, pub active: bool, pub workspaces: Vec<TreeWorkspace> }`
  - `pub struct TreeWorkspace { pub id: i64, pub number: u32, pub label: String, pub shown: bool, pub tiles: Vec<TreeTile> }`
  - `pub struct TreeTile { pub address: String, pub class: String, pub title: String, pub focused: bool }`
  - `pub struct Live { pub connected: Vec<(String, String)> /* (connector, description) */, pub focused_connector: Option<String>, pub focused_address: Option<String>, pub submap: String, pub relaunch: bool }`
  - `pub fn build(state: &State, live: &Live) -> View`
  - `pub fn session_label(state: &State, number: u32) -> String` (`name`, or
    `name (number)` when any other session has the same name, spec 4.1)
  - `pub fn workspace_label(ws: &Workspace) -> String` (Ruling 4)
  - Engine: `pub fn view(&self) -> Result<View>` (one `hypr.monitors()` call).
- Changes in `src/daemon.rs`: the `Msg::Watch` branch and the broadcast after each
  message send `serde_json::to_string(&engine.view()?)`; broadcast when
  `take_dirty()` or `take_view_changed()` is true (save state only on dirty).

Rules: only connected monitors get a `MonitorView` (a stale connector of an
unplugged monitor never collides); workspaces in number order; `current` = the
monitor's `shown`; `empty` = no non-placeholder tile; tree tiles exclude
placeholders and are in `reading_key` order; `mode` maps `codemux` -> `LEADER`,
`codemux_resize` -> `RESIZE`, anything else -> `""`; `marked` = `state.mark` is set
(always false until Task 18 adds `State.mark`; add the field now with
`#[serde(default)]` so the view is final).

- [ ] **Step 1: Write failing tests**

```rust
// src/view.rs
#[test] fn bar_row_matches_tmux_example() {
    /* session "SC Neural Nets", ws1 named "Net Extraction" with an app tile,
       ws2 unnamed with a zsh tile (auto_name "zsh"), ws2 shown:
       session_label == "SC Neural Nets"; labels == ["Net Extraction 1", "zsh 2"];
       current flags == [false, true] */ }
#[test] fn clashing_session_names_show_numbers() {
    /* sessions 1 "main" (LG A) and 3 "main" (Dell B): session_label(3) == "3: main" */ }
#[test] fn placeholder_only_workspace_shows_number_only() {
    /* ws 4 with one codemux-placeholder tile and auto_name Some("foot"): label == "4", empty == true */ }
#[test] fn renamed_empty_workspace_keeps_its_name() { /* name "notes", no tiles: label == "notes 2" */ }
#[test] fn only_connected_monitors_get_rows() {
    /* state has monitors "LG A"(DP-1) and "Old"(DP-1, unplugged); live.connected = [("DP-1","LG A")]: monitors.len() == 1 */ }
#[test] fn tree_hides_placeholders_and_orders_tiles() {
    /* tiles at (900,0),(0,0),(0,500) plus a placeholder: tree tile order by (y, x): (0,0),(900,0),(0,500); no placeholder */ }
#[test] fn mode_marker_names() { /* submap "codemux" -> "LEADER"; "codemux_resize" -> "RESIZE"; "" -> "" */ }
// tests/engine_view.rs
#[test] fn view_json_has_monitor_rows() {
    /* engine fixture; serde_json::to_value(e.view().unwrap())["monitors"][0]["connector"] == "DP-1" */ }
```

- [ ] **Step 2: Run** `cargo test view`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: view model on the watch stream`).

### Task 4: Plugin service and bar widget

**Files:**
- Create: `plugin/synju.codemux/manifest.json`, `Service.qml`, `BarWidget.qml`,
  `Codemux.js`, `tests/tst_codemux.qml`
- Test: `plugin/synju.codemux/tests/tst_codemux.qml`

**Interfaces:**
- Consumes: the `View` JSON (Task 3); host injection as read from
  `/usr/share/omarchy/shell/shell.qml`: a service gets `shell` (scoped
  `PluginShellApi`, `serviceFor`, `summon`, `hide`) and `manifest`
  (`shell.qml:955-960`); a bar widget gets `bar`, whose `bar.shell.serviceFor("synju.codemux")`
  returns the plugin's own service (`Bar.qml:349-366`, the pattern
  `rosakodu.dock/BarWidget.qml:984-985` uses).
- Produces:
  - `manifest.json`: `schemaVersion 1`, `id "synju.codemux"`, `name "codemux"`,
    `kinds ["service", "bar-widget"]` (Task 10 adds `"menu"`), `keepLoaded true`,
    `entryPoints { "service": "Service.qml", "barWidget": "BarWidget.qml" }`,
    `barWidget { displayName "codemux", description "tmux-style sessions and workspaces", category "Compositor", allowMultiple false, defaultSection "left" }`.
  - `Service.qml` (root `Item`): `property var shell`, `property var manifest`,
    `property bool connected`, `property var view` (`null` until the first line),
    `readonly property string exe` (`Quickshell.env("HOME") + "/.local/bin/codemux"`),
    `function run(args)` (`Quickshell.execDetached([exe].concat(args))`). A `Process`
    runs `[exe, "watch"]` with `stdout: SplitParser` feeding `Codemux.parseView`; on
    exit it sets `connected = false` and a 2000 ms `Timer` restarts it (spec 3.1).
  - `BarWidget.qml` (`BarWidget` from `qs.Ui`, `moduleName "synju.codemux"`): finds
    its connector exactly as `Workspaces.qml:19-24` does, renders
    `Codemux.barRow(service.view, connector, vertical)`; a segment click runs
    `service.run(["focus-id", String(id)])`; current segment highlighted, empty
    segments at reduced opacity; markers drawn after the segments. With no service
    or `connected == false` it shows one dim `codemux` label. Vertical bar: numbers
    only (spec 4.5).
  - `Codemux.js` (`.pragma library`):
    `parseView(line) -> object|null` (null on bad JSON),
    `monitorRow(view, connector) -> object|null`,
    `barRow(view, connector, vertical) -> { session: string, items: [{ id, text, current, empty }], markers: [string] }`
    (markers in order `LEADER`/`RESIZE` then `MARK`; vertical: `session` is `""`
    and every `text` is the number).

- [ ] **Step 1: Write failing JS tests** (`tests/tst_codemux.qml`, `TestCase`, `import "../Codemux.js" as Codemux`):

```qml
function test_parseView_rejects_garbage() { compare(Codemux.parseView("{oops"), null) }
function test_barRow_for_connector() {
  var v = { mode: "", marked: false, monitors: [{ connector: "DP-1", session_label: "main",
            workspaces: [{ id: 100001, number: 1, label: "foot 1", current: true, empty: false }] }] }
  var r = Codemux.barRow(v, "DP-1", false)
  compare(r.session, "main"); compare(r.items[0].text, "foot 1"); compare(r.items[0].current, true)
}
function test_unknown_connector_gives_null_row() { compare(Codemux.monitorRow({ monitors: [] }, "DP-9"), null) }
function test_markers() { /* mode "LEADER", marked true -> markers == ["LEADER", "MARK"] */ }
function test_vertical_numbers_only() { /* same view, vertical true -> items[0].text == "1", session == "" */ }
```

- [ ] **Step 2: Run** `/usr/lib/qt6/bin/qmltestrunner -input plugin/synju.codemux/tests`.
  Expected: FAIL (no `Codemux.js`).
- [ ] **Step 3: Implement** the four files. Take styling (font, colors,
  `WidgetButton` margins, `bar.barSize`) from `Workspaces.qml:247-286`.
- [ ] **Step 4: Run** the qmltestrunner command and `qmlformat` on every `.qml`.
  Expected: all JS tests pass; every parse exits 0.
- [ ] **Step 5: Spike: plugin loads in the live shell (reversible).** Copy
  `plugin/synju.codemux/` to `~/.config/omarchy/plugins/synju.codemux/`, run
  `omarchy-shell shell rescanPlugins`, then
  `omarchy plugin enable synju.codemux --section right` (temporary place, away from
  `omarchy.workspaces`). With the phase 2 daemon still installed the stream is the
  old raw state, so expect the dim `codemux` label or an empty row, never a QML
  error. Find where the shell logs QML warnings (`qs log`, or the omarchy-shell user
  unit's journal) and record which works in `docs/spikes.md` ("Phase 3 spike:
  plugin load"), with any warnings. Revert: `omarchy plugin disable synju.codemux`,
  `rm -rf ~/.config/omarchy/plugins/synju.codemux`, `omarchy-shell shell rescanPlugins`;
  check `jq '.bar.layout' ~/.config/omarchy/shell.json` equals its pre-spike value.
- [ ] **Step 6: Commit** (`feat: synju.codemux plugin service and bar widget`).

### Task 5: Install and uninstall the plugin

**Files:**
- Modify: `Cargo.toml` (`serde_json` feature `preserve_order`), `src/install.rs`,
  `packaging/codemux-uninstall.sh`
- Test: `src/install.rs`

**Interfaces:**
- Produces:
  - `pub const PLUGIN_FILES: &[(&str, &str)]` (relative path, `include_str!`
    contents) covering every file under `plugin/synju.codemux/` except `tests/`.
  - `pub fn find_bar_widget(shell_json: &str, id: &str) -> Option<(String, usize)>`
    (section, index within `bar.layout`).
  - `pub fn replace_bar_widget(shell_json: &str, from: &str, to: &str, index: usize) -> Result<String>`
    (the spec 8.1 fallback: drop `from` from every `bar.layout` section; insert
    `{ "id": to }` at `index` of `left` (clamped) if `to` is not on the bar; keys keep
    their order).
  - `install::install` gains, after the existing Hyprland steps (spec 8.1):
    1. write `PLUGIN_FILES` into `~/.config/omarchy/plugins/synju.codemux/` (directory
       removed first, each file via `write_atomic`);
    2. if `find_bar_widget(shell.json, "omarchy.workspaces")` is `Some((s, i))`,
       write `s i` to `~/.local/state/codemux/bar-place`;
    3. `omarchy-shell shell rescanPlugins`;
       `omarchy plugin disable omarchy.workspaces`;
       `omarchy plugin enable synju.codemux --section left --index 1`.
       Disabling a first-party bar widget is supported: it only removes its layout
       entry (`/usr/share/omarchy/shell/services/PluginRegistry.qml`, `setEnabled`,
       the `location.kind === "bar"` splice). If any of these commands exits
       non-zero (for example the shell is not running), apply
       `replace_bar_widget(.., "omarchy.workspaces", "synju.codemux", 1)` to
       `~/.config/omarchy/shell.json` with `write_atomic` and say so.
  - Uninstall script, spec 8.2 step 4 (inserted before the state deletion), printing
    each step: read `~/.local/state/codemux/bar-place` (default `left 1`);
    `omarchy plugin disable synju.codemux`; `rm -rf ~/.config/omarchy/plugins/synju.codemux`;
    `omarchy-shell shell rescanPlugins`;
    `omarchy plugin enable omarchy.workspaces --section <s> --index <i>`; if either
    `omarchy plugin` call fails, edit `shell.json` with `jq` (replace the
    `synju.codemux` entry by `{"id":"omarchy.workspaces"}`, or insert it at the
    recorded place) via a temp file and `mv`. Renumber the script's echo lines to
    follow spec 8.2's order.

- [ ] **Step 1: Write failing tests**

```rust
const SHELL: &str = r#"{"bar":{"layout":{"left":[{"id":"omarchy.menu"},{"id":"omarchy.workspaces"}],"right":[]}},"version":1}"#;
#[test] fn finds_workspaces_widget_place() { assert_eq!(find_bar_widget(SHELL, "omarchy.workspaces"), Some(("left".into(), 1))); }
#[test] fn fallback_swaps_widget_in_place_and_keeps_key_order() {
    let out = replace_bar_widget(SHELL, "omarchy.workspaces", "synju.codemux", 1).unwrap();
    assert_eq!(find_bar_widget(&out, "synju.codemux"), Some(("left".into(), 1)));
    assert_eq!(find_bar_widget(&out, "omarchy.workspaces"), None);
    assert!(out.find("\"bar\"").unwrap() < out.find("\"version\"").unwrap());
}
#[test] fn fallback_is_idempotent() { /* applying twice == applying once */ }
#[test] fn plugin_files_cover_the_plugin_dir() {
    /* walk plugin/synju.codemux (skip tests/): every file is in PLUGIN_FILES and vice versa */ }
#[test] fn uninstaller_restores_workspaces_widget() {
    assert!(UNINSTALLER.contains("omarchy plugin enable omarchy.workspaces"));
    assert!(UNINSTALLER.contains("bar-place"));
    /* bar-place is read before `rm -rf "$HOME/.local/state/codemux"` */ }
```

- [ ] **Step 2: Run** `cargo test install`. Expected: FAIL.
- [ ] **Step 3: Implement**; `bash -n packaging/codemux-uninstall.sh`.
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: install the bar plugin in place of omarchy.workspaces`).

### Task 6: Phase 3 live acceptance (user, on the real desktop)

**Files:** none (results appended to `docs/spikes.md` under "Phase 3 acceptance").

- [ ] **Step 1:** `cargo build --release && ./target/release/codemux install`.
  Expected: `jq -c '.bar.layout.left' ~/.config/omarchy/shell.json` shows
  `synju.codemux` at index 1 and no `omarchy.workspaces`; `cat ~/.local/state/codemux/bar-place`
  prints `left 1`; each monitor's bar shows its own session name and workspaces.
- [ ] **Step 2:** User checks on both monitors: the label follows the focused app
  (open foot: `foot N`); clicking a workspace switches to it, also on the
  non-focused monitor; Super + B shows `LEADER` until Escape or 2 s; `codemux new main`
  on the other monitor makes both bars show `main (n)`; a workspace left empty but
  shown shows its number only.
- [ ] **Step 3:** `pkill -x codemux`: within 2 s every bar shows a dim `codemux`.
  `hyprctl reload`: the bars recover within 2 s.
- [ ] **Step 4:** `omarchy bar position bottom`: the widget still renders; then
  `omarchy bar position top` (revert).
- [ ] **Step 5:** Run the uninstaller: `omarchy.workspaces` is back at `left` index
  1, `~/.config/omarchy/plugins/synju.codemux` is gone, `~/.codemux.conf` kept.
  Reinstall to keep using it. Record pass/fail per step.

---

## Phase 4: Overlays

### Task 7: Side-effect plumbing: spawn, notify, deadlines

**Files:**
- Modify: `src/hypr.rs`, `tests/common/mod.rs`, `src/engine.rs`, `src/daemon.rs`
- Test: `src/daemon.rs`, `tests/engine_overlays.rs` (new)

**Interfaces:**
- Produces:
  - `Hypr` gains `fn spawn(&self, argv: &[String], cwd: Option<&Path>) -> Result<u32>`
    and `fn notify(&self, msg: &str)`. `Hyprctl::spawn`: `Command` with
    `process_group(0)`, null stdio, returns the child pid and hands the `Child` to a
    reaper thread (`child.wait()`), so no zombies and no `SIGCHLD` changes (which
    would break `Command::output` for `hyprctl`). `Hyprctl::notify` =
    `notify-send codemux <msg>`.
  - `FakeHypr` gains `spawned: RefCell<Vec<Vec<String>>>`, `next_pid: Cell<u32>`
    (starts at 9000, increments), `notified: RefCell<Vec<String>>`.
  - Engine: `pub fn next_deadline(&self) -> Option<Instant>` and
    `pub fn on_deadline(&mut self, now: Instant) -> Result<()>` (empty dispatch hub;
    Tasks 9, 16, 19, 23 register their timed jobs here). `pub exe: String` field,
    set in `Engine::new` to `daemon::installed_exe()` (tests overwrite it).
  - `daemon::loop_timeout(pending_since: Option<Instant>, deadline: Option<Instant>, now: Instant) -> Option<Duration>`:
    the earliest of "settle 300 ms after the last pending event" and the engine
    deadline; `None` = block. `run()` uses it with `recv_timeout`; on timeout it runs
    `enforce_invariant` if the settle elapsed and `on_deadline(Instant::now())`.
  - `Engine::start`'s relaunch `eprintln!` becomes `hypr.notify` only where a user
    must see it (none yet; leave it).

- [ ] **Step 1: Write failing tests**

```rust
// src/daemon.rs
#[test] fn loop_timeout_picks_the_earliest() {
    let now = Instant::now();
    assert_eq!(loop_timeout(None, None, now), None);
    assert_eq!(loop_timeout(Some(now), None, now), Some(Duration::from_millis(300)));
    assert_eq!(loop_timeout(Some(now), Some(now + Duration::from_millis(50)), now), Some(Duration::from_millis(50)));
    assert_eq!(loop_timeout(None, Some(now - Duration::from_secs(1)), now), Some(Duration::ZERO));
}
// tests/engine_overlays.rs
#[test] fn fake_spawn_records_argv_and_returns_pids() {
    /* h.spawn(&["a".into()], None) == 9000, then 9001; h.spawned()[0] == ["a"] */ }
#[test] fn no_deadline_when_idle() { assert_eq!(engine().next_deadline(), None); }
```

- [ ] **Step 2: Run** `cargo test loop_timeout --test engine_overlays`. Expected: FAIL.
- [ ] **Step 3: Implement.** Keep the existing settle behaviour exactly (Task 7 of
  the phase 1-2 plan): an invariant-pending event restarts the 300 ms window.
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: spawn, notify and deadlines for the engine`).

### Task 8: Overlay summons from leader actions

**Files:**
- Create: `src/overlay.rs`
- Modify: `src/lib.rs`, `src/engine.rs` (`Request::Action` branch)
- Test: `src/overlay.rs`, `tests/engine_overlays.rs`, `tests/engine_commands.rs`

**Interfaces:**
- Consumes: `Hypr::spawn`, `view::session_label`, `view::workspace_label`,
  `reading_key`, `config.display_panes_ms`.
- Produces:
  - `pub fn summon_argv(payload: &serde_json::Value) -> Vec<String>` =
    `["omarchy-shell", "shell", "summon", "synju.codemux", <payload as compact JSON>]`.
  - Engine: `pub(crate) fn summon(&self, payload: serde_json::Value) -> Result<()>`.
  - Payloads (the `menu` root switches on `view`):
    - `{"view":"tree"}`
    - `{"view":"command"}`
    - `{"view":"rename","target":"session","id":<number>,"current":"<name>"}`
    - `{"view":"rename","target":"workspace","id":<real_id>,"current":"<display name or empty>"}`
    - `{"view":"options"}`
    - `{"view":"confirm","text":"Kill <name>? (y/n)","run":["kill-session","3"]}` (built by Task 9 / 16)
    - `{"view":"panes","timeout":<display_panes_ms>,"panes":[{"key":"1","address":"0x..","monitor":"DP-1","x":0,"y":0,"w":960,"h":1080}]}`
  - `pub struct Pane { pub key: String, pub address: String, pub monitor: String, pub x: i32, pub y: i32, pub w: i32, pub h: i32 }` (Serialize)
  - `pub fn number_panes(monitors: &[HMonitor], clients: &[HClient]) -> Vec<Pane>`:
    visible tiles only (client on its monitor's `active_workspace`, regular id;
    placeholders included, spec 5.5); monitors sorted by `x` then `y`; within a
    monitor `reading_key`; keys `1`-`9` then `a`, `b`, `c`, ...; coordinates
    relative to the monitor's origin (layout coordinates, as `hyprctl` reports).
  - `pub fn pane_key(i: usize) -> String`.
  - `Request::Action` maps `tree`, `command-prompt`, `rename-session`,
    `rename-workspace`, `options`, `display-panes` to these summons (rename targets
    the focused session / the workspace shown on the focused monitor). `mark`,
    `swap`, `kill-tile` keep answering "not in this version yet" until Tasks 16, 18.

- [ ] **Step 1: Write failing tests**

```rust
// src/overlay.rs
#[test] fn pane_keys_digits_then_letters() {
    assert_eq!((0..11).map(pane_key).collect::<Vec<_>>(), ["1","2","3","4","5","6","7","8","9","a","b"]);
}
#[test] fn panes_number_monitors_left_to_right_then_reading_order() {
    /* DP-2 at x=0 (shows 200001: tiles at (960,0) and (0,0)), DP-1 at x=1920 (shows 100001: one tile):
       keys 1,2 on DP-2 in order (0,0),(960,0); key 3 on DP-1; DP-1 pane x is relative (at.x - 1920) */ }
#[test] fn panes_skip_unshown_and_special_workspaces() { /* client on 100002 (not shown) and on -98: absent */ }
#[test] fn summon_argv_shape() {
    assert_eq!(summon_argv(&json!({"view":"tree"})), ["omarchy-shell","shell","summon","synju.codemux","{\"view\":\"tree\"}"]);
}
// tests/engine_overlays.rs
#[test] fn tree_action_summons_tree() { /* Action{"tree"} -> spawned == [summon_argv({"view":"tree"})] */ }
#[test] fn rename_session_prefills_current_name() {
    /* Action{"rename-session"} -> payload target "session", id 1, current "1" */ }
#[test] fn rename_workspace_prefills_display_name() {
    /* shown 100001 with auto_name "foot" and an app tile -> current "foot" */ }
#[test] fn display_panes_uses_configured_timeout() { /* config.display_panes_ms = 1500 -> payload timeout 1500 */ }
// tests/engine_commands.rs: replace `unimplemented_action_says_so` (it used "tree")
#[test] fn unimplemented_action_says_so() { /* Action{"swap"} -> message "codemux: swap is not in this version yet" */ }
```

- [ ] **Step 2: Run** `cargo test overlay --test engine_overlays --test engine_commands`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: leader actions summon the codemux overlays`).

### Task 9: Requests behind overlay choices

**Files:**
- Modify: `src/engine.rs` (new `Request` variants), `src/overlay.rs` (kill jobs),
  `src/hypr.rs` (`focus_window`, `close_window`), `src/main.rs`
- Test: `tests/engine_overlays.rs`, `tests/cli.rs`

**Interfaces:**
- Produces:
  - Lua builders: `focus_window(addr) -> String` =
    `hl.dsp.focus({ window = "address:<a>" })`;
    `close_window(addr) -> String` = `hl.dsp.window.close({ window = "address:<a>" })`
    (forms confirmed live in Task 14's spike; Task 11's acceptance uses them first,
    so Step 1 below checks them).
  - `Request` variants (kebab-case `cmd`) and CLI forms:
    - `FocusWindow { address: String }`, `codemux focus-window ADDR`: focus the
      window's monitor if needed, then `focus_window` (the session follows through
      the `workspacev2` path, spec 3.2.6).
    - `FocusMonitor { connector: String }`, `codemux focus-monitor NAME`.
    - `RenameSession { number: u32, name: String }`,
      `codemux rename-session N NAME...`: trimmed empty name ->
      `Response::err("session name cannot be empty")`, no change (spec 5.2).
    - `RenameWorkspace { id: i64, name: String }`,
      `codemux rename-workspace ID [NAME...]`: empty -> `name = None` (automatic).
    - `Command { line: String }`, `codemux command LINE...`: `new` and `new <name>`
      run `NewSession`; empty -> ok, nothing; anything else ->
      `Response::err("unknown command: <line>")` (spec 5.3).
    - `SetRelaunch { on: bool }`, `codemux set-relaunch on|off`: writes
      `config::set_relaunch_line` to `self.config_path` (new `pub config_path: PathBuf`
      field, default `config::config_path()`) with `write_atomic`, sets
      `config.relaunch`, `view_changed`. No Hyprland reload follows: the generated Lua
      does not depend on `relaunch` (`daemon::lua_update`, spec 6.3).
    - `KillSession { number: u32 }`, `KillWorkspace { id: i64 }`
      (`codemux kill-session N`, `codemux kill-workspace ID`): Ruling 7. Dispatch
      `close_window` for every tile (placeholders included), drop archives (Task 19
      adds the field; drop it there too), delete unshown window-less workspaces now,
      and register `pub(crate) struct KillJob { pub label: String, pub addresses: Vec<String>, pub deadline: Instant, pub switch_monitor: Option<String> }`
      in `pub(crate) kills: Vec<KillJob>`. On `CloseWindow` events and in
      `on_deadline`: a job whose addresses are all gone completes (switching the
      monitor for an active-session kill: `activate` + `focus_monitor` + `focus` of
      the other session's show target, or a new `main`); at the deadline (10 s) any
      live address is named in one notification
      `codemux: still open after 10 s: <class> "<title>"[, ...]` and the job ends.
  - CLI: a leading `--no-notify` makes `client()` print a failure message to stdout
    and exit 1 without `notify-send` (prompts show the message themselves).
  - Overlays also need the kill confirm for the tree: built in QML (Task 10) from the
    tree labels; no extra engine payload.

- [ ] **Step 1: Check the two dispatcher forms (live, reversible).** Open a foot
  window, read its address with `hyprctl -j activewindow | jq -r .address`, focus
  another window, then run `hyprctl dispatch 'hl.dsp.focus({ window = "address:<a>" })'`
  (expect `ok`, foot focused) and `hyprctl dispatch 'hl.dsp.window.close({ window = "address:<a>" })'`
  (expect `ok`, foot closes). If either form is rejected, try the selector key
  Omarchy uses elsewhere (`hl.dsp.window.move({ ..., window = "address:<a>" })` is
  confirmed by phase 2) and record the working form in `docs/spikes.md`
  ("Phase 4 spike: window selectors"). Nothing to revert beyond the closed foot.
- [ ] **Step 2: Write failing tests**

```rust
#[test] fn rename_session_refuses_empty() { /* RenameSession{1,"  "} -> !ok, message "session name cannot be empty", name unchanged */ }
#[test] fn rename_workspace_empty_resets_to_automatic() { /* name Some("x") -> RenameWorkspace{id,""} -> None */ }
#[test] fn command_new_with_name_creates_session() { /* Command{"new Work"} -> a session named "Work", active on focused monitor */ }
#[test] fn command_unknown_is_reported() { /* Command{"frob"} -> message "unknown command: frob" */ }
#[test] fn set_relaunch_rewrites_only_that_line() {
    /* config_path = tmp holding "# mine\nset relaunch on\n"; SetRelaunch{false} -> file == "# mine\nset relaunch off\n"; e.config.relaunch == false */ }
#[test] fn kill_workspace_closes_every_window() { /* KillWorkspace{100002} with tiles 0xa2 -> dispatched contains close_window("0xa2") */ }
#[test] fn kill_deletes_unshown_empty_workspaces_at_once() { /* session 1 has unshown empty 100003: KillSession{1} -> 100003 gone immediately */ }
#[test] fn kill_names_windows_still_open_after_10s() {
    /* KillWorkspace{100002}; on_deadline(now + 11s) -> notified[0] contains "foot" */ }
#[test] fn killing_active_session_switches_monitor_once_its_windows_are_gone() {
    /* session 1 active on LG A (shown 100001, tile 0xa1), session 4 also on LG A (last_used higher than others);
       KillSession{1}; CloseWindow{0xa1} -> LG A active_session == 4, dispatched ends with focus of 4's show target */ }
#[test] fn killing_last_session_of_monitor_makes_main() { /* only session on LG A killed -> after windows close a session "main" is active */ }
#[test] fn focus_window_on_other_monitor_focuses_monitor_first() { /* client 0xb1 on DP-2 -> [focus_monitor("DP-2"), focus_window("0xb1")] */ }
// tests/cli.rs
#[test] fn no_notify_flag_prints_instead_of_notifying() {
    /* `codemux --no-notify focus 1` without daemon: exit 1, stdout contains "daemon not running", notify log absent */ }
```

- [ ] **Step 3: Run** `cargo test --test engine_overlays --test cli`. Expected: FAIL.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 6: Commit** (`feat: requests for tree, prompts, options and kills`).

### Task 10: Menu root and the modal overlays

**Files:**
- Create: `plugin/synju.codemux/Menu.qml`, `views/Tree.qml`, `views/Prompt.qml`,
  `views/Options.qml`, `views/Confirm.qml`
- Modify: `plugin/synju.codemux/manifest.json` (`kinds` gains `"menu"`,
  `entryPoints.menu = "Menu.qml"`), `Codemux.js`, `tests/tst_codemux.qml`,
  `src/install.rs` (`PLUGIN_FILES`)
- Test: `tests/tst_codemux.qml`, `src/install.rs` (`plugin_files_cover_the_plugin_dir`)

**Interfaces:**
- Consumes: host contract for `menu` plugins (`shell.qml:1395-1416`: the root gets
  `shell`, `manifest`, `service`; with `keepLoaded` it is mounted at start and
  `open(payloadJson)` is called on every summon, `shell.qml:1282-1293`;
  `isPluginOpen` reads `item.opened`, `shell.qml:1239-1248`); `OverlayWindow` from
  `qs.Ui` (`/usr/share/omarchy/shell/Ui/OverlayWindow.qml`: fullscreen, focused
  screen, `WlrLayer.Overlay`, `WlrKeyboardFocus.Exclusive`, driven by `shown`).
- Produces:
  - `Menu.qml` root `Item`: `property var shell`, `property var manifest`,
    `property var service`, `property bool opened`, `property var payload`,
    `function open(payloadJson)` (parse with `Codemux.parsePayload`, set `opened`,
    load the matching view into one `OverlayWindow`), `function close()` (clears
    `opened`; views call `root.finish()` = `close()` plus `shell.hide("synju.codemux")`).
  - `views/Tree.qml`: rows from `Codemux.treeRows(service.view.tree, collapsed)`;
    preselect with `Codemux.preselect(rows)`; keys: Up / `k`, Down / `j` move,
    Left / `h` collapse, Right / `l` expand (Ruling 6), Enter runs
    `Codemux.pickCommand(row)` via `service.run`, `x` on a session or workspace row
    opens `Confirm` with `Kill <label>? (y/n)` and
    `["kill-session", n]` / `["kill-workspace", id]`, Escape closes.
  - `views/Prompt.qml` (rename and command): one `TextInput` prefilled with
    `payload.current`; Enter runs a `Process`
    `[exe, "--no-notify", ...Codemux.promptCommand(payload, text)]`, closes on exit
    0, otherwise shows the process stdout under the field and stays open; Escape
    cancels.
  - `views/Options.qml`: one row "Relaunch apps on restart" bound to
    `service.view.relaunch`; Space / Enter runs `set-relaunch on|off`; Escape closes.
  - `views/Confirm.qml`: shows `text`; `y` runs `run` and closes; any other key
    closes.
  - `Codemux.js` additions:
    `parsePayload(json) -> object` (`{ view: "" }` on bad input),
    `treeRows(tree, collapsed) -> [{ kind, depth, key, label, id, address, connector, current }]`
    (`key` unique per node, e.g. `s:3`, `w:100002`, `t:0xab`, `m:DP-1`; children of
    a collapsed key omitted; `current` marks the focused monitor, its active session,
    shown workspace and focused tile),
    `preselect(rows) -> int` (deepest `current` row, else 0),
    `pickCommand(row) -> [string]` (`["focus-monitor", c]`, `["switch", n]`,
    `["focus-id", id]`, `["focus-window", a]`),
    `promptCommand(payload, text) -> [string]`
    (`["rename-session", id, text]`, `["rename-workspace", id, text]`,
    `["command", text]`).

- [ ] **Step 1: Write failing JS tests**

```qml
function test_tree_rows_flatten_in_order() {
  /* tree: DP-1 > session 1 "main" > ws 100001 "foot 1" > tile 0xa1:
     kinds == ["monitor","session","workspace","tile"], depths == [0,1,2,3] */ }
function test_collapsed_session_hides_children() { /* collapsed {"s:1": true} -> 2 rows */ }
function test_preselect_deepest_current() { /* tile row current -> index 3 */ }
function test_pick_commands() {
  /* session row -> ["switch","1"]; workspace -> ["focus-id","100001"]; tile -> ["focus-window","0xa1"]; monitor -> ["focus-monitor","DP-1"] */ }
function test_prompt_commands() {
  /* {view:"rename",target:"workspace",id:100002} + "notes" -> ["rename-workspace","100002","notes"];
     {view:"command"} + "new Work" -> ["command","new Work"] */ }
function test_bad_payload_is_empty_view() { compare(Codemux.parsePayload("nope").view, "") }
```

- [ ] **Step 2: Run** the qmltestrunner command. Expected: FAIL.
- [ ] **Step 3: Implement** the QML. Card look (border, radius, colors, font) follows
  `/usr/share/omarchy/shell/plugins/menu/Menu.qml` (`OverlayWindow` + `BorderSurface`
  card, scrim click cancels).
- [ ] **Step 4: Run** qmltestrunner, `qmlformat` on each new file,
  `cargo test install`. Expected: PASS.
- [ ] **Step 5: Spike: keyboard focus of a third-party overlay (live, reversible).**
  Install the build (`cargo build --release && ./target/release/codemux install`),
  then `omarchy-shell shell summon synju.codemux '{"view":"command"}'`: the prompt
  appears on the focused monitor and typing reaches it; Escape closes it and focus
  returns to the previous window. Record in `docs/spikes.md`
  ("Phase 4 spike: overlay focus"). If keyboard focus does not arrive, record what
  happens and stop for a ruling. Revert: none needed (the install is the product);
  the user may run the uninstaller.
- [ ] **Step 6: Commit** (`feat: tree, prompts, options and confirm overlays`).

### Task 11: Tile numbers overlay (one surface per monitor)

**Files:**
- Create: `plugin/synju.codemux/views/Panes.qml`
- Modify: `Menu.qml`, `Codemux.js`, `tests/tst_codemux.qml`, `src/install.rs`
- Test: `tests/tst_codemux.qml`

**Interfaces:**
- Consumes: the `panes` payload (Task 8), `Quickshell.screens`, `Hyprland.focusedMonitor`.
- Produces:
  - `views/Panes.qml`: a `Variants { model: Quickshell.screens }` of `PanelWindow`s
    (`WlrLayershell.layer: WlrLayer.Overlay`, `exclusionMode: ExclusionMode.Ignore`,
    all four anchors, transparent), each drawing a large label centered on every
    pane whose `monitor` equals its screen name; only the focused screen's window
    takes `WlrKeyboardFocus.Exclusive`, the others `None`. A `Timer` of
    `payload.timeout` ms closes it. Key handling: `Codemux.paneForKey(panes, text)`
    -> run `["focus-window", address]` and close; Escape or any other key closes
    (spec 5.5).
  - `Codemux.paneForKey(panes, key) -> object|null` (case-insensitive for letters).

- [ ] **Step 1: Write failing JS tests**

```qml
function test_pane_for_key() { /* panes keys "1","a": paneForKey(p,"1").address == "0x1"; paneForKey(p,"A").address == "0xa" */ }
function test_unknown_key_closes() { compare(Codemux.paneForKey([{ key: "1" }], "z"), null) }
```

- [ ] **Step 2: Run** qmltestrunner. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Spike: layer surfaces on both monitors (live).** With two monitors
  and a few tiles each, run `codemux action display-panes` from a terminal. Check
  and record in `docs/spikes.md` ("Phase 4 spike: per-monitor overlay"): numbers
  appear on both monitors at the tile centers (including eDP-2 at scale 1.6);
  keys reach only the focused monitor's surface; pressing a number on the other
  monitor's tile moves focus there. If coordinates are off by the scale, record the
  needed transform and apply it in `Panes.qml`. Nothing to revert.
- [ ] **Step 5: Run** qmltestrunner, `qmlformat`, `cargo test install`. Expected: PASS.
- [ ] **Step 6: Commit** (`feat: tile numbers overlay`).

### Task 12: Phase 4 live acceptance (user)

**Files:** none (results under "Phase 4 acceptance" in `docs/spikes.md`).

- [ ] **Step 1:** Install the release build.
- [ ] **Step 2:** Leader + `w`: the tree opens with the current tile preselected;
  j/k and arrows move; h/l collapse and expand; Enter on a tile of the other
  monitor's session switches there and focuses it; `x` on a spare workspace asks
  `Kill <name>? (y/n)`, `y` closes its windows and it disappears from the bar.
- [ ] **Step 3:** Leader + `:`: `new Work` creates and shows session `Work`;
  `frob` shows `unknown command: frob` in the prompt. Leader + `$`: rename to
  `Notes`, the bar updates; an empty name shows the refusal message. Leader + `,`:
  rename, then rename to empty: the automatic name returns.
- [ ] **Step 4:** Leader + `o`: toggle relaunch off; `grep relaunch ~/.codemux.conf`
  shows `off`, no Hyprland reload happened (no `configreloaded`); toggle back on.
- [ ] **Step 5:** Leader + `q` on two monitors: numbers on every tile, a digit
  focuses that tile (also across monitors), Escape and the 2 s timeout close it.
- [ ] **Step 6:** Record pass/fail per step.

---

## Phase 5: Tiles

Spec 6.2's geometry capture lands first (Task 13): placeholder archiving (Task 19)
and relaunch (Tasks 22-24) read it.

### Task 13: Mode tracking and geometry capture (spec 6.2)

**Files:**
- Create: `src/geometry.rs`
- Modify: `src/model.rs`, `src/hypr.rs`, `tests/common/mod.rs`, `src/engine.rs`
  (`tick`, `renumber`), `src/daemon.rs`
- Test: `tests/engine_geometry.rs` (new)

**Interfaces:**
- Produces:
  - `Workspace.mode: Option<String>` (`#[serde(default)]`): the last mode read
    (spec 3.2.11, 7.1). `Tile` derives `Default`; test helpers building `Tile`
    literals (`tests/engine_events.rs:9`, `tests/engine_layout.rs:27`) switch to
    `..Default::default()`.
  - `HClient.hidden: bool` (from `hyprctl -j clients` `hidden`, serde default false);
    `tests/common::client` sets `false`.
  - `hypr::set_mode(id, mode) -> String` =
    `o.workspace_mode({ workspace = "<id>", mode = "<mode>" })` (extracted from
    `Engine::renumber`, `src/engine.rs:419`, which now uses it).
  - Engine (`src/geometry.rs`):
    `pub(crate) fn live_mode(&self, id: i64) -> Result<String>`
    (`layout::read_mode(layouts_dir, id, live tiledLayout, layout::omarchy_default_mode)`),
    `pub fn check_modes(&mut self) -> Result<()>` (reads every known workspace's
    live mode, stores it in `mode`; Task 19 hooks archive / recreate here),
    `pub fn capture_geometry(&mut self) -> Result<bool>` (for each live client with
    a tile record: update `at`, `size`, `floating`, `title`; skip workspaces whose
    recorded `mode` differs from the live mode; if Hyprland reports the window on a
    different known regular workspace, move the record there and apply
    `maybe_delete` to the old one (Carry-over: tick re-sync); returns whether
    anything changed).
  - `Engine::tick` order becomes: missed windows (existing), `check_modes`,
    `capture_geometry` (dirty only if it returned true), outside-renumber check
    (existing), `enforce_invariant` (existing).
  - `daemon::run`: when `take_dirty()` is true, call `engine.capture_geometry()`
    before `save_atomic` (geometry is captured whenever the state file is written,
    spec 6.2).

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn capture_updates_geometry_and_reports_change() {
    /* tile 0xa1 at (0,0) 100x100; live client at (10,20) 300x400 -> capture true, tile updated; second capture false */ }
#[test] fn capture_skips_workspace_whose_mode_changed() {
    /* ws 100002 recorded mode Some("dwindle"), layouts dir has 100002.lua floating; live geometry differs -> tile unchanged */ }
#[test] fn hand_floated_window_on_tiled_workspace_is_captured() {
    /* mode dwindle both recorded and live; client floating true at new spot -> tile.floating true, at updated */ }
#[test] fn tick_runs_mode_check_before_capture() {
    /* recorded None, file says scrolling, geometry differs: after tick mode == Some("scrolling") and geometry captured */ }
#[test] fn capture_resyncs_tile_workspace() {
    /* record says 0xa2 on 100002, hyprctl says 100001 -> record moved to 100001; 100002 (unshown, now empty) deleted */ }
#[test] fn old_tiles_without_new_fields_load() { /* phase 2 state JSON parses */ }
```

- [ ] **Step 2: Run** `cargo test --test engine_geometry`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: layout mode tracking and periodic geometry capture`).

### Task 14: `codemux placeholder` and its window rules

**Wayland client choice:** `smithay-client-toolkit` (sctk), added with
`cargo add smithay-client-toolkit --no-default-features` (record the resolved
version in the ledger). It wraps `wayland-client` and already handles the registry,
`xdg_wm_base` ping, toplevel configure and shm buffer pools, so the placeholder is
one small file; its `simple_window` example is this exact shape. Raw
`wayland-client` plus `wayland-protocols` would need the same code written by hand;
GTK or a GUI toolkit would pull a far larger dependency tree for one transparent
rectangle. Default features (calloop, xkbcommon) are not needed: the placeholder
takes no keyboard input of its own and runs a plain `blocking_dispatch` loop.

**Files:**
- Create: `src/placeholder.rs`, `tests/placeholder.rs`
- Modify: `Cargo.toml`, `src/lib.rs`, `src/main.rs`, `src/luagen.rs`
- Test: `tests/placeholder.rs`, `src/luagen.rs`

**Interfaces:**
- Produces:
  - `pub fn run() -> Result<()>` behind `codemux placeholder`: one xdg toplevel,
    `app_id` `codemux-placeholder`, title `codemux placeholder`; on every configure
    it attaches a fully transparent ARGB8888 shm buffer of the configured size
    (min 1x1) and commits; it exits 0 on the toplevel close event and on a lost
    connection. No content, no input handling beyond what the compositor needs.
  - `pub fn buffer_len(w: u32, h: u32) -> usize` (`w.max(1) * h.max(1) * 4`).
  - `luagen::generate` emits, before the daemon start line:
    `o.window("^codemux-placeholder$", { workspace = "special:codemux-stash silent", border_size = 0, no_border = true, no_shadow = true, no_blur = true, decorate = false, no_dim = true, rounding = 0, no_anim = true })`
    (rule names as Omarchy uses them, `default/hypr/apps/*.lua`; exact set from the
    spike below).

- [ ] **Step 1: Spike (live, reversible).** Write `spike/codemux-ph.lua` with only
  the window rule above, symlink it into `~/.config/hypr/`, add
  `require("hypr.codemux-ph")` after the codemux line in `hyprland.lua`, and
  `cargo build`. Run `./target/debug/codemux placeholder &` from a foot window on a
  `dwindle` workspace (expect: it lands on `special:codemux-stash`, check with
  `hyprctl -j clients | jq '.[] | select(.class=="codemux-placeholder") | .workspace'`).
  Then record, in `docs/spikes.md` ("Phase 5 spike: placeholder"):
  1. `hyprctl dispatch 'hl.dsp.window.move({ workspace = "<dwindle id>", follow = false, window = "address:<p>" })'`:
     it tiles like an app, fully invisible, no border or shadow, even when focused;
  2. the same on a `scrolling` workspace;
  3. `hl.dsp.window.swap({ target = "address:<p>", window = "address:<foot>" })`
     across workspaces emits `movewindowv2` for both windows (yes / no);
  4. `hl.dsp.window.close({ window = "address:<p>" })` makes the process exit 0;
  5. absolute geometry dispatchers for Tasks 19 and 24: try
     `hl.dsp.window.resize({ x = 800, y = 600, relative = false, window = "address:<a>" })`
     and `hl.dsp.window.move({ x = 100, y = 100, relative = false, window = "address:<a>" })`
     on a floating window, then the resize on a tiled one; record the forms that
     work;
  6. any window-rule key Hyprland rejects (check `hyprctl configerrors`).
  Revert: close the placeholder, remove the require line and the symlink, delete
  `spike/`, `hyprctl reload`; `grep -c codemux-ph ~/.config/hypr/hyprland.lua`
  prints `0`.
- [ ] **Step 2: Write failing tests**

```rust
// tests/placeholder.rs
#[test] fn placeholder_without_wayland_fails_cleanly() {
    /* run `codemux placeholder` with WAYLAND_DISPLAY unset and XDG_RUNTIME_DIR=<empty tmp>: exit code != 0 within 2 s, stderr mentions wayland */ }
#[test] fn buffer_len_is_never_zero() { assert_eq!(codemux::placeholder::buffer_len(0, 0), 4); }
// src/luagen.rs
#[test] fn placeholder_rule_sends_it_to_the_stash_without_decoration() {
    let g = lua(); assert!(g.contains("o.window(\"^codemux-placeholder$\"")); assert!(g.contains("special:codemux-stash silent"));
    assert!(g.find("codemux-placeholder").unwrap() < g.find(" daemon\")").unwrap());
}
```

- [ ] **Step 3: Run** `cargo test --test placeholder luagen`. Expected: FAIL.
- [ ] **Step 4: Implement**, with the rule keys the spike accepted.
- [ ] **Step 5: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 6: Commit** (`feat: codemux placeholder window and its window rules`).

### Task 15: Lua for resize mode, Alt + Tab, and safe config values

**Files:**
- Modify: `src/luagen.rs`, `src/config.rs`
- Test: `src/luagen.rs`, `src/config.rs`

**Interfaces:**
- Produces:
  - `luagen::lua_str(s: &str) -> String`: a quoted Lua string literal with `\` and
    `"` escaped; every interpolated value (exe, keys, descriptions, args) goes
    through it (carry-over: Lua escaping).
  - `leader_key` (`src/luagen.rs:13`) also maps `.` `period`, `/` `slash`,
    `;` `semicolon`, `'` `apostrophe`, `-` `minus`, `=` `equal`, `[` `bracketleft`,
    `]` `bracketright`, `\` `backslash`, `` ` `` `grave` (carry-over: `period`).
  - `config::apply_line` (`src/config.rs:140`): a `bind` key must be one ASCII letter
    or digit, one of `$ : , . / ; ' - = [ ] \ ``, or an xkb name of letters, digits
    and `_`; anything else (quotes, spaces) is an error
    `unsupported key \`<k>\``; `bind-global` keys and `set leader` must match
    `MOD(+MOD)*+KEY` with the same character rules; `set leader` with no value is
    the error `set leader needs keys, e.g. SUPER+B` (carry-over: misleading empty
    leader message).
  - Resize mode (spec 2 leader detail 6), generated:
    - file level: `codemux_resize_timer` = `hl.timer(<fn>, { timeout = 50, type = "repeat" })`
      created disabled; `<fn>` leaves resize mode when neither
      `hl.is_key_down("Super_L")` nor `hl.is_key_down("Super_R")` (pattern of
      `~/.config/hypr/bindings.lua:84-91`); `codemux_resize_enter()` stops the leader
      timer, enables the repeat timer, `hl.dispatch(hl.dsp.submap("codemux_resize"))`;
      `codemux_resize_exit()` disables it and resets the submap;
    - in the `codemux` submap: `SUPER + LEFT|RIGHT|UP|DOWN` with
      `{ repeating = true }`: enter resize mode, then resize;
    - `hl.define_submap("codemux_resize", ...)`: the same four SUPER + arrow binds
      (repeating) dispatching
      `hl.dsp.window.resize({ x = <dx>, y = <dy>, relative = true })` with
      `dx, dy` = `-step, 0` / `step, 0` / `0, -step` / `0, step` and
      `step = cfg.resize_step`; no-op binds for `Super_L`, `Super_R`,
      `SUPER + Super_L`, `SUPER + Super_R` (key repeat of the held Super must not hit
      the catch-all); `ESCAPE` and the catch-all call `codemux_resize_exit`
      (Ruling 10).
  - Alt + Tab takeover (spec 2 takeover 8):
    `o.rebind("ALT + TAB", "codemux next window", "<exe> cycle next")` and
    `o.rebind("ALT + SHIFT + TAB", "codemux previous window", "<exe> cycle prev")`
    (one `hl.unbind` removes both Omarchy binds per key, `docs/spikes.md` S2).

- [ ] **Step 1: Write failing tests**

```rust
// src/luagen.rs (replace alt_tab_not_taken_over_in_phase_2)
#[test] fn alt_tab_taken_over_once_per_key() {
    let g = lua();
    assert_eq!(g.matches("o.rebind(\"ALT + TAB\"").count(), 1);
    assert_eq!(g.matches("o.rebind(\"ALT + SHIFT + TAB\"").count(), 1);
    assert!(g.contains("codemux cycle next"));
}
#[test] fn resize_mode_uses_step_and_polls_super() {
    let (c, _) = parse("set resize-step 30\n"); let g = generate(&c, "x");
    assert!(g.contains("hl.define_submap(\"codemux_resize\""));
    assert!(g.contains("x = -30, y = 0, relative = true"));
    assert!(g.contains("hl.is_key_down(\"Super_L\")"));
    assert!(g.contains("repeating = true"));
}
#[test] fn period_key_is_named() { let (c, _) = parse("bind . tree\n"); assert!(generate(&c, "x").contains("o.bind(\"period\", \"codemux tree\"")); }
#[test] fn quotes_in_exe_are_escaped() { assert!(generate(&Config::default(), "/a\"b").contains("\"/a\\\"b daemon\"")); /* exact escaped form */ }
// src/config.rs
#[test] fn empty_leader_says_what_to_write() { let (_, e) = parse("set leader\n"); assert!(e[0].1.contains("SUPER+B")); }
#[test] fn quote_in_bind_key_is_rejected() { let (c, e) = parse("bind \" tree\n"); assert_eq!(e.len(), 1); assert_eq!(c, Config::default()); }
```

  Also keep `generated_lua_parses` (it now checks the resize submap through `luac -p`).
- [ ] **Step 2: Run** `cargo test luagen config`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: resize mode, alt-tab takeover, escaped config values`).

### Task 16: Placeholder jobs, new-workspace placeholders, Leader + `x`

**Files:**
- Create: `src/tiles.rs`
- Modify: `src/lib.rs`, `src/engine.rs` (`on_event` `OpenWindow` / `CloseWindow`,
  `maybe_delete`, `NewWorkspace`, `NewSession`, `enforce_invariant` step 3,
  `adopt_all`, `Action`), `src/hypr.rs` (`swap_windows`), `src/main.rs`
- Test: `tests/engine_tiles.rs` (new)

**Interfaces:**
- Consumes: `Hypr::spawn`, `live_mode` (Task 13), `next_deadline` / `on_deadline`
  (Task 7), `summon` (Task 8), `close_window` (Task 9).
- Produces:
  - `hypr::swap_windows(target, window) -> String` =
    `hl.dsp.window.swap({ target = "address:<t>", window = "address:<w>" })`
    (`docs/spikes.md` S4).
  - `pub(crate) enum JobKind { FillNew { target: i64 }, LeaveSpot { app: String, source: i64, target: i64, follow: bool }, Recreate { target: i64, after: Option<String>, size: (i32, i32), archive_index: Option<usize>, refocus: Option<String> } }`
  - `pub(crate) struct PlaceholderJob { pub pid: u32, pub kind: JobKind, pub deadline: Instant }`;
    engine fields `pub(crate) jobs: Vec<PlaceholderJob>`, `pub(crate) busy: HashSet<i64>`.
  - `pub(crate) fn spawn_placeholder(&mut self, kind: JobKind) -> Result<()>`:
    spawns `[exe, "placeholder"]`, inserts the pid into `spawned`, marks the job's
    workspaces `busy`, deadline 5 s.
  - `pub(crate) fn wants_placeholder(&self, id: i64) -> Result<bool>`:
    `live_mode(id)` is `dwindle` or `scrolling` (spec 1 item 5).
  - `on_event(OpenWindow)` is restructured: before the regular-id check, a window
    whose pid belongs to a job runs that job (the placeholder opens on the stash,
    whose name does not parse as an id). `FillNew`: `move_window(ph, target, false)`,
    record the placeholder tile on `target`, job done.
  - `maybe_delete` skips `busy` workspaces; a job finishing (or timing out in
    `on_deadline`) clears its `busy` ids and re-runs `maybe_delete` on them.
  - Placeholder in every new workspace (spec 5.7 "Where one appears"): `NewWorkspace`
    (Leader + `c`), `NewSession` (and so `new` in the command prompt), the `main` of
    `enforce_invariant` step 3, and an adoption `main` created for a monitor with no
    occupied workspace: if `wants_placeholder`, `spawn_placeholder(FillNew)`.
  - `Request::CloseWindow { address: String }` (`codemux close-window ADDR`):
    dispatch `close_window`.
  - `Action "kill-tile"` (spec 2, 5.6): focused window none: nothing; placeholder:
    `close_window` at once; app: summon
    `{"view":"confirm","text":"Kill <class>? (y/n)","run":["close-window","<addr>"]}`.
  - At `start()`: close placeholders found on `special:codemux-stash` (Ruling 13).

- [ ] **Step 1: Write failing tests** (fixture: one monitor, session 1, shown 100001
  with foot 0xa1; `e.exe = "cm"`; helper `open_ph(e, pid, addr)` pushes a
  `codemux-placeholder` client on `special:codemux-stash` (id -99) and sends
  `OpenWindow` with workspace name `special:codemux-stash`):

```rust
#[test] fn new_workspace_spawns_a_placeholder_and_fills_it() {
    /* NewWorkspace -> spawned == [["cm","placeholder"]]; open_ph(9000, "0xp") ->
       dispatched contains move_window("0xp", 100002, false); 100002 has one placeholder tile */ }
#[test] fn floating_default_gets_no_placeholder() {
    /* layouts dir: 100002.lua floating pre-written; NewWorkspace -> spawned empty */ }
#[test] fn new_session_workspace_starts_with_placeholder() { /* NewSession{"Work"} -> one spawn */ }
#[test] fn monitor_main_from_invariant_gets_placeholder() { /* MonitorAdded new monitor -> enforce -> one spawn */ }
#[test] fn busy_workspace_is_not_deleted_mid_job() {
    /* 100002 unshown, in busy; CloseWindow of its last tile -> still present; job deadline passes -> deleted */ }
#[test] fn kill_tile_on_placeholder_closes_at_once() { /* focused = placeholder 0xp -> dispatched == [close_window("0xp")], spawned empty */ }
#[test] fn kill_tile_on_app_asks_first() { /* focused 0xa1 -> summon payload text "Kill foot? (y/n)", run ["close-window","0xa1"]; no close dispatched */ }
#[test] fn closing_placeholder_of_empty_unshown_workspace_deletes_it() { /* spec 5.7 lifecycle */ }
#[test] fn stash_orphans_closed_at_start() { /* live placeholder on special:codemux-stash at start() -> close_window dispatched */ }
```

- [ ] **Step 2: Run** `cargo test --test engine_tiles`. Expected: FAIL.
- [ ] **Step 3: Implement** in `src/tiles.rs` (plus the named `engine.rs` hooks).
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: placeholders in new workspaces and leader x`).

### Task 17: Moving leaves a placeholder; filling placeholders

**Files:**
- Modify: `src/tiles.rs`, `src/engine.rs` (`Request::Move` branch, `on_event`
  `OpenWindow`)
- Test: `tests/engine_tiles.rs`

**Interfaces:**
- Produces:
  - `Request::Move { n, follow }` (spec 2 takeovers 2-3), replacing the body at
    `src/engine.rs:174-187`: no active window, active window is a placeholder, or the
    target is the workspace it is already on: nothing. Missing target number:
    created (existing behaviour, user decision). If `wants_placeholder(source)`:
    `spawn_placeholder(LeaveSpot { app, source, target, follow })`; else
    `deliver(app, target, follow)` directly.
  - `LeaveSpot` on the placeholder's open: `swap_windows(ph, app)` (the placeholder
    takes the app's spot, the app goes to the stash), update both tile records
    directly (idempotent with any `movewindowv2` the swap emits, spike Task 14),
    then `deliver(app, target, follow)`. Placeholder never appears within 5 s:
    `deliver` anyway, log it.
  - `pub(crate) fn deliver(&mut self, app: &str, target: i64, follow: bool) -> Result<()>`:
    if `wants_placeholder(target)` and the target holds placeholder tiles, take the
    first by `reading_key` of its recorded geometry: `swap_windows(ph, app)`,
    `close_window(ph)`, and `focus_window(app)` when `follow`; otherwise
    `move_window(app, target, follow)`.
  - Fill-on-open (spec 5.7 "Filling one"): `OpenWindow` of a window that is not a
    placeholder, whose pid is not in `spawned`, while no restore runs (add
    `pub fn restoring(&self) -> bool` here returning `false`; Task 23 gives it its
    real body), on a known regular workspace W, live `floating == false`,
    and the window focused just before it (`focused`, or `prev_focused` if `focused`
    is the new window, Ruling 15) is a placeholder tile on W:
    `swap_windows(ph, new)`, `close_window(ph)`.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn move_from_dwindle_leaves_placeholder_then_moves_app() {
    /* active 0xa1 on shown 100001 (dwindle), Move{2,follow:true} -> spawn; open_ph ->
       dispatched == [swap_windows("0xp","0xa1"), move_window("0xa1",100002,true)];
       100001 records the placeholder, 100002 records 0xa1 */ }
#[test] fn move_from_floating_leaves_nothing() { /* 100001.lua floating -> no spawn; dispatched == [move_window(..)] */ }
#[test] fn move_into_workspace_with_placeholder_fills_top_left_first() {
    /* source floating; 100002 holds placeholders at (900,0) and (0,0): swap with the (0,0) one, then close it */ }
#[test] fn move_on_placeholder_does_nothing() { /* active is a placeholder -> no spawn, no dispatch */ }
#[test] fn move_to_own_workspace_does_nothing() { /* Move{1} while on 1 -> nothing */ }
#[test] fn source_kept_while_app_swaps_through_the_stash() {
    /* source unshown (silent move from tree context): MoveWindow(0xa1 -> stash) arrives before the placeholder's move: source not deleted */ }
#[test] fn opened_app_takes_focused_placeholder_spot() {
    /* focused 0xp (placeholder on 100001), new 0xn opens tiled on 100001 -> [swap_windows("0xp","0xn"), close_window("0xp")] */ }
#[test] fn opened_app_after_focus_event_still_fills() { /* ActiveWindow{0xn} arrives before OpenWindow: prev_focused 0xp is used */ }
#[test] fn dialog_or_spawned_or_other_workspace_never_fills() {
    /* floating client; pid in spawned; placeholder on another workspace: no swap in each case */ }
```

- [ ] **Step 2: Run** `cargo test --test engine_tiles`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: moving apps leaves and fills placeholders`).

### Task 18: Mark, swap and Alt + Tab cycling

**Files:**
- Modify: `src/model.rs` (`State.mark` already added in Task 3), `src/tiles.rs`,
  `src/engine.rs` (`Action`, `CloseWindow` event), `src/hypr.rs`, `src/main.rs`
- Test: `tests/engine_tiles.rs`

**Interfaces:**
- Produces:
  - `hypr::bring_to_top() -> String` = `hl.dsp.window.bring_to_top()`;
    `hypr::lower_to_bottom() -> String` = `hl.dsp.window.alter_zorder({ mode = "bottom" })`
    (forms from `tiling.lua:57,60`).
  - `Action "mark"`: toggles `state.mark` on the focused window (placeholders
    allowed); dirty (spec 6.1). The marked window closing clears the mark.
  - `Action "swap"`: Ruling 9; else `swap_windows(mark, focused)`, swap the two tile
    records (workspace and geometry), clear the mark.
  - `Request::Cycle { forward: bool }` (`codemux cycle next|prev`), Ruling 11:
    candidates = clients on the focused monitor's `active_workspace`, not hidden, not
    placeholders, in `hyprctl` order; start from the active window's index in the
    unfiltered list; next / previous with wrap. Forward: `focus_window(next)`, then
    `bring_to_top()`. Backward: `lower_to_bottom()`, then `focus_window(prev)`.
    Fewer than two candidates: nothing.
  - After this task no action answers "not in this version yet"; an unknown action
    name answers `codemux: unknown action <name>` (update
    `unimplemented_action_says_so` into `unknown_action_says_so`).

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn mark_toggles_and_shows_in_view() { /* mark 0xa1 -> state.mark Some, view.marked true; mark again -> None */ }
#[test] fn swap_with_mark_across_workspaces() {
    /* mark 0xa2 (100002), focus 0xa1 (100001), swap -> [swap_windows("0xa2","0xa1")]; records exchanged; mark None */ }
#[test] fn swap_without_mark_notifies() { /* -> !ok, message "codemux: no marked tile", no dispatch */ }
#[test] fn swap_never_closes_a_placeholder() { /* marked placeholder: one swap dispatched, no close */ }
#[test] fn closing_marked_window_clears_mark() {}
#[test] fn alt_tab_skips_placeholders_and_raises() {
    /* clients on 100001 in order [0xa1, 0xp, 0xa3], active 0xa1: Cycle{true} -> [focus_window("0xa3"), bring_to_top()] */ }
#[test] fn alt_shift_tab_lowers_then_focuses_previous_with_wrap() {
    /* active 0xa1 first -> [lower_to_bottom(), focus_window("0xa3")] */ }
#[test] fn unknown_action_says_so() {}
```

- [ ] **Step 2: Run** `cargo test --test engine_tiles --test engine_commands`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: mark, swap and alt-tab without placeholders`).

### Task 19: Archiving placeholders on floating, recreating them, restart rules

**Files:**
- Modify: `src/model.rs`, `src/geometry.rs`, `src/tiles.rs`, `src/engine.rs`
  (`on_event` `FloatingChanged`, `maybe_delete`, `start` Restarted branch),
  `src/hypr.rs` (`resize_exact`), `src/overlay.rs` (kills drop archives)
- Test: `tests/engine_geometry.rs`

**Interfaces:**
- Produces:
  - `pub struct ArchivedPlaceholder { pub index: usize, pub after: Option<String>, pub at: (i32, i32), pub size: (i32, i32) }`
    (`Serialize, Deserialize, Clone, Debug, PartialEq`);
    `Workspace.archived: Vec<ArchivedPlaceholder>` (`#[serde(default)]`).
  - `hypr::resize_exact(addr, w, h) -> String` in the form Task 14's spike recorded.
  - `pub(crate) fn archive(&mut self, id: i64) -> Result<()>` (spec 5.7
    "Archiving"): placeholders of `id` in `reading_key` order of their **recorded**
    geometry (never a fresh read); `after` = address of the tile just before each in
    that order among all of the workspace's recorded tiles; push the entries, remove
    the placeholder tile records, dispatch `close_window` for each, set
    `mode = "floating"`, dirty.
  - `pub(crate) fn recreate_archived(&mut self, id: i64) -> Result<()>`: one
    `Recreate` job per entry, in `index` order, each carrying `refocus` = the
    currently focused window (Ruling 14). On open: `focus_window(after)` if `after`
    is live on `id`, `move_window(ph, id, false)`, `resize_exact(ph, size)`, focus
    `refocus` back, remove that archive entry, record the tile.
  - Triggers: `FloatingChanged { floating: true }` for a placeholder tile whose
    workspace's `live_mode` is `floating` -> `archive`. `FloatingChanged { floating: false }`
    for a window on a workspace with archives, or `check_modes` finding such a
    workspace back in `dwindle` / `scrolling` -> `recreate_archived`.
    `check_modes` also archives a workspace that reads `floating` while it still has
    placeholder tiles (the 30 s fallback, spec 5.7 "Noticing").
  - `maybe_delete` treats a workspace with archives as not empty (spec 5.6).
    `KillSession` / `KillWorkspace` clear `archived` (spec 5.6).
  - Restart rules (spec 3.2.8, same signature): before `clear_tiles`, collect saved
    placeholder tiles; after `record_live_windows`, those with no live window
    (address and pid) are recreated (`Recreate`, `after` from saved order) when the
    workspace's live mode is `dwindle` / `scrolling`, and archived from the saved
    geometry when it is `floating`.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn floating_switch_archives_from_recorded_geometry() {
    /* 100001 dwindle: foot (0,0) and placeholder (960,0) recorded; live clients now floated to (400,300) 1100x716;
       write 100001.lua floating; FloatingChanged{0xp,true} -> archived == [{index:0, after:Some("0xa1"), at:(960,0), size:<recorded>}];
       dispatched contains close_window("0xp"); no tile record for 0xp */ }
#[test] fn archived_workspace_is_not_deleted() { /* unshown, only archives, last app closes -> still present */ }
#[test] fn back_to_tiled_recreates_next_to_neighbour_and_refocuses() {
    /* archives on 100001, file removed (dwindle), FloatingChanged{0xa1,false} -> spawn; open_ph ->
       [focus_window("0xa1"), move_window("0xp2",100001,false), resize_exact("0xp2",..), focus_window(<previously focused>)]; archived empty */ }
#[test] fn thirty_second_check_archives_when_event_missed() { /* tick with file floating and placeholder tiles -> archived */ }
#[test] fn kill_drops_archives() {}
#[test] fn restart_recreates_missing_placeholder_on_tiled_workspace() { /* saved placeholder, no live window, mode dwindle -> one spawn */ }
#[test] fn restart_archives_missing_placeholder_on_floating_workspace() { /* mode floating -> archived, no spawn */ }
```

- [ ] **Step 2: Run** `cargo test --test engine_geometry`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: archive placeholders on floating workspaces and recreate them`).

### Task 20: Install and uninstall for tiles

**Files:**
- Modify: `src/install.rs`, `packaging/codemux-uninstall.sh`
- Test: `src/install.rs`

**Interfaces:**
- Produces:
  - `pub const SWITCHER_FILTER: &str = "map(select(.class != \"codemux-placeholder\")) | ";`
  - `pub fn add_switcher_filter(text: &str) -> Option<String>`: inserts the filter
    right before `sort_by(.workspace.id)[]` in the jq query
    (`~/.local/bin/window-switcher:5-7`); already present -> unchanged; anchor missing
    -> `None` (install then prints that the script changed and skips, spec 10 keeps
    the script otherwise untouched).
  - `pub fn remove_switcher_filter(text: &str) -> String`.
  - `install::install` applies it with `write_atomic`, keeping the file mode.
  - Uninstall script, spec 8.2 step 1: after stopping the daemon,
    `hyprctl -j clients | jq -r '.[] | select(.class == "codemux-placeholder") | .pid' | xargs -r kill`;
    step 3: `sed -i 's/map(select(.class != "codemux-placeholder")) | //' ~/.local/bin/window-switcher`
    when the file exists.

- [ ] **Step 1: Write failing tests**

```rust
const SW: &str = "mapfile -t rows < <(hyprctl clients -j | jq -r '\n  sort_by(.workspace.id)[]\n  | \"x\"')\n";
#[test] fn switcher_filter_round_trips() {
    let once = add_switcher_filter(SW).unwrap();
    assert!(once.contains("map(select(.class != \"codemux-placeholder\")) | sort_by(.workspace.id)[]"));
    assert_eq!(add_switcher_filter(&once).unwrap(), once);
    assert_eq!(remove_switcher_filter(&once), SW);
}
#[test] fn changed_switcher_is_left_alone() { assert_eq!(add_switcher_filter("echo hi\n"), None); }
#[test] fn uninstaller_closes_placeholders_and_removes_filter() {
    assert!(UNINSTALLER.contains("codemux-placeholder")); assert!(UNINSTALLER.contains("window-switcher"));
}
```

- [ ] **Step 2: Run** `cargo test install`. Expected: FAIL.
- [ ] **Step 3: Implement**; `bash -n packaging/codemux-uninstall.sh`.
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: hide placeholders from the window switcher`).

### Task 21: Phase 5 live acceptance (user)

**Files:** none (results under "Phase 5 acceptance" in `docs/spikes.md`).

- [ ] **Step 1:** Install the release build. `grep -c codemux-placeholder ~/.local/bin/window-switcher` prints `1`.
- [ ] **Step 2:** Leader + `c`: a new workspace with an invisible placeholder (bar
  shows its number only); open foot: it fills the whole workspace; Leader + `x` on
  foot asks `Kill foot? (y/n)`.
- [ ] **Step 3:** On a dwindle workspace with three tiles, Super + Shift + 2 on one:
  the other two do not move; the gap stays; Super + Shift + 1 from elsewhere fills
  the gap exactly. Same on a scrolling workspace. Super + Shift + Alt + 2: same,
  without switching.
- [ ] **Step 4:** Leader + `q` then the placeholder's number, Leader + `x`: it
  closes and the others fill the space. Leader + `m` on one tile (`MARK` shows),
  Leader + `v` on a tile of another monitor: they swap; `MARK` goes.
- [ ] **Step 5:** Leader then Super + arrows (B released, Super held): the tile
  resizes in 50 px steps while held and repeating; releasing Super shows the
  `RESIZE` marker gone.
- [ ] **Step 6:** Alt + Tab / Alt + Shift + Tab never land on a placeholder;
  Super + Shift + K lists no placeholder.
- [ ] **Step 7:** Super + L to floating on a workspace with a placeholder: it
  vanishes (archived) and the workspace stays; Super + L back: it returns near its
  old spot. `pkill -x codemux; hyprctl reload`: placeholders survive the restart.
- [ ] **Step 8:** Run the uninstaller: no `codemux-placeholder` window remains, the
  switcher filter is gone; reinstall. Record pass/fail per step.

---

## Phase 6: Relaunch, then layout restore

### Task 22: Launch records and the restore planner

**Files:**
- Create: `src/relaunch.rs`
- Modify: `src/model.rs`, `src/hypr.rs`, `tests/common/mod.rs`, `src/engine.rs`
  (`tile_for`), `src/lib.rs`
- Test: `src/relaunch.rs`, `tests/engine_relaunch.rs` (new)

**Interfaces:**
- Produces:
  - `pub struct ProcInfo { pub cmdline: Vec<String>, pub cwd: Option<String> }`
    (`Default`); `Hypr::proc_info(&self, pid: u32) -> ProcInfo` (`Hyprctl`: reads
    `/proc/<pid>/cmdline` split on NUL and `readlink /proc/<pid>/cwd`; empty on
    error); `FakeHypr.procs: RefCell<HashMap<u32, ProcInfo>>`.
  - `Tile.cmdline: Vec<String>`, `Tile.cwd: Option<String>` (`#[serde(default)]`),
    filled by `Engine::tile_for` (`src/engine.rs:313`) whatever the relaunch toggle
    says (spec 6.5, 7.2).
  - `pub enum LaunchCmd { Argv(Vec<String>), Shell(String) }`
  - `pub fn launch_command(tile: &Tile, overrides: &BTreeMap<String, String>) -> Option<LaunchCmd>`
    (override for the class wins as `Shell`; else non-empty cmdline as `Argv`;
    placeholders and empty cmdlines -> `None`; Ruling 18).
  - `pub enum RestoreStep { SetMode { workspace: i64, mode: String }, Open { workspace: i64, tile: Tile }, Layout { workspace: i64 } }`
  - `pub fn plan(state: &State) -> Vec<RestoreStep>`: for each workspace in
    (session number, workspace number) order: `SetMode` (recorded mode, default
    `dwindle`), one `Open` per saved tile in `reading_key` order, then `Layout`.
  - `pub fn claim<'a>(tile: &Tile, live: &'a [HClient], claimed: &HashSet<String>, procs: &dyn Fn(u32) -> ProcInfo) -> Option<&'a HClient>`:
    an unclaimed live window of the same class and the same cmdline (spec 7.3
    "Claim before launch").

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn override_beats_cmdline() { /* class steam, override "steam -silent" -> Shell("steam -silent") */ }
#[test] fn cmdline_runs_as_argv() { /* ["foot","--app-id","x"] -> Argv(..) */ }
#[test] fn placeholder_has_no_launch_command() {}
#[test] fn plan_orders_mode_then_tiles_in_reading_order_then_layout() {
    /* ws 100001 scrolling with tiles at (900,0),(0,0): [SetMode(100001,"scrolling"), Open((0,0)), Open((900,0)), Layout(100001)] */ }
#[test] fn claim_needs_same_class_and_cmdline_and_unclaimed() {}
// tests/engine_relaunch.rs
#[test] fn new_tiles_record_cmdline_and_cwd() { /* procs[31] = {["kitty"], Some("/home/u")} ; OpenWindow 0xc1 pid 31 -> tile.cmdline == ["kitty"], cwd Some */ }
```

- [ ] **Step 2: Run** `cargo test relaunch --test engine_relaunch`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: record launch commands and plan the restore`).

### Task 23: The restore queue (apps and placeholders, one at a time)

**Files:**
- Modify: `src/relaunch.rs`, `src/engine.rs` (`start` HyprlandStarted branch,
  `on_event` `OpenWindow`, `record_live_windows`, `on_deadline`), `src/tiles.rs`
  (fill-on-open guard)
- Test: `tests/engine_relaunch.rs`

**Interfaces:**
- Produces:
  - `pub(crate) struct Restore { pub steps: VecDeque<RestoreStep>, pub waiting: Option<Waiting>, pub claimed: HashSet<String>, pub placed: Vec<(i64, String, Tile)> }`
  - `pub(crate) struct Waiting { pub workspace: i64, pub tile: Tile, pub pid: u32, pub known_before: HashSet<String>, pub deadline: Instant }`
  - Engine field `pub(crate) restore: Option<Restore>`; `pub fn restoring(&self) -> bool`.
  - `start()` HyprlandStarted branch (`src/engine.rs:761`), when `config.relaunch`:
    `plan` from the saved tiles, then `clear_tiles`, `land` (existing, first, spec
    3.2.8 "Landing"), then `begin_restore`; `record_live_windows` is **not** called
    yet. Relaunch off: unchanged (clear, land, record).
  - `fn advance_restore(&mut self) -> Result<()>`, one step at a time:
    `SetMode`: `eval(set_mode(..))` and write or delete `<id>.lua` exactly as Omarchy's
    toggle does (delete when the mode equals `omarchy_default_mode()` or, with no
    default, `dwindle`; spec 3.2.2); `Open` of a placeholder:
    `spawn_placeholder(Recreate { target, after: None, size, .. })` and wait for that
    job; `Open` of an app: `claim` first (move the claimed window with
    `move_window(addr, ws, false)`, record the saved tile with the live address and
    pid); else spawn its `launch_command` (`Argv` with `cwd`, or `["sh","-c",s]`),
    add the pid to `spawned`, set `Waiting` with a 15 s deadline; no launch command:
    log and skip; `Layout`: Task 24 (no-op here).
  - `OpenWindow` while `Waiting`: accept the window if its pid equals the spawned pid,
    or its class equals the tile's class and its address is neither claimed nor in
    `known_before` (spec 7.3); then `move_window(addr, ws, false)`, record, claim,
    advance. Deadline passes: Ruling 19 notification, advance.
  - Other `OpenWindow`s during a restore are not recorded (the final sweep records
    them); fill-on-open is off; all restoring workspaces are `busy`.
  - `finish_restore`: clear `busy`, then `record_live_windows`, which now first
    removes an existing record of the same address from any workspace (so a window
    recorded during the restore is never duplicated).

- [ ] **Step 1: Write failing tests** (fixture: saved state with an older signature,
  `config.relaunch = true`, ws 100001 tiles foot (cmdline `["foot"]`) and a
  placeholder, ws 100002 tile kitty; live: no clients):

```rust
#[test] fn hyprland_start_lands_before_launching() { /* first dispatches are focus_monitor/focus_workspace; first spawn after them */ }
#[test] fn launches_one_tile_at_a_time_in_order() {
    /* after start(): spawned == [["foot"]]; open foot (pid 9000) -> move_window(..,100001,false), then the placeholder spawn, then kitty */ }
#[test] fn claims_autostarted_window_instead_of_launching() {
    /* live kitty already open with cmdline ["kitty"]: no kitty spawn; move_window(<kitty>,100002,false) */ }
#[test] fn same_class_window_accepted_only_if_new_and_unclaimed() {}
#[test] fn gives_up_after_15s_and_moves_on() { /* on_deadline(now+16s) -> notified contains "foot"; next step spawned */ }
#[test] fn windows_opened_during_restore_recorded_afterwards_once() {
    /* user opens 0xu on 100001 mid-restore: no tile yet; after the last step: exactly one record of 0xu */ }
#[test] fn no_fill_on_open_during_restore() { /* focused placeholder + new user window mid-restore: no swap */ }
#[test] fn relaunch_off_restores_nothing_and_keeps_empty_workspaces() {
    /* relaunch false: no spawn; 100001 and 100002 still in state with no tiles; landing done */ }
#[test] fn daemon_restart_never_relaunches() { /* same signature: no spawn */ }
#[test] fn set_mode_step_writes_or_deletes_mode_file() { /* scrolling -> 100001.lua written; dwindle default -> file deleted */ }
```

- [ ] **Step 2: Run** `cargo test --test engine_relaunch`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: relaunch apps and placeholders on hyprland start`).

### Task 24: Layout restore

**Files:**
- Modify: `src/relaunch.rs`, `src/hypr.rs` (`move_exact`)
- Test: `tests/engine_relaunch.rs`

**Interfaces:**
- Produces:
  - `hypr::move_exact(addr, x, y) -> String` in the form Task 14's spike recorded
    (`resize_exact` exists from Task 19).
  - `RestoreStep::Layout { workspace }` (spec 7.3): by the workspace's recorded mode:
    `dwindle`: `resize_exact(addr, saved size)` for each placed tile in saved order;
    `floating`: `move_exact(addr, saved at)` then `resize_exact(addr, saved size)`;
    `scrolling`: nothing (order only; column widths are a spec 7.4 limit).
  - After the last `Layout`, `finish_restore` (Task 23).

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn dwindle_layout_resizes_to_saved_sizes() { /* after both tiles placed: resize_exact for each, in saved order */ }
#[test] fn floating_layout_moves_and_resizes() { /* move_exact then resize_exact per tile */ }
#[test] fn scrolling_layout_only_keeps_order() { /* no resize/move dispatched for the scrolling workspace */ }
```

- [ ] **Step 2: Run** `cargo test --test engine_relaunch`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: restore tiled sizes and floating positions`).

### Task 25: Phase 6 live acceptance (user)

**Files:** none (results under "Phase 6 acceptance" in `docs/spikes.md`).

- [ ] **Step 1:** Install the release build. Set up: two sessions, a dwindle
  workspace with foot (in `~/Projects`) and a browser split unevenly, a placeholder
  gap, a scrolling workspace with three terminals, a floating workspace with one
  window moved off-center. Wait 30 s (geometry capture).
- [ ] **Step 2:** Log out and back in (a new Hyprland). Expected: each monitor lands
  on its active session; apps come back one by one into their workspaces; the foot
  opens in `~/Projects`; the dwindle split is close to the saved sizes; the gap is a
  placeholder again; the scrolling order is kept; the floating window is back at its
  spot; an app Omarchy autostarts is claimed, not doubled.
- [ ] **Step 3:** An app that cannot relaunch (e.g. a Flatpak) produces the
  "did not come back; a launch-override line may help" notification after 15 s;
  add a `launch-override` line, log out and in again: it returns.
- [ ] **Step 4:** Leader + `o`, relaunch off, log out and in: sessions, names and
  empty workspaces come back, no app is started, each monitor still lands on its
  session. Switch relaunch back on.
- [ ] **Step 5:** Record pass/fail per step.

---

## Self-review (done while writing)

- Spec coverage: 3 item 3 (service, barWidget, menu) Tasks 4, 10, 11; 3.1 stream,
  retry, dim label, CLI from overlays Tasks 3, 4, 9; 3.2.8 restart rules for
  placeholders Task 19, relaunch start Task 23; 3.2.11 mode reads Task 13; 4.1-4.6
  Tasks 3, 4; 5.1 Tasks 9, 10; 5.2-5.4 Tasks 9, 10; 5.5 Tasks 8, 11; 5.6 kills Task 9,
  tile close Task 16, archives keep workspaces Task 19; 5.7 Tasks 14, 16, 17, 19, 20;
  2 table (`m`, `v`, `x`, arrows) Tasks 15, 16, 18; 2 takeovers 2-3 placeholders
  Task 17, takeover 8 Tasks 15, 18; 6.1 mark and archives in state Tasks 3, 18, 19;
  6.2 Task 13; 6.5, 7.1-7.5 Tasks 13, 22-24; 8.1 / 8.2 Tasks 5, 20.
- Names used consistently: `View`, `MonitorView`, `WsView`, `Tree*`, `summon_argv`,
  `number_panes`, `pane_key`, `KillJob`, `JobKind`, `PlaceholderJob`,
  `spawn_placeholder`, `wants_placeholder`, `deliver`, `live_mode`, `check_modes`,
  `capture_geometry`, `archive`, `recreate_archived`, `ArchivedPlaceholder`,
  `ProcInfo`, `LaunchCmd`, `RestoreStep`, `Restore`, `Waiting`, `restoring`,
  `PLACEHOLDER_CLASS`, `reading_key`, `write_atomic`, `register_watcher`,
  `loop_timeout`.
