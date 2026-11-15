# codemux Core Implementation Plan (phases 1-2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove the risky Hyprland mechanics (spikes S1-S4), then ship a working
codemux core: daemon, CLI, generated `codemux.lua`, state file, adoption, monitor
handling, session-local workspace keys, `~/.codemux.conf`, install and uninstall.

**Architecture:** One Rust binary. `codemux daemon` owns all state in a single-threaded
`Engine` fed by a channel (socket2 events, IPC requests, a 30 s tick, config changes);
every other subcommand is a thin client over a Unix socket. Hyprland is reached only
through a `Hypr` trait, so the engine is unit-tested against a fake. Keys live in a
`codemux.lua` generated from `~/.codemux.conf`.

**Tech Stack:** Rust 1.99.1 (edition 2024), std threads and channels (no async),
`anyhow`, `serde`, `serde_json`, `libc` (flock). Lua (Hyprland 0.56.2 config API).
Bash for the uninstall script.

**Spec:** `docs/spec.md` (draft 13). Executors read both; section numbers below
(`spec 3.2.8`) point into it.

**Scope:** spec section 9 phases 1 (spikes) and 2 (core). Phases 3-6 (bar widget,
overlays, tiles and placeholders, relaunch) get their own plans after phase 2 passes
its live acceptance (Task 13). Leader actions those phases implement are bound in
`codemux.lua` already but answer with a "not in this version yet" notification.
Deferred on purpose, with Omarchy's behaviour kept meanwhile:
- Alt + Tab takeover (spec 2 takeover 8) moves to phase 5: it exists only to skip
  placeholders, which phase 5 introduces. Omarchy's Alt + Tab binds stay untouched.
- Periodic tile-geometry capture (spec 6.2) is deferred: phase 5's placeholder
  archiving (spec 5.7) and phase 6's relaunch both read it, so the phase 5 plan must
  build it first. Phase 2 records geometry only when a tile is created.

## Global Constraints

- No em dash character anywhere: code, comments, docs, commit messages.
- Never edit files under `/usr/share/omarchy/`. Omarchy behaviour is changed only from
  `~/.config/hypr/codemux.lua` (spec 3.2.1).
- Binary name `codemux`, installed to `~/.local/bin/codemux`.
- Paths: socket `$XDG_RUNTIME_DIR/codemux.sock`, lock `$XDG_RUNTIME_DIR/codemux.lock`,
  state `~/.local/state/codemux/state.json`, config `~/.codemux.conf`, generated
  `~/.config/hypr/codemux.lua`, uninstaller `~/Scripts/AppUninstallers/codemux-uninstall.sh`.
- Real workspace ids: monitor slot `k` (1-based, first seen = 1) owns
  `k*100000+1 ..= k*100000+99999` (spec 3.2.2).
- Session-local workspace numbers start at 1, never 0. Session numbers are unique across
  monitors and never change (spec 1).
- Leader default `SUPER + B`; leader timeout 2000 ms; display-panes 2000 ms; resize step
  50 px; relaunch default on (spec 2, 6.3).
- Number-row binds use Omarchy's exact specs `SUPER + code:10` .. `code:19`
  (`tiling.lua:23-28`).
- State is written atomically (temp file + rename) (spec 6.1).
- Lint and typecheck after every task: `cargo clippy --all-targets -- -D warnings` and
  `cargo test` must pass.
- Commits only after the user has approved committing for this project (global rule);
  until then, "Commit" steps stage and stop. The folder is not a git repository yet:
  the first commit step asks the user to approve `git init` first.

## Review Focus

1. Second monitor with no sessions yet (first plug): must get session `main` with
   workspace 1, never crash or leave the bar empty. Test in Task 7.
2. Super + N where the session has no workspace N: nothing happens, no workspace is
   created (spec 2 takeover 1). Test in Task 6.
3. Daemon not running when a key is pressed: CLI exits non-zero and sends a desktop
   notification, never fails silently (spec 3.1). Test in Task 10.
4. `~/.codemux.conf` with a line codemux cannot parse: that line is skipped and named,
   the rest still applies, defaults fill the gaps (spec 6.3). Test in Task 3.
5. Adopting a workspace that has a non-default layout mode file (the user's 1 and 2 are
   `scrolling` today): it must stay `scrolling` after renumbering (spec 3.2.3). Test in
   Task 8.

---

## Phase 1: Spikes (live desktop, no product code)

Spikes run on the user's real session. Each writes its findings into
`docs/spikes.md` under its own heading. Every spike step that changes desktop state is
reversible and states how to revert. Phase 2 tasks read `docs/spikes.md` for the
values marked "from S1-S4" below.

### Task 1: Spikes S1-S4

**Files:**
- Create: `docs/spikes.md`
- Create (scratch, deleted at the end): `spike/codemux-spike.lua`

**Interfaces:**
- Produces, in `docs/spikes.md`: exact socket2 line formats for `openwindow`,
  `closewindow`, `movewindowv2`, `createworkspacev2`, `destroyworkspacev2`,
  `moveworkspacev2`, `monitoraddedv2`, `monitorremovedv2`, `workspacev2`,
  `configreloaded`, the floating-change event; whether `hl.unbind` removes every bind
  on a key spec; whether a catch-all bind works in a submap and its exact Lua form;
  whether `hl.exec_cmd` at top level runs on load and on reload; whether
  `hl.dsp.window.swap` works across workspaces and monitors.

- [ ] **Step 1: S3 first (it gates the rest).** Write `spike/codemux-spike.lua`
  containing only `hl.exec_cmd("sh -c 'echo $HYPRLAND_INSTANCE_SIGNATURE >> /tmp/codemux-s3.log'")`,
  add `require("hypr.codemux-spike")` after `require("hypr.bindings")` in
  `~/.config/hypr/hyprland.lua` (symlink the file into `~/.config/hypr/`), run
  `hyprctl reload` twice.
  Expected: `/tmp/codemux-s3.log` has 2 lines, each the live signature
  (`ls $XDG_RUNTIME_DIR/hypr/`). Record result. If 0 lines, record "S3 failed: start
  must move to hooks" (spec 3 item 2 fallback).

- [ ] **Step 2: S1 reconcile.** Add `o.workspaces.reconcile = function() end` to the
  spike file, `hyprctl reload`. Create `hyprctl dispatch 'hl.dsp.focus({ workspace = "100001" })'`,
  open a terminal there, `hyprctl reload` again.
  Expected: `hyprctl -j workspaces | jq '.[].id'` still lists `100001`. Then run
  `socat -u UNIX-CONNECT:$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock -`
  in a terminal, and do: open a window, close it, move a window to another workspace,
  unplug and replug DP-1, run
  `hyprctl dispatch 'hl.dsp.workspace.change_id({ workspace = "100001", id = 100002 })'`.
  Record every line verbatim, in order, per action. Revert: move the terminal back,
  remove the reconcile line, `hyprctl reload`.

- [ ] **Step 3: S2 submap.** In the spike file define a submap `spike` with a bare
  `c` bind and a `SUPER + c` bind (both `notify-send c`), a catch-all bind
  (try `hl.bind("catchall", ...)` inside `hl.define_submap`; the binary says
  "catchall keybinds are only allowed in submaps"), and `ESCAPE` reset; bind
  `SUPER + B` to enter it. Also: bind `SUPER + F9` twice to two notify commands, call
  `hl.unbind("SUPER + F9")` once, reload, press `SUPER + F9`.
  Expected and recorded: which presses fire; exact working catch-all syntax; whether
  one `hl.unbind` removed both F9 binds; and what `hl.unbind` returns, both when a
  bind was removed and when none was left (log it with `print` or `notify-send`).
  Timer probe: create one `hl.timer(..., { timeout = 3000, type = "oneshot" })`
  that runs `notify-send timer`, disable it, then from a bind re-enable it twice 2 s
  apart; record whether the notification comes 3 s after the second enable (countdown
  restarts) or after the first.

- [ ] **Step 4: S4 swap.** Open two terminals on workspace A (dwindle) and one on
  workspace B (scrolling, via Super + L). Run
  `hyprctl dispatch 'hl.dsp.window.swap({ target = "address:<A1>", window = "address:<B1>" })'`
  (try `with`/`other` if `target` is rejected; the binary lists
  "target/with/other").
  Expected and recorded: whether the windows swapped workspaces and each took the
  other's geometry (compare `hyprctl -j clients` before/after). Repeat with one window
  on `special:codemux-stash` and with A and B on different monitors. Also run
  `hyprctl dispatch 'hl.dsp.focus({ workspace = "B" })'` then Super + L to floating
  while the socket2 listener runs; record the floating-change event line.
  Revert: close the test terminals; press Super + L on workspace B until it is back to
  its previous mode, or delete B's file in `~/.local/state/omarchy/workspace-layouts/`
  if B had none before (note which before starting); remove the stash workspace by
  closing its window. Verify `cat ~/.local/state/omarchy/workspace-layouts/1.lua
  ~/.local/state/omarchy/workspace-layouts/2.lua` still both say `scrolling`.

- [ ] **Step 5: Clean up and verify revert.** Remove the spike `require` line and the
  symlink, `hyprctl reload`, delete `spike/`.
  Expected: `grep -c codemux ~/.config/hypr/hyprland.lua` prints `0`;
  `hyprctl -j binds | jq '[.[] | select(.key=="F9")] | length'` prints `0`.

- [ ] **Step 6: Record verdicts.** `docs/spikes.md` ends with one line per spike:
  `S1: pass|fail - <note>` etc. Any fail: stop and report to the user before Phase 2.

- [ ] **Step 7: Commit** `docs/spikes.md` (message `docs: record spike results S1-S4`).

---

## Phase 2: Core

### File structure

```
Cargo.toml
src/main.rs          CLI entry: parses argv, runs a subcommand
src/model.rs         State, Monitor, Session, Workspace, Tile; id allocation
src/store.rs         load / atomic save of state.json
src/config.rs        ~/.codemux.conf parser and defaults
src/luagen.rs        codemux.lua generator (pure)
src/hypr.rs          Hypr trait, hyprctl-backed impl, Lua expression builders
src/events.rs        socket2 line parser -> Event
src/engine.rs        Engine: requests, events, invariant, adoption, start kinds
src/layout.rs        layout-mode reader and mode-file mover
src/ipc.rs           socket server, client (Request/Response live in engine.rs)
src/daemon.rs        threads, channel loop, lock, exit rules, watch broadcast
src/install.rs       install command
packaging/codemux-uninstall.sh
tests/common/mod.rs  FakeHypr used by engine tests
```

### Task 2: Project scaffold and model

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/model.rs`, `.gitignore` (`/target`)
- Test: `src/model.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces:
  - `pub struct State { pub version: u32, pub signature: Option<String>, pub monitors: Vec<Monitor>, pub sessions: BTreeMap<u32, Session>, pub workspaces: BTreeMap<i64, Workspace> }`
    (all `Serialize, Deserialize, Clone, Debug, PartialEq`; `version` = 1)
  - `pub struct Monitor { pub key: String /* description */, pub slot: u32, pub connector: Option<String>, pub active_session: Option<u32> }`
  - `pub struct Session { pub number: u32, pub name: String, pub monitor: String, pub last_shown: Option<i64>, pub last_used: u64 }`
  - `pub struct Workspace { pub real_id: i64, pub session: u32, pub number: u32, pub name: Option<String>, pub tiles: Vec<Tile>, pub previous: Option<i64> }`
  - `pub struct Tile { pub address: String, pub pid: Option<u32>, pub class: String, pub title: String, pub floating: bool, pub at: (i32, i32), pub size: (i32, i32) }`
  - `impl State`: `fn range_for_slot(slot: u32) -> RangeInclusive<i64>`,
    `fn monitor(&self, key: &str) -> Option<&Monitor>`,
    `fn ensure_monitor(&mut self, key: &str, connector: &str) -> u32 /* slot */`,
    `fn alloc_real_id(&self, monitor_key: &str) -> i64`,
    `fn next_session_number(&self) -> u32`,
    `fn next_workspace_number(&self, session: u32) -> u32`,
    `fn session_workspaces(&self, session: u32) -> Vec<&Workspace>` (sorted by number),
    `fn workspace_by_number(&self, session: u32, number: u32) -> Option<&Workspace>`,
    `fn create_session(&mut self, monitor_key: &str, name: Option<&str>) -> u32`
    (name defaults to the number as text; also creates workspace 1),
    `fn create_workspace(&mut self, session: u32) -> i64`.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn slot_ranges() {
    assert_eq!(State::range_for_slot(1), 100001..=199999);
    assert_eq!(State::range_for_slot(2), 200001..=299999);
}
#[test] fn monitor_slots_are_first_seen_order_and_permanent() {
    let mut s = State::default();
    assert_eq!(s.ensure_monitor("LG A", "DP-1"), 1);
    assert_eq!(s.ensure_monitor("Panel B", "eDP-1"), 2);
    assert_eq!(s.ensure_monitor("LG A", "DP-3"), 1); // connector changed, same slot
}
#[test] fn real_ids_lowest_free_in_range_across_all_sessions() {
    let mut s = State::default(); s.ensure_monitor("M", "DP-1");
    let a = s.create_session("M", None);
    assert_eq!(s.session_workspaces(a)[0].real_id, 100001);
    assert_eq!(s.create_workspace(a), 100002);
    s.workspaces.remove(&100001);
    assert_eq!(s.alloc_real_id("M"), 100001);
}
#[test] fn session_numbers_unique_and_named_after_number() {
    let mut s = State::default(); s.ensure_monitor("M", "DP-1"); s.ensure_monitor("N", "DP-2");
    let a = s.create_session("M", None); let b = s.create_session("N", Some("main"));
    assert_eq!((a, b), (1, 2));
    assert_eq!(s.sessions[&1].name, "1");
    assert_eq!(s.sessions[&2].name, "main");
}
#[test] fn workspace_numbers_lowest_free_from_one() {
    let mut s = State::default(); s.ensure_monitor("M", "DP-1");
    let a = s.create_session("M", None);
    let w2 = s.create_workspace(a); let _w3 = s.create_workspace(a);
    s.workspaces.remove(&w2);
    assert_eq!(s.next_workspace_number(a), 2);
}
```

- [ ] **Step 2: Run** `cargo test model` . Expected: FAIL (types missing).
- [ ] **Step 3: Implement** the types and methods in `src/model.rs`; `src/main.rs`
  declares the modules and prints usage for now. `Cargo.toml`: package `codemux`,
  edition 2024, deps `anyhow`, `serde` (derive), `serde_json`, `libc`.
- [ ] **Step 4: Run** `cargo test model && cargo clippy --all-targets -- -D warnings`.
  Expected: PASS, no warnings.
- [ ] **Step 5: Commit** (`feat: codemux model and id allocation`).

### Task 3: Config file `~/.codemux.conf`

**Files:**
- Create: `src/config.rs`
- Test: `src/config.rs`

**Interfaces:**
- Produces:
  - `pub enum Action { NewWorkspace, CommandPrompt, RenameSession, RenameWorkspace, Tree, Options, DisplayPanes, Mark, Swap, KillTile, PreviousWorkspace, NextWorkspace }`
    with `fn from_name(&str) -> Option<Action>` and `fn name(&self) -> &'static str`
    using the kebab names `new-workspace`, `command-prompt`, `rename-session`,
    `rename-workspace`, `tree`, `options`, `display-panes`, `mark`, `swap`, `kill-tile`,
    `previous-workspace`, `next-workspace`.
  - `pub struct Config { pub leader: String, pub binds: Vec<(String, Action)>, pub global_binds: Vec<(String, Action)>, pub relaunch: bool, pub leader_timeout_ms: u32, pub display_panes_ms: u32, pub resize_step: u32, pub launch_overrides: BTreeMap<String, String> }`
  - `impl Default for Config` with the spec defaults: leader `SUPER+B`; binds
    `c`->NewWorkspace, `:`->CommandPrompt, `$`->RenameSession, `,`->RenameWorkspace,
    `w`->Tree, `o`->Options, `q`->DisplayPanes, `m`->Mark, `v`->Swap, `x`->KillTile;
    global `SUPER+SHIFT+ALT+H`->PreviousWorkspace, `SUPER+SHIFT+ALT+L`->NextWorkspace;
    relaunch true; 2000; 2000; 50.
  - `pub fn parse(text: &str) -> (Config, Vec<(usize, String)>)` (errors: 1-based line,
    message). Starts from `Config::default()`; a `bind` for a key replaces the default
    bind of that key.
  - `pub fn default_text() -> String` (the file install writes; round-trips through
    `parse` to `Config::default()` with no errors).
  - `pub fn set_relaunch_line(text: &str, on: bool) -> String` (rewrites only that line,
    appends one if absent, keeps comments).

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn default_text_round_trips() {
    let (c, e) = parse(&default_text());
    assert!(e.is_empty()); assert_eq!(c, Config::default());
}
#[test] fn bad_line_is_skipped_and_named_rest_applies() {
    let (c, e) = parse("set leader SUPER+N\nfrobnicate x\nset relaunch off\n");
    assert_eq!(c.leader, "SUPER+N"); assert!(!c.relaunch);
    assert_eq!(e.len(), 1); assert_eq!(e[0].0, 2);
}
#[test] fn bind_replaces_default_for_that_key() {
    let (c, _) = parse("bind c tree\n");
    assert_eq!(c.binds.iter().find(|(k, _)| k == "c").unwrap().1, Action::Tree);
}
#[test] fn unknown_action_is_an_error() {
    let (_, e) = parse("bind c fly\n"); assert_eq!(e[0].0, 1);
}
#[test] fn launch_override_keeps_quoted_command() {
    let (c, _) = parse("launch-override steam \"steam -silent\"\n");
    assert_eq!(c.launch_overrides["steam"], "steam -silent");
}
#[test] fn set_relaunch_line_preserves_comments() {
    let out = set_relaunch_line("# mine\nset relaunch on\nset leader SUPER+B\n", false);
    assert_eq!(out, "# mine\nset relaunch off\nset leader SUPER+B\n");
}
```

- [ ] **Step 2: Run** `cargo test config`. Expected: FAIL.
- [ ] **Step 3: Implement** `src/config.rs`. Grammar: blank and `#` lines ignored;
  `set <name> <value>`, `bind <key> <action>`, `bind-global <keys> <action>`,
  `launch-override <class> "<command>"`. `set relaunch` accepts `on`/`off`; numeric sets
  must parse as `u32`.
- [ ] **Step 4: Run** `cargo test config && cargo clippy --all-targets -- -D warnings`.
  Expected: PASS.
- [ ] **Step 5: Commit** (`feat: codemux.conf parser`).

### Task 4: `codemux.lua` generator

**Files:**
- Create: `src/luagen.rs`
- Test: `src/luagen.rs`

**Interfaces:**
- Consumes: `Config`, `Action` (Task 3).
- Produces: `pub fn generate(cfg: &Config, exe: &str, s1_s2: &SpikeFacts) -> String`
  where `pub struct SpikeFacts { pub catchall: Option<String> /* exact Lua bind form from S2, None if unsupported */, pub unbind_removes_all: bool }`
  (values from `docs/spikes.md`, hard-coded as consts in `luagen.rs`).

Content the generated file must contain, in this order (spec 2, 3 item 2, 3.2.1):
1. Header comment: generated from `~/.codemux.conf`, do not edit.
2. `o.workspaces.reconcile = function() end`
3. Leader: a fresh oneshot timer per press (spikes.md S2: a fired oneshot cannot be
   re-armed): `local codemux_timer` at file level, and
   `o.bind("<leader>", "codemux leader", function() if codemux_timer then codemux_timer:set_enabled(false) end hl.dispatch(hl.dsp.submap("codemux")) codemux_timer = hl.timer(function() hl.dispatch(hl.dsp.submap("reset")) end, { timeout = <leader_timeout_ms>, type = "oneshot" }) end)`
   with `<leader>` converted from `SUPER+B` to `SUPER + B`. Every submap action, the
   catch-all and `ESCAPE` also disable `codemux_timer`.
4. `hl.define_submap("codemux", function() ... end)` holding, per bind, one **plain**
   bind (no `SUPER + ` variant; user decision after S2),
   `o.bind(<key>, "codemux <action>", function() hl.exec_cmd("<exe> action <name>") hl.dispatch(hl.dsp.submap("reset")) end)`;
   key spelling `$` -> `SHIFT + 4`, `:` -> `SHIFT + semicolon`, `,` -> `comma`, letters
   upper-case; `ESCAPE` resets; no-op binds for `Super_L`, `Super_R`,
   `SUPER + Super_L`, `SUPER + Super_R` (spikes.md S2); the catch-all
   `hl.bind("catchall", ...)` resets.
5. Global binds: `o.bind("SUPER + SHIFT + ALT + H", ..., "<exe> action previous-workspace")` etc.
6. Takeovers (spec 2): for n in 1..=10, key `code:<n+9>`, local number `n` (10 for
   `code:19`): `o.rebind("SUPER + code:K", "codemux workspace N", "<exe> focus N")`,
   `o.rebind("SUPER + SHIFT + code:K", ..., "<exe> move N")`,
   `o.rebind("SUPER + SHIFT + ALT + code:K", ..., "<exe> move N --silent")`;
   `SUPER + TAB` -> `next`, `SUPER + SHIFT + TAB` -> `prev`, `SUPER + CTRL + TAB` ->
   `previous`, `SUPER + mouse_down` -> `next`, `SUPER + mouse_up` -> `prev`,
   `SUPER + SHIFT + ALT + LEFT|RIGHT|UP|DOWN` -> `session-move l|r|u|d`. If
   `unbind_removes_all` is false, each takeover is preceded by repeated
   `hl.unbind("<keys>")` calls using the return value S2 recorded (a loop until it
   reports nothing removed). Alt + Tab is not taken over (Scope).
7. Daemon start, last: `hl.exec_cmd("<exe> daemon")`.

- [ ] **Step 1: Write failing tests**

```rust
fn gen() -> String { generate(&Config::default(), "/home/u/.local/bin/codemux",
    &SpikeFacts { catchall: Some("catchall".into()), unbind_removes_all: true }) }
#[test] fn leader_timeout_from_config() {
    let (c, _) = crate::config::parse("set leader-timeout 1500
");
    let g = generate(&c, "x", &SpikeFacts { catchall: None, unbind_removes_all: true });
    assert!(g.contains("timeout = 1500"));
}
#[test] fn alt_tab_not_taken_over_in_phase_2() { assert!(!gen().contains("ALT + TAB")); }
#[test] fn disables_reconcile_and_starts_daemon_last() {
    let g = gen();
    assert!(g.contains("o.workspaces.reconcile = function() end"));
    assert!(g.trim_end().ends_with("hl.exec_cmd(\"/home/u/.local/bin/codemux daemon\")"));
}
#[test] fn number_row_uses_omarchy_keycodes() {
    let g = gen();
    assert!(g.contains("o.rebind(\"SUPER + code:10\", \"codemux workspace 1\", \"/home/u/.local/bin/codemux focus 1\")"));
    assert!(g.contains("\"SUPER + SHIFT + ALT + code:19\""));
    assert!(g.contains("codemux move 10 --silent"));
}
#[test] fn leader_actions_bound_plain_only() {
    let g = gen();
    assert!(g.contains("o.bind(\"C\", \"codemux new-workspace\""));
    assert!(!g.contains("o.bind(\"SUPER + C\""));
    assert!(g.contains("\"Super_L\""));
    assert!(g.contains("o.bind(\"SHIFT + 4\", \"codemux rename-session\""));
    assert!(g.contains("o.bind(\"SHIFT + semicolon\", \"codemux command-prompt\""));
}
#[test] fn custom_leader_is_used() {
    let (c, _) = crate::config::parse("set leader SUPER+N\n");
    let g = generate(&c, "x", &SpikeFacts { catchall: None, unbind_removes_all: true });
    assert!(g.contains("o.bind(\"SUPER + N\", \"codemux leader\""));
}
#[test] fn no_em_dash() { assert!(!gen().contains('\u{2014}')); }
```

- [ ] **Step 2: Run** `cargo test luagen`. Expected: FAIL.
- [ ] **Step 3: Implement** `generate` as a string builder.
- [ ] **Step 4: Run** `cargo test luagen`; then write the output for the real config to
  a temp file and run `luac -p` on it if `luac` is installed (syntax check).
  Expected: PASS; `luac -p` silent.
- [ ] **Step 5: Commit** (`feat: generate codemux.lua from config`).

### Task 5: Hyprland adapter and socket2 parser

**Files:**
- Create: `src/hypr.rs`, `src/events.rs`, `tests/common/mod.rs` (FakeHypr)
- Test: `src/events.rs`, `src/hypr.rs`

**Interfaces:**
- Produces:
  - `pub struct HMonitor { pub name: String, pub description: String, pub focused: bool, pub active_workspace: i64, pub x: i32, pub y: i32 }`
  - `pub struct HWorkspace { pub id: i64, pub name: String, pub monitor: String, pub windows: u32 }`
    (`name` identifies special workspaces such as `special:minimized`, whose ids are
    dynamic)
  - `pub struct HClient { pub address: String, pub pid: u32, pub class: String, pub title: String, pub workspace: i64, pub floating: bool, pub at: (i32, i32), pub size: (i32, i32), pub tags: Vec<String> }`
  - `pub trait Hypr { fn monitors(&self) -> Result<Vec<HMonitor>>; fn workspaces(&self) -> Result<Vec<HWorkspace>>; fn clients(&self) -> Result<Vec<HClient>>; fn active_address(&self) -> Result<Option<String>>; fn dispatch(&self, lua: &str) -> Result<()>; fn eval(&self, lua: &str) -> Result<()>; }`
  - `pub struct Hyprctl;` implementing it via `hyprctl -j monitors|workspaces|clients|activewindow`,
    `hyprctl dispatch <lua>`, `hyprctl eval <lua>` (spec 3.1). The JSON is camelCase
    with nested objects (`"workspace": {"id", "name"}`, `"activeWorkspace": {"id", "name"}`,
    `"at": [x, y]`, `"size": [w, h]`), so `Hyprctl` deserializes private raw structs
    matching that shape and flattens them into `HClient`/`HMonitor`/`HWorkspace`.
    Special workspaces have negative ids and stay in the returned lists; callers
    filter them (Task 7).
  - Lua builders (`pub fn`): `focus_workspace(id) -> String` =
    `hl.dsp.focus({ workspace = "<id>" })`; `focus_monitor(name)` =
    `hl.dsp.focus({ monitor = "<name>" })`; `move_window(addr, id, follow)` =
    `hl.dsp.window.move({ workspace = "<id>", follow = <bool>, window = "address:<addr>" })`;
    `move_workspace(id, monitor)` =
    `hl.dsp.workspace.move({ workspace = "<id>", monitor = "<name>" })`;
    `change_id(old, new)` = `hl.dsp.workspace.change_id({ workspace = "<old>", id = <new> })`
    (forms as Omarchy uses them in `monitor-workspaces.lua` and `Workspaces.qml`).
  - `pub enum Event { OpenWindow { address, workspace_name, class, title }, CloseWindow { address }, MoveWindow { address, workspace: i64, workspace_name: String }, CreateWorkspace { id: i64 }, DestroyWorkspace { id: i64 }, MoveWorkspace { id: i64, monitor: String }, MonitorAdded { name: String, description: String }, MonitorRemoved { name: String, description: String }, Workspace { id: i64 }, ConfigReloaded, Other }`
    plus `ChangeWorkspaceId { old: i64, new: i64 }`, `FloatingChanged { address, floating: bool }`
    and `Submap { name: String }`, and `pub fn parse_line(line: &str) -> Event` using the
    exact formats recorded in `docs/spikes.md` (S1, S4); only the `v2` variants of
    workspace/monitor events are parsed, the v1 duplicates map to `Other`.
    Addresses are normalised to `0x`-prefixed lower-case (socket2 omits `0x`;
    `hyprctl -j` includes it).
  - `tests/common/mod.rs`: `pub struct FakeHypr { pub monitors, pub workspaces, pub clients: RefCell<Vec<..>>, pub dispatched: RefCell<Vec<String>>, pub evaled: RefCell<Vec<String>> }`
    implementing `Hypr` from its fields and recording `dispatch`/`eval` strings.

- [ ] **Step 1: Write failing tests** (sample lines copied verbatim from
  `docs/spikes.md`; the shapes below are placeholders to be replaced by the recorded
  ones):

```rust
#[test] fn parses_openwindow_and_normalises_address() {
    assert_eq!(parse_line("openwindow>>55d1a2b3c4d0,100001,kitty,~"),
        Event::OpenWindow { address: "0x55d1a2b3c4d0".into(), workspace_name: "100001".into(),
            class: "kitty".into(), title: "~".into() });
}
#[test] fn parses_monitor_removed_v2() { /* recorded S1 line -> MonitorRemoved { name: "DP-1", description: <recorded> } */ }
#[test] fn parses_moveworkspace_v2() { /* recorded S1 line -> MoveWorkspace { id: 100002, monitor: "DP-1" } */ }
#[test] fn unknown_event_is_other() { assert_eq!(parse_line("bell>>x"), Event::Other); }
#[test] fn lua_builders_match_omarchy_forms() {
    assert_eq!(focus_workspace(100003), "hl.dsp.focus({ workspace = \"100003\" })");
    assert_eq!(move_window("0xab", 200001, false),
        "hl.dsp.window.move({ workspace = \"200001\", follow = false, window = \"address:0xab\" })");
}
#[test] fn hyprctl_json_fixtures_deserialize() {
    // fixtures captured once with `hyprctl -j clients|workspaces|monitors > tests/fixtures/*.json`
}
```

- [ ] **Step 2: Run** `cargo test events hypr`. Expected: FAIL.
- [ ] **Step 3: Implement** both modules and the fake. Capture the three fixtures from
  the live desktop into `tests/fixtures/`.
- [ ] **Step 4: Run** `cargo test`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: hyprland adapter and socket2 parser`).

### Task 6: Engine, workspace commands

**Files:**
- Create: `src/engine.rs`
- Test: `tests/engine_commands.rs`

**Interfaces:**
- Consumes: `State` (Task 2), `Config` (Task 3), `Hypr`, Lua builders (Task 5).
- Produces:
  - `pub struct Engine<H: Hypr> { pub state: State, pub config: Config, hypr: H, signature: String, spawned: HashSet<u32>, dirty: bool }`
  - `pub fn new(state: State, config: Config, hypr: H, signature: String, layouts_dir: PathBuf) -> Self`
    (the daemon passes `layout::layouts_dir()`; tests pass a temp dir; Task 6 stores it
    unused until Task 8)
  - `pub fn focused_session(&self) -> Result<u32>` (active session of the focused monitor)
  - `pub fn handle(&mut self, req: Request) -> Response` where (defined here, reused by
    Task 10) `#[serde(tag = "cmd", rename_all = "kebab-case")] pub enum Request { Focus { n: u32 }, Move { n: u32, follow: bool }, Next, Prev, Previous, NewWorkspace, NewSession { name: Option<String> }, SwitchSession { number: u32 }, SessionMove { dir: char }, Action { name: String }, Status }`
    and `pub struct Response { pub ok: bool, pub message: Option<String> }`.
  - `pub fn take_dirty(&mut self) -> bool`, `pub fn hypr(&self) -> &H` (tests read
    `FakeHypr::dispatched() -> Vec<String>`).

Behaviour (spec 2 takeovers 1-6, 5.6):
- `Focus{n}`: workspace n of the focused session: dispatch `focus_workspace(real_id)`;
  missing n: `ok: true`, no dispatch, no state change.
- `Move{n, follow}`: active window to workspace n of the focused session via
  `move_window`; missing n or no active window: no dispatch. (Placeholders: phase 5.)
- `Next`/`Prev`: next/previous existing number in the session, wrapping around at either end like tmux (user decision at acceptance).
- `Previous`: the workspace's `previous` pointer, within the session.
- `NewWorkspace`: `create_workspace` then focus it.
- `NewSession{name}`: `create_session` on the focused monitor, make it active, focus its
  workspace 1.
- `SwitchSession{number}` (temporary CLI, spec 9 phase 2): focus that session's
  `last_shown` (or lowest workspace) on its monitor.
- `Status`: `ok: true`, `message` = one line per monitor
  `<connector> <description>: [<n>: <name>]* <other sessions>` then, per session,
  `  <n>: <name> workspaces <number>=<real_id> ...` (plain text for humans and tests).
- Whenever a session becomes a monitor's active session (any path), set its
  `last_used` to a counter that increases on every activation.
- `Action{name}`: `new-workspace`, `previous-workspace`, `next-workspace` map to the
  requests above; every other action answers
  `ok: false, message: "codemux: <name> is not in this version yet"`.
- Every state change sets `dirty`.

- [ ] **Step 1: Write failing tests** (each builds a `FakeHypr` with one monitor
  `DP-1`/`"LG A"` focused on 100001 and a state with session 1 owning 100001, 100002):

```rust
#[test] fn focus_existing_number_dispatches_real_id() {
    let mut e = engine(); e.handle(Request::Focus { n: 2 });
    assert_eq!(e.hypr().dispatched(), vec![focus_workspace(100002)]);
}
#[test] fn focus_missing_number_does_nothing() {
    let mut e = engine(); let before = e.state.clone();
    let r = e.handle(Request::Focus { n: 7 });
    assert!(r.ok); assert!(e.hypr().dispatched().is_empty()); assert_eq!(e.state, before);
}
#[test] fn move_silent_keeps_focus() {
    let mut e = engine_with_active_window("0xab");
    e.handle(Request::Move { n: 2, follow: false });
    assert_eq!(e.hypr().dispatched(), vec![move_window("0xab", 100002, false)]);
}
#[test] fn next_does_not_wrap() { /* focused 100002 (last): Next -> dispatched() empty */ }
#[test] fn new_workspace_takes_lowest_free_number_and_focuses_it() {
    let mut e = engine(); e.state.workspaces.remove(&100002);
    e.handle(Request::NewWorkspace);
    assert_eq!(e.state.workspace_by_number(1, 2).unwrap().real_id, 100002);
    assert!(e.take_dirty());
}
#[test] fn new_session_named_and_active() {
    let mut e = engine(); e.handle(Request::NewSession { name: Some("Work".into()) });
    let s = e.state.sessions.values().find(|s| s.name == "Work").unwrap();
    assert_eq!(e.state.monitors[0].active_session, Some(s.number));
}
#[test] fn status_lists_sessions_per_monitor() {
    let r = engine().handle(Request::Status);
    assert!(r.ok); assert!(r.message.unwrap().contains("1: 1"));
}
#[test] fn unimplemented_action_says_so() {
    let r = engine().handle(Request::Action { name: "tree".into() });
    assert!(!r.ok); assert_eq!(r.message.unwrap(), "codemux: tree is not in this version yet");
}
```

- [ ] **Step 2: Run** `cargo test --test engine_commands`. Expected: FAIL.
- [ ] **Step 3: Implement** `Engine::new`, `handle`, `focused_session`, `take_dirty`.
- [ ] **Step 4: Run** `cargo test`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: engine workspace commands`).

### Task 7: Engine, events, monitor invariant, session move

**Files:**
- Modify: `src/engine.rs`
- Test: `tests/engine_events.rs`

**Interfaces:**
- Produces: `pub fn on_event(&mut self, ev: Event) -> Result<()>` and
  `pub fn enforce_invariant(&mut self) -> Result<()>` (spec 3.2.5 steps 1-3), plus the
  `SessionMove{dir}` request branch.

Behaviour:
- **Special workspaces** (id <= 0: scratchpad, Quake console, minimized, stash) are
  outside the model (spec 1 item 6): `on_event` ignores events whose workspace is
  special, `enforce_invariant` and every scan skip them (like Omarchy's `regular()`).
  A window moved **to** a special workspace (Super + Alt + S, Super + Shift + grave):
  its tile is removed from its workspace, and that counts as the tile moving away for
  deletion (spec 5.6), except while the window is on `special:minimized` and tagged
  `omarchy-minimized-from-<id>` (spec 1 item 6), which keeps the origin alive. A
  window moved **from** a special workspace onto a regular one is recorded like
  `OpenWindow` there.
- **Unknown workspace while running** (spec 3.2.10): `OpenWindow`, `MoveWindow` or
  `MoveWorkspace` onto a regular id not in state, unless the window's pid is in
  `spawned`: adopt it into the active session of the monitor it is on as a new
  workspace, renumbered with `renumber` (Task 8 adds `renumber`; until Task 8 lands
  this branch calls a `todo!()`-free stub that records the workspace under its current
  id, and Task 8's tests cover the renumbered form).
- `OpenWindow` on a known workspace: append a `Tile` (fill pid/geometry from
  `clients()`); `CloseWindow`: remove the tile; if the workspace is now empty and not
  shown, delete it (spec 5.6, event-driven); if its session has no workspaces left and
  is not active, delete the session.
- `Workspace{id}` (shown changed): set `previous` and `last_shown`; if the workspace left
  behind is empty, delete it as above; if `id` belongs to another session on that
  monitor, that session becomes active (spec 3.2.6).
- `MoveWindow`: move the tile record to the target workspace's record (spec 3.2.7);
  apply deletion to the source as above.
- `MonitorAdded`/`MonitorRemoved`/`MoveWorkspace`/`CreateWorkspace`: mark the invariant
  pending; the daemon (Task 11) calls `enforce_invariant` once 300 ms pass with no
  further such event (spikes.md S1: Hyprland moves workspaces before reporting a
  monitor removal or addition). `enforce_invariant` reads the live monitor list.
- `ChangeWorkspaceId{old, new}` not caused by codemux: move the mapping to `new` under
  the spec 3.2.9 guards, then `renumber` it back into its range (Task 8). It is idempotent, so running it after codemux's own actions is a
  no-op; that is how "events it did not cause" is honoured without tracking causes.
- `enforce_invariant` step 1: a session whose monitor is gone moves to the monitor
  holding most of its live workspaces (tie: focused monitor), other workspaces moved
  there with `move_workspace`; step 2: any workspace of a connected monitor's session
  sitting elsewhere is moved back; step 3: a connected monitor with no sessions gets
  `create_session(name "main")`.
- `SessionMove{dir}`: target = connected monitor in that direction by `x`/`y`; move every
  workspace of the focused session there, make it active there, focus its last shown;
  the source monitor activates its most recently used other session (`last_used`) or a
  new `main` (spec 2 takeover 7). No target: no-op.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn new_monitor_without_sessions_gets_main() {
    let mut e = two_monitor_engine_with_sessions_only_on("LG A");
    e.on_event(Event::MonitorAdded { name: "DP-2".into(), description: "Dell B".into() }).unwrap();
    let s = e.state.sessions.values().find(|s| s.monitor == "Dell B").unwrap();
    assert_eq!(s.name, "main");
    assert_eq!(e.state.session_workspaces(s.number).len(), 1);
}
#[test] fn unplugged_monitor_sessions_join_remaining_monitor_keeping_names() {
    /* MonitorRemoved(Dell B) with its 200001 now on DP-1: session "Gaming" has monitor "LG A", name unchanged, LG A active session unchanged */ }
#[test] fn replug_moves_returned_workspaces_back() {
    // after unplug+replug, Hyprland put session 1's 100002 on DP-1 again; session 1 now
    // belongs to "Dell B": expect dispatched move_workspace(100002, "DP-2")
}
#[test] fn invariant_is_idempotent() {
    let mut e = steady_engine(); e.enforce_invariant().unwrap();
    assert!(e.hypr().dispatched().is_empty());
}
#[test] fn closing_last_window_of_unshown_workspace_deletes_it() { /* 100002 unshown, one tile; CloseWindow -> 100002 not in state */ }
#[test] fn shown_empty_workspace_is_kept_until_left() { /* close last tile of shown 100001 -> kept; Workspace{100002} -> 100001 deleted */ }
#[test] fn outside_focus_switches_active_session() { /* Workspace{id of session 2 on same monitor} -> monitor.active_session == Some(2) */ }
#[test] fn session_move_right_activates_on_target_and_refills_source() {
    /* DP-1 at x=0, DP-2 at x=2560; SessionMove{'r'} from DP-1: move_workspace(each, "DP-2") dispatched, session.monitor == "Dell B" and active there, DP-1 active = most recent other session or new "main" */ }
#[test] fn move_to_scratchpad_removes_tile_and_ignores_special() {
    /* MoveWindow{0xab -> -98}: tile gone from 100002; no special id in state; 100002
       (unshown, now empty) deleted */ }
#[test] fn special_workspaces_never_enter_state() {
    /* CreateWorkspace{-98}, OpenWindow on special: state.workspaces has no id <= 0 */ }
#[test] fn window_opened_on_unknown_id_is_adopted_into_monitor_active_session() {
    /* OpenWindow on id 5 (DP-1): a workspace for it joins DP-1's active session */ }
#[test] fn session_move_with_no_monitor_in_direction_is_noop() { /* SessionMove{'l'} from leftmost: dispatched() empty, state unchanged */ }
```

- [ ] **Step 2: Run** `cargo test --test engine_events`. Expected: FAIL.
- [ ] **Step 3: Implement** `on_event`, `enforce_invariant`, `SessionMove`.
- [ ] **Step 4: Run** `cargo test`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: engine events, monitor invariant, session move`).

### Task 8: Layout modes and renumbering

**Files:**
- Create: `src/layout.rs`
- Modify: `src/engine.rs` (`renumber`, `delete_workspace`, `layouts_dir` field)
- Test: `src/layout.rs` (file-level tests, temp dir); engine-level tests in
  `tests/engine_layout.rs` (FakeHypr from `tests/common`)

**Interfaces:**
- Produces:
  - `pub fn layouts_dir() -> PathBuf` (`~/.local/state/omarchy/workspace-layouts`)
  - `pub fn read_mode(dir: &Path, id: i64, live_tiled_layout: Option<&str>, default_mode: impl Fn() -> Option<String>) -> String`
    implementing spec 3.2.11's chain: `<id>.lua` mode; else if `dir/default.lua`
    exists, `default_mode()` (wraps `omarchy-hyprland-workspace-layout-default`); else
    `live_tiled_layout`; else `"dwindle"`.
  - `pub fn move_mode_file(dir: &Path, old: i64, new: i64) -> Result<()>` (rename and
    rewrite `workspace = "<old>"` to `"<new>"`; no file, no-op).
  - `pub fn delete_mode_file(dir: &Path, id: i64) -> Result<()>`.
  - Engine: `pub fn renumber(&mut self, old: i64, new: i64) -> Result<()>` = dispatch
    `change_id`, `move_mode_file`, update state keys. Task 7's unknown-workspace stub
    now calls `renumber` into the monitor's range.
  - Engine: one private `fn delete_workspace(&mut self, id: i64)` used by **every**
    deletion path from Task 7 (close, move away, leave while empty, restart drop); it
    removes the state entry and calls `delete_mode_file(layouts_dir, id)`, so a reused
    id never inherits a dead workspace's mode (spec 3.2.2). The layouts dir is a field
    on `Engine` (tests pass a temp dir).

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn scrolling_file_moves_with_renumber() {
    let d = tmp(); fs::write(d.join("1.lua"), "o.workspace_mode({ workspace = \"1\", mode = \"scrolling\" })\n").unwrap();
    move_mode_file(&d, 1, 100001).unwrap();
    assert!(!d.join("1.lua").exists());
    assert_eq!(read_mode(&d, 100001, None, || None), "scrolling");
}
#[test] fn no_file_no_default_falls_to_live_then_dwindle() {
    let d = tmp();
    assert_eq!(read_mode(&d, 5, Some("master"), || None), "master");
    assert_eq!(read_mode(&d, 5, None, || None), "dwindle");
}
#[test] fn default_lua_present_uses_default_script() {
    let d = tmp(); fs::write(d.join("default.lua"), "").unwrap();
    assert_eq!(read_mode(&d, 5, Some("dwindle"), || Some("scrolling".into())), "scrolling");
}
#[test] fn deleting_a_workspace_deletes_its_mode_file() {
    /* engine with layouts dir holding 100002.lua; close last tile of unshown 100002 ->
       100002.lua gone; create_workspace reuses 100002 and read_mode == "dwindle" */ }
#[test] fn live_adoption_renumbers_into_range() {
    /* OpenWindow on id 5 (DP-1, slot 1): change_id(5, 100003) dispatched */ }
#[test] fn delete_removes_file() { /* write 100001.lua, delete_mode_file(100001) -> file gone; missing file -> Ok */ }
```

- [ ] **Step 2: Run** `cargo test layout`. Expected: FAIL.
- [ ] **Step 3: Implement.** Mode file format verified against a real file in
  `~/.local/state/omarchy/workspace-layouts/` before writing the regex.
- [ ] **Step 4: Run** `cargo test`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: layout mode files follow renumbering`).

### Task 9: Start kinds, adoption, start scan, landing

**Files:**
- Modify: `src/engine.rs`
- Test: `tests/engine_start.rs`

**Interfaces:**
- Produces: `pub enum StartKind { Adopted, Restarted, HyprlandStarted }` and
  `pub fn start(&mut self) -> Result<StartKind>` (spec 3.2.8).

Behaviour:
- No saved state (caller passes `State::default()` with `signature: None`): adoption
  (spec 3.2.3): per connected monitor, `ensure_monitor`, session `main`, each occupied
  regular workspace on that monitor renumbered into its range keeping its slot number:
  local number = `id - ((id - 1) / 10) * 10` (so 12 -> 2, 10 -> 10), computed from the
  workspace id only. If two workspaces on one monitor map to the same local number
  (a guest from an unplugged monitor), the higher id takes the lowest free number.
  Tiles recorded.
- Same signature: restart rules (spec 3.2.8): match live clients to tiles by address AND
  pid; unmatched app tiles dropped (deletion rules apply, unless shown); unaccounted
  windows on known workspaces recorded there; unknown ids adopted into the active
  session of their monitor and renumbered.
- Different signature: clear addresses/pids; landing: each monitor focuses its active
  session's `last_shown` (creating the workspace if needed); then the scan as above.
  Relaunch is phase 6: if `config.relaunch`, log `relaunch not in this version yet`.
- Always: set `state.signature` to the current one; `dirty = true`.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn adoption_keeps_slots_and_makes_main_per_monitor() {
    // live: DP-1 block 0 with occupied 1 and 3; DP-2 block 1 with occupied 11
    let mut e = fresh_engine(); assert_eq!(e.start().unwrap(), StartKind::Adopted);
    let main1 = session_named(&e, "main", "LG A");
    assert_eq!(nums(&e, main1), vec![1, 3]);
    assert!(e.hypr().dispatched().contains(&change_id(3, 100003)));
}
#[test] fn adoption_collision_takes_lowest_free_number() {
    /* DP-1 holds 12 and guest 2: 2 -> number 2, 12 -> lowest free (1 if free) */ }
#[test] fn adoption_keeps_scrolling_mode() { /* layouts dir has 1.lua scrolling; after start(): 1.lua gone, read_mode(100001) == "scrolling" */ }
#[test] fn restart_matches_by_address_and_pid() { /* saved tile 0xab pid 10; live 0xab pid 11 -> old tile dropped, live window recorded as new tile */ }
#[test] fn restart_drops_closed_apps_and_deletes_unshown_empty_workspace() { /* saved 100002 tile has no live window, 100002 unshown -> 100002 not in state */ }
#[test] fn restart_records_new_window_on_known_workspace() { /* live 0xcd on 100001 not in state -> tile added to 100001, no new workspace */ }
#[test] fn restart_adopts_unknown_workspace_into_its_monitors_active_session() { /* live ws 4 on DP-2 -> change_id(4, 2000xx) dispatched, joins DP-2 active session */ }
#[test] fn hyprland_started_lands_each_monitor_on_last_shown() { /* other signature: focus_monitor + focus_workspace(last_shown) dispatched per monitor, before anything else */ }
#[test] fn signature_saved_after_start() { /* after start(): state.signature == Some(current) */ }
```

- [ ] **Step 2: Run** `cargo test --test engine_start`. Expected: FAIL.
- [ ] **Step 3: Implement** `start` (Task 8's `renumber` does the renumbering).
- [ ] **Step 4: Run** `cargo test`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: start kinds, adoption, landing`).

### Task 10: State store, IPC, CLI client

**Files:**
- Create: `src/store.rs`, `src/ipc.rs`
- Modify: `src/main.rs`
- Test: `src/store.rs`, `tests/ipc.rs`

**Interfaces:**
- Produces:
  - `pub fn state_path() -> PathBuf`, `pub fn load(path: &Path) -> Result<Option<State>>`,
    `pub fn save_atomic(state: &State, path: &Path) -> Result<()>` (write
    `state.json.tmp`, fsync, rename).
  - `pub fn socket_path() -> PathBuf`; `pub fn send(req: &Request) -> Result<Response>`
    (one JSON line out, one line back); `pub fn serve(listener: UnixListener, tx: Sender<Msg>)`.
  - CLI (`src/main.rs`): `codemux focus N | move N [--silent] | next | prev | previous |
    new-workspace | new [NAME] | switch N | session-move l|r|u|d | action NAME | status |
    watch | daemon | install`. **Watch handshake:** `codemux watch` sends the line
    `{"cmd":"watch"}`; `serve` intercepts it (it never reaches `Engine::handle`), turns
    the connection into a watcher via `Msg::Watch`, and the daemon immediately writes
    the full state JSON line to it (spec 3.1: full state on connect), then again on
    every change. `codemux watch` prints each line it receives. Client subcommands: on connection failure exit code 1 and
    run `notify-send codemux "daemon not running"`; on `ok: false` with a message,
    `notify-send codemux "<message>"` and exit 1.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn save_is_atomic_and_round_trips() { /* save then load == original; state.json.tmp absent */ }
#[test] fn load_missing_file_is_none() { /* load(nonexistent) == Ok(None) */ }
#[test] fn request_json_shape() {
    assert_eq!(serde_json::to_string(&Request::Move { n: 3, follow: false }).unwrap(),
        r#"{"cmd":"move","n":3,"follow":false}"#);
}
#[test] fn client_without_daemon_fails_and_notifies() {
    // run the built binary with XDG_RUNTIME_DIR=<empty tmp> and PATH holding a stub
    // notify-send that writes its args to a file; expect exit 1 and the file to
    // contain "daemon not running"
}
```

- [ ] **Step 2: Run** `cargo test store ipc`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test`. Expected: PASS.
- [ ] **Step 5: Commit** (`feat: state store, ipc and cli`).

### Task 11: Daemon loop, lock, lifetime, config watch

**Files:**
- Create: `src/daemon.rs`
- Test: `tests/daemon.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: `pub fn run() -> Result<()>` behind `codemux daemon`, and
  `pub enum Msg { Event(Event), Request(Request, Sender<Response>), Watch(UnixStream), Tick, ConfigChanged, Socket2Closed }`.

Behaviour (spec 3 item 2, 3.1, 6):
- Lock: `flock(LOCK_EX | LOCK_NB)` retried every 100 ms for up to 5 s on
  `codemux.lock`; failure: `notify-send codemux "codemux: another daemon is still running"`,
  exit 0. Holding the lock: remove a stale socket, bind a fresh one. Never remove the
  socket on exit.
- Threads: socket2 reader (`Msg::Event` per line; EOF -> `Socket2Closed`), IPC acceptor,
  30 s ticker, config watcher (poll `~/.codemux.conf` mtime every 2 s).
- Main loop: `Socket2Closed` -> exit 0. `Tick`: exit 0 if
  `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE` is gone; else
  `engine.tick()`, a new `pub fn tick(&mut self) -> Result<()>` in `src/engine.rs` that
  runs the outside-renumber check (spec 3.2.9: a known workspace's tiles found under a
  new regular id move the mapping to that id, **only if** the old id is gone from
  Hyprland and the new id is not mapped in state; then `renumber` it back into its
  range) and then `enforce_invariant`. `ConfigChanged`: re-parse; errors -> one notification naming the
  lines; regenerate Lua in memory; write it only if it differs from the file on disk
  (spec 6.3). Writing it is the reload: Hyprland reloads by itself when a config file
  changes (spikes.md S3), so no `hyprctl reload` call.
- Settle: when the engine marks the invariant pending (Task 7), the loop uses
  `recv_timeout(300 ms)`; if no further monitor or workspace-move event arrives in that
  window, it calls `enforce_invariant`.
- After every message: if `take_dirty()`, `save_atomic` and send the state JSON line to
  every watcher (drop dead watchers).
- Startup order: take the lock, remove a stale socket and bind the listener (so
  clients connecting during start wait rather than fail), load config, load state,
  build engine, `start()`, save, then accept connections.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn second_daemon_gives_up_after_lock_wait_and_notifies() {
    // hold the lock in the test, run `codemux daemon` with a tmp XDG_RUNTIME_DIR and a
    // stub notify-send; expect exit within 5.5 s and the notification text
}
#[test] fn exiting_daemon_leaves_socket_file() {
    /* tmp XDG_RUNTIME_DIR with hypr/<sig>/.socket2.sock served by the test (closes at
       once); PATH holds a stub `hyprctl` printing canned JSON from tests/fixtures and
       a stub notify-send; run `codemux daemon`; after it exits, codemux.sock exists */ }
#[test] fn tick_renumber_check_follows_outside_change_id_only_when_safe() {
    /* engine test: tiles of 100002 now reported under 7, 100002 gone -> mapping moves
       and change_id(7, 100002) dispatched; if 100002 still exists -> nothing */ }
#[test] fn config_change_without_lua_change_does_not_reload() {
    // unit test of `fn reload_needed(old_lua: &str, cfg: &Config) -> bool`
}
```

- [ ] **Step 2: Run** `cargo test --test daemon`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test && cargo clippy --all-targets -- -D warnings`.
  Expected: PASS.
- [ ] **Step 5: Commit** (`feat: daemon loop, lock and lifetime`).

### Task 12: Install and uninstall

**Files:**
- Create: `src/install.rs`, `packaging/codemux-uninstall.sh`
- Test: `src/install.rs` (pure helpers), manual script check

**Interfaces:**
- Produces: `codemux install` and `pub fn add_require_line(hyprland_lua: &str) -> String`
  (inserts `require("hypr.codemux")` on the line after `require("hypr.bindings")`;
  idempotent), `pub fn remove_require_line(text: &str) -> String`.

Install (spec 8.1, phase 2 subset): copy the running binary to `~/.local/bin/codemux`;
write `~/.codemux.conf` from `default_text()` only if absent; generate
`~/.config/hypr/codemux.lua`; patch `hyprland.lua`; copy the uninstaller to
`~/Scripts/AppUninstallers/codemux-uninstall.sh` (chmod 755); `hyprctl reload` (the
reload runs `codemux.lua`, which disables reconcile first and then starts the daemon,
which adopts). The bar widget, window-switcher filter and plugin steps belong to the
phase 3 and 5 plans; in phase 2 Omarchy's workspace widget stays and shows raw slots.

Uninstall script (spec 8.2, phase 2 subset), printing each step: `pkill -x codemux`
(daemon), wait for exit; remove the require line (sed on the exact line); delete
`~/.config/hypr/codemux.lua`; delete layout files with id >= 100001; delete
`~/.local/state/codemux/`; delete `~/.local/bin/codemux`; `hyprctl reload`. Keeps
`~/.codemux.conf` and says so. Deletes itself last.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn require_inserted_after_bindings_once() {
    let t = "require(\"hypr.monitors\")\nrequire(\"hypr.bindings\")\nrequire(\"hypr.looknfeel\")\n";
    let once = add_require_line(t);
    assert_eq!(once, "require(\"hypr.monitors\")\nrequire(\"hypr.bindings\")\nrequire(\"hypr.codemux\")\nrequire(\"hypr.looknfeel\")\n");
    assert_eq!(add_require_line(&once), once);
    assert_eq!(remove_require_line(&once), t);
}
```

- [ ] **Step 2: Run** `cargo test install`. Expected: FAIL.
- [ ] **Step 3: Implement** both; `bash -n packaging/codemux-uninstall.sh` and
  `shellcheck` (if installed) on the script.
- [ ] **Step 4: Run** `cargo test`. Expected: PASS; `bash -n` silent.
- [ ] **Step 5: Commit** (`feat: install and uninstall`).

### Task 13: Live acceptance (user, on the real desktop)

**Files:** none (results appended to `docs/spikes.md` under "Phase 2 acceptance").

- [ ] **Step 1:** `cargo build --release && ./target/release/codemux install`.
  Expected: `hyprctl -j workspaces | jq '[.[].id | select(. > 0)] | min'` is >= 100001; `codemux status`
  shows one `main` session per monitor; the user's scrolling workspaces still scroll.
- [ ] **Step 2:** User checks, each on both monitors: Super + Alt + S (scratchpad) and
  Super + grave (Quake console) still work and leave sessions untouched; Super + 1-0 (missing number does
  nothing), Super + Shift + N, Super + Shift + Alt + N, Super + Tab / Shift + Tab,
  Super + Ctrl + Tab, Super + wheel, Super + Shift + Alt + H / L, Leader then `c`,
  `codemux new Work`, `codemux switch <n>`, Super + Shift + Alt + arrow session move,
  Leader then `w` (notification "not in this version yet").
- [ ] **Step 3:** `hyprctl reload`: nothing renumbers. Unplug and replug DP-1: sessions
  follow spec 3.2.5. Kill the daemon (`pkill -x codemux`), press Super + 1:
  notification "daemon not running"; `hyprctl reload` restarts it and the restart rules
  keep every window in its session.
- [ ] **Step 4:** Edit `~/.codemux.conf` (`set leader SUPER+N`): within 2 s the new
  leader works; `set relaunch off` causes no Hyprland reload.
- [ ] **Step 5:** Run the uninstaller: Omarchy's keys and per-monitor numbering are
  back, no window lost, `~/.codemux.conf` still exists, nothing else of codemux remains
  (`ls ~/.local/state/codemux ~/.config/hypr/codemux.lua ~/.local/bin/codemux` all
  missing).
- [ ] **Step 6:** Reinstall to keep using it; record pass/fail per step.
