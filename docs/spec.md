# ymux - spec

Status: draft 13, written by Opus 5.5. Revised after Fable 5 reviews 1-8 and
confirmation checks of drafts 9-12. The draft-12 check found no blockers and no
majors; its four minors are fixed here. Ready for the implementation plan.
Date: 2026-11-14

ymux brings tmux-style sessions to Hyprland on Omarchy: named sessions per monitor,
each holding its own workspaces numbered from 1, persisted across restarts, shown in a
tmux-style bar widget. (Working name; the project folder is still `~/Projects/hmux`.)

## 1. Model

```
monitor  >>  session  >>  workspace  >>  tile
```

1. **Monitor:** a physical screen, identified by make, model and serial (Hyprland's
   monitor `description`), not by connector name, the same rule Omarchy uses
   (`/usr/share/omarchy/default/hypr/monitor-workspaces.lua:9-14`).
2. **Session:** belongs to exactly one monitor. Has a number and a name the user can
   change. The number is unique across all monitors and never changes, even when the
   session moves to another monitor, so two sessions may share a name (like tmux,
   where sessions are told apart by id). Each monitor has one active session at a
   time; monitors switch sessions independently. A monitor always has at least one
   session.
3. **Workspace:** belongs to exactly one session. Numbered from 1 within its session
   (never 0). Can be given a name (Leader + `,`); an unnamed workspace is shown by its
   number only, never by an app name (user decision at phase 3 acceptance). Has a layout
   mode, using Omarchy's names: `dwindle` (tiled), `scrolling` or `floating`.
4. **Tile:** one window on a workspace (the counterpart of a tmux pane). A tile holds
   either an app's window or a **placeholder**.
5. **Placeholder:** a transparent, borderless window owned by ymux that keeps a
   tile's spot open after its app moved away, so the other tiles do not resize (like
   the idle pane left behind in tmux). Placeholders exist only on `dwindle` and
   `scrolling` workspaces; floating windows never reflow, and Omarchy floats, resizes
   and titles every window arriving on a floating workspace (`workspace-modes.lua`,
   `watch()` / `arrange()`), so a placeholder there could neither keep a spot nor stay
   invisible. Details in section 5.7.
6. **Special workspaces** (scratchpad, Quake console, minimized-to-dock) are outside
   the model: they belong to no session, do not appear in the bar or tree, and are not
   captured for relaunch. ymux's own placeholder stash (section 5.7) is one of
   these. **Minimized windows:** Omarchy's minimize (floating workspaces only) tags a
   window `omarchy-minimized-from-<id>` and moves it to `special:minimized`
   (`omarchy-hyprland-window-minimize`); ymux counts such a window as a tile of
   workspace `<id>` for deletion and id reuse **only while it is both tagged and on
   `special:minimized`** (the same test Omarchy's busy check uses; a restored window
   keeps its stale tag, so the tag alone is not enough), as Omarchy itself counts that workspace
   as busy (`omarchy-hyprland-workspace-layout-default`). This only keeps the origin
   workspace and its id alive while the window is minimized. Both docks restore a
   minimized window to the **focused** workspace, not its origin (Omarchy's
   `Dock.qml` `bringForward()`; the user's rosakodu.dock `scripts/dock-minimize.py`);
   ymux then records the tile on that workspace, and the origin workspace empties
   and is deleted by the normal rules (section 5.6). rosakodu.dock's own minimize sets
   no tag, so its minimized windows are not covered by this rule. The user does not
   currently minimize windows; the rule costs nothing when unused.
7. **Names live in ymux state only.** ymux never renames Hyprland workspaces;
   their Hyprland names stay their numeric ids.

All commands act on the **focused monitor**, the same way Super + 1-0 does today.

**Source of truth:** the ymux daemon's state, not Hyprland. Hyprland deletes empty
workspaces that are not shown, so a workspace in ymux state may briefly have no
Hyprland workspace behind it; ymux creates it again when it is switched to.

## 2. Keys

"Leader" = Super + B (verified free: no live bind with SUPER only + B).

The keys below are the **defaults**. The leader key, every leader action key and
Shift + Ctrl + Alt + H / L can be changed in `~/.ymux.conf` (section 6.3). The
Omarchy binds ymux takes over (further down) keep Omarchy's keys and are not
configurable in v1.

| Keys | Action |
|---|---|
| Leader, then `c` | Create a workspace in the current session (lowest free number) and switch to it. Like a new tmux window always holds a shell, it starts with one placeholder filling it (section 5.7) |
| Leader, then `:` | Open the command prompt (section 5.3); `new` creates a session |
| Leader, then `$` | Rename the current session |
| Leader, then `,` | Rename the current workspace |
| Leader, then `w` | Open the tree picker (monitors > sessions > workspaces > tiles); picking an entry switches to it. This is the only way to switch sessions for now |
| Leader, then `o` | Open the options modal |
| Leader, then `q` | Show a number over every visible tile on all monitors; pressing a number focuses that tile (section 5.5) |
| Leader, then `m` | Mark the focused tile (one mark at a time, like tmux `select-pane -m`) |
| Leader, then `v` | Swap the focused tile with the marked tile, across workspaces and monitors (like the user's tmux `bind v swap-pane`, `~/.tmux.conf:106`). Always a plain swap: if one side is a placeholder, the app takes the placeholder's spot and the placeholder takes the app's old spot, so that spot stays open |
| Leader, then `x` | Close the focused tile at once, no confirmation (user decision 2026-11-15; Super + W does the same, takeover 9): a placeholder closes and the other tiles fill the space; a tiled app is closed and an empty spot (placeholder) holds its place (section 5.7); a floating window just closes |
| Leader, then arrows (B released, Super held) | Resize the focused tile; arrows repeat while held; releasing Super ends resize mode |
| Shift + Ctrl + Alt + H / L | Previous / next workspace in the current session, wrapping around at either end like tmux `previous-window` / `next-window` (verified free) |
| Escape (inside leader mode) | Leave leader mode |

**Leader mode details:**

1. Leader mode is a Hyprland submap. Action keys are **plain only** (user decision
   after spike S2): after Super + B the user releases Super and presses the key.
   Holding Super at the action key reaches the catch-all and just ends leader mode.
   The submap also binds `Super_L` / `Super_R` (with and without `SUPER + `) as no-ops,
   so pressing Super inside leader mode does not end it (spike S2).
2. `$` is bound as SHIFT + 4 and `:` as SHIFT + semicolon.
3. Any other key leaves leader mode and is swallowed (not passed to the app), like tmux.
   This uses a catch-all bind inside the submap, `hl.bind("catchall", ...)`, confirmed
   by spike S2.
4. Leader mode also ends by itself after 2 s with no key (a fresh `oneshot` timer per
   leader press; a fired oneshot timer cannot be re-armed, spike S2). The first arrow press switches
   to resize mode, which is a second submap (`ymux_resize`) with no timeout.
5. While leader mode or resize mode is active, the ymux bar widget shows a `LEADER` or
   `RESIZE` marker so the mode is never invisible. A marked tile shows `MARK` in the bar
   until the mark is used or cleared (marking the same tile again clears it).
6. **Resize mode:** the arrow binds inside `ymux_resize` (and the first-arrow binds in
   the leader submap) are SUPER + arrow, repeating. A repeat timer polls
   `hl.is_key_down` for Super_L and Super_R, and leaves the submap the moment neither is
   down. This copies the user's working mouse mode (`~/.config/hypr/bindings.lua:79-139`,
   polling at `:84-91`). Steps: 50 px per press.

**Omarchy binds ymux takes over** (defaults in
`/usr/share/omarchy/default/hypr/bindings/tiling.lua`; replaced with `o.rebind`, whose
`hl.unbind(keys)` removes the default by exact key spec,
`/usr/share/omarchy/default/hypr/helpers.lua:154-155`, `o.rebind` defined at `:154`,
`hl.unbind` at `:155`):

1. Super + 1-0 (`:25`): go to workspace N of the current session. If the session has no
   workspace N, nothing happens (like tmux).
2. Super + Shift + 1-0 (`:26`): move the focused app to workspace N of the current
   session and switch there with it. The spot it left becomes a placeholder (section
   5.7) if the source workspace is `dwindle` or `scrolling`; on a `floating` source no
   placeholder is left. In a `dwindle` or `scrolling` target workspace it fills the
   first placeholder (top-left first) if there is one, otherwise it tiles in normally;
   a `floating` target never has placeholders. If the session has no workspace N, it is
   created with that number and the app moves there, like tmux `move-window` to a free
   index (user decision at acceptance; Super + N alone still does nothing). With no app
   focused, nothing happens. On a placeholder, nothing happens.
3. Super + Shift + Alt + 1-0 (`:27`): same as 2, without switching.
4. Super + Tab / Super + Shift + Tab (`:36-37`): next / previous workspace in the
   current session, wrapping around at either end (user decision at acceptance).
5. Super + Ctrl + Tab (`:38`): the previously shown workspace, within the current
   session.
6. Super + mouse wheel (`:80-81`): next / previous workspace in the current session.
7. Super + Shift + Alt + arrows (`:40-43`, move workspace to another monitor): move the
   **current session** to the monitor in that direction, with all its workspaces and
   tiles (Omarchy's version would renumber workspaces). On the target monitor the moved
   session becomes the active session and focus goes with it (tmux-like, and what
   Omarchy's version does today). The session that monitor was showing stays there,
   inactive, reachable through the tree. The monitor it came from switches to its most
   recently used other session, or to a new `main` if it has none left. No monitor in
   that direction: nothing happens. The workspaces keep their real ids (section 3.2
   item 2).
8. Alt + Tab / Alt + Shift + Tab (`:57-60`). Omarchy binds each key twice: Alt + Tab
   to `window.cycle_next` (`:58`) and `window.bring_to_top` (`:60`), Alt + Shift + Tab
   to `window.alter_zorder` bottom (`:57`) and `window.cycle_next` with `next = false`
   (`:59`). ymux removes **both** binds on each key and binds one replacement per
   key that calls the daemon. Hyprland's own cycling cannot skip a class
   (`cycleNext` takes no filter), so the daemon picks the next / previous window on the
   focused workspace itself, in Hyprland's cycle order, skipping placeholders, and
   dispatches focus to it. It keeps Omarchy's raise behaviour: Alt + Tab raises the
   newly focused window to the top; Alt + Shift + Tab first lowers the window it leaves
   to the bottom. Spike S2 confirms `hl.unbind` removes every bind on a key spec; if it
   removes only one, `ymux.lua` unbinds repeatedly until none remain.
9. Super + W, Omarchy's "Close window" (`tiling.lua:1`, user decision 2026-11-15): it
   becomes ymux's close, the same as Leader + `x` (a tiled app leaves an empty spot;
   a placeholder or floating window closes at once).

Left untouched: Super + Shift + K, which (user decision, phase 5) now opens the user's
Window overview panel instead of the old window switcher script; Alt + Tab moved from that
overview to ymux (takeover 8),
Super + Shift + Ctrl + arrows (move a window to another monitor; the window lands in
that monitor's active session; no placeholder is left), and every other bind.

**Key specs must match Omarchy's.** Omarchy binds the number row by keycode
(`SUPER + code:10` to `code:19`, `tiling.lua:23-28`), and `o.rebind` unbinds by exact
key spec, so `ymux.lua` uses the same `code:` specs and the same modifier spelling
for every taken-over bind.

Workspace numbers: the number keys reach workspaces 1-10 of a session; a session can
have more than 10, reachable through the tree, the widget and H / L.

## 3. Architecture

Three pieces:

1. **`ymux` (daemon + CLI), Rust.** Owns all state: monitors, sessions,
   workspaces, the mapping onto real Hyprland workspaces, placeholders, persistence and
   relaunch. One binary: `ymux daemon` runs the daemon; `ymux placeholder` runs
   one placeholder window (section 5.7); every other subcommand (`ymux focus 3`,
   `ymux new`, ...) is a thin client that sends one request over the socket and
   exits.
2. **`~/.config/hypr/ymux.lua` (Hyprland config).** Keys and submaps can only live in
   Hyprland config, and must survive config reloads. It is **generated** from
   `~/.ymux.conf` (section 6.3) and never edited by hand: `ymux install` writes it,
   and the daemon regenerates it per section 6.3, reloading Hyprland only when the
   generated content changes.
   Loaded by one line,
   `require("hypr.ymux")`, added to `~/.config/hypr/hyprland.lua` after
   `require("hypr.bindings")` (`hyprland.lua:24`). It contains:
   - the leader submap and resize mode (section 2);
   - the rebinds of section 2, each calling the CLI (e.g. `ymux focus 3`);
   - window rules for placeholders (section 5.7);
   - the takeover of Omarchy's per-monitor workspace system (section 3.2);
   - starting the daemon if it is not running. This runs as top-level code, so it
     happens on every config evaluation (Hyprland start and every reload), not only on
     `hyprland.start`; `ymux daemon` exits at once if one is already running, so
     repeated starts are harmless. Single instance: the daemon takes an exclusive
     `flock` on `$XDG_RUNTIME_DIR/ymux.lock`, waiting up to 5 s for it (an old daemon
     from a just-ended Hyprland may still be exiting) before giving up. Giving up exits
     with a desktop notification (`ymux: another daemon is still running`); the next
     config evaluation retries through the same top-level start. A daemon never removes
     the socket file when it exits: removing a stale socket belongs only to the next
     lock-holder, so an exiting daemon can never delete a newer daemon's socket; if it gets the lock it removes any
     stale socket file and binds a fresh one. A crashed daemon releases the lock, so the
     next start works. **The daemon never outlives its Hyprland:** Hyprland starts it as
     a separate process (its own systemd scope), so it does not die with Hyprland by
     itself; it exits, releasing the lock, as soon as its socket2 connection closes or
     the Hyprland instance it was started from (`HYPRLAND_INSTANCE_SIGNATURE`) is no
     longer listed under `$XDG_RUNTIME_DIR/hypr/`. A new Hyprland therefore always
     starts a fresh daemon. Spike S3 confirms `hl.exec_cmd` works from top-level config code
     (Omarchy itself only execs inside `hyprland.start`, `autostart.lua`); if it does not,
     the start moves to a `config.reloaded` hook plus `hyprland.start`.
3. **`synju.ymux` Omarchy shell plugin (QML).** Lives in
   `~/.config/omarchy/plugins/synju.ymux/`, structured like the user's working
   `rosakodu.dock` plugin (`~/.config/omarchy/plugins/rosakodu.dock/manifest.json`:
   kinds `service`, `bar-widget`, `menu`, `keepLoaded: true`). Entry points:
   - `service`: one instance for the whole shell. Holds the single `ymux watch`
     connection and exposes the state to the other parts. (The bar creates one widget
     per monitor, so the connection must not live in the widget.)
   - `barWidget`: the status line, one instance per monitor, reading from the service.
   - `menu`: ONE entry point for every overlay (tree picker, command prompt, rename
     prompt, options modal, tile numbers), switching on the summon
     payload, e.g. `omarchy-shell shell summon synju.ymux '{"view":"tree"}'`. The
     shell loads the plugin's menu root once and the plugin creates its own windows,
     so the tile-numbers view can open one surface per monitor.

### 3.1 Communication

- **Socket:** `$XDG_RUNTIME_DIR/ymux.sock`, newline-delimited JSON requests/replies.
- **State stream:** `ymux watch` prints the full state as one JSON line on connect
  and again on every change. If the daemon is not running, the plugin service retries
  every 2 s and the widget shows a dim `ymux` label (never a silent blank).
- **Plugin to daemon:** clicks and overlay choices call the CLI via `bar.run(...)`.
- **Daemon to Hyprland, two channels:**
  - `hyprctl dispatch '<lua>'` for dispatchers: expressions that evaluate to a
    dispatcher, as Omarchy's widget does (`Workspaces.qml:102-111`).
  - `hyprctl eval '<lua>'` for config calls that act directly and return nothing, such
    as `o.workspace_mode(...)`; Omarchy's own layout toggle applies modes this way
    (`omarchy-hyprland-workspace-layout-toggle`).
  - Hyprland's event socket (`socket2`) for window, workspace, monitor and
    `configreloaded` events.
- **CLI failures:** if the daemon is unreachable, the CLI exits non-zero and sends a
  desktop notification (`notify-send ymux "daemon not running"`). A key press never
  fails silently.

### 3.2 Taking over Omarchy's per-monitor workspaces

Omarchy splits Hyprland's single workspace list into blocks of ten per monitor
(`monitor-workspaces.lua:1-8`) and, on every config reload, Hyprland start and monitor
plug or unplug, runs `reconcile` (`:671-703`). Its `merge()` step (`:622-647`) renumbers
any occupied workspace outside a connected monitor's block into that monitor's block.
That would scramble ymux's workspaces, so ymux takes over this job.

1. **Turning Omarchy's off:** `ymux.lua` sets `o.workspaces.reconcile` to a function
   that does nothing. Every Omarchy hook calls it through that table
   (`monitor-workspaces.lua:675, 688, 692, 701`; `o.workspaces = M` at `:705`), so this
   switches off all of them without editing Omarchy's files. The replacement sends
   nothing: monitor events and config reloads both reach the daemon only through
   socket2. One event, one path.
2. **Real ids:** ymux allocates Hyprland workspace ids itself, from ranges that no
   Omarchy block can ever own: the first monitor ymux sees gets 100001-199999, the
   second 200001-299999, and so on. A monitor keeps its slot (and range) for good,
   plugged in or not. A new workspace takes the lowest id in its current monitor's range
   that no live workspace in ymux state uses (any monitor, any session), so ids are
   reused once their workspace is deleted (section 5.6). A workspace keeps its real id
   across daemon restarts, reloads and session moves. Because no ymux id falls inside
   an Omarchy block, uninstall puts every workspace back on the monitor it is already on
   (section 8.2).
   **Layout mode files:** Omarchy stores a workspace's non-default layout mode in
   `~/.local/state/omarchy/workspace-layouts/<id>.lua` and re-applies them on every
   config load (`workspace-layouts.lua`, `workspace-modes.lua`). ymux mirrors
   Omarchy's own toggle (`omarchy-hyprland-workspace-layout-toggle`) exactly:
   - setting a mode: apply it with `hyprctl eval` of `o.workspace_mode(...)`, then write
     `<id>.lua`, **or delete `<id>.lua` when the mode equals the user's default layout**
     (`omarchy-hyprland-workspace-layout-default`), so default-mode workspaces keep
     following the default;
   - renumbering a workspace (including adoption): move `<old>.lua` to `<new>.lua`,
     rewriting the id inside; a default-mode workspace has no file, so nothing moves;
   - deleting a workspace: delete `<id>.lua`, so a reused id never inherits a dead
     workspace's mode.
3. **First run (adoption):** each connected monitor gets one session named `main`, whose
   workspaces are that monitor's existing occupied workspaces, renumbered once into the
   monitor's ymux range in slot order (Omarchy's 3 becomes `main` workspace 3). Their
   layout mode files move with them (item 2), so a `scrolling` workspace stays
   scrolling.
4. **Switching session** = showing the target session's last-shown workspace on that
   monitor, creating it if Hyprland has deleted it.
5. **Monitors coming and going: one invariant.** Every session belongs to a monitor.
   The daemon re-establishes this invariant after **every** `monitoradded`,
   `monitorremoved`, `moveworkspace` or `createworkspace` event it did not cause (an
   outside actor recreating a known ymux id creates it on the focused monitor with
   no `moveworkspace`), in this order:
   1. **Gone monitor:** each session whose monitor is no longer connected becomes a
      normal session of the monitor its workspaces now sit on (Hyprland moves an
      unplugged monitor's workspaces to a remaining one by itself). If they landed on
      different monitors, the session goes to the monitor holding most of them (ties:
      the focused monitor), and its other workspaces are moved there. Names, workspaces and
      numbers are kept; names may clash, session numbers tell them apart. The receiving
      monitor's active session does not change.
   2. **Connected monitors:** every workspace whose session belongs to a connected
      monitor but which sits on a different monitor is moved back to its session's
      monitor. This undoes Hyprland returning workspaces to a replugged monitor
      (Omarchy notes this behaviour, `monitor-workspaces.lua:16-17`), so a replugged
      monitor's old sessions **stay where they are now**.
   3. **Monitor with no sessions** (first time, or returning after its sessions moved
      away): gets a session `main` with workspace 1.
   Sessions can be moved back by hand with Super + Shift + Alt + arrows (section 2,
   item 7). **Settling:** Hyprland moves a leaving monitor's workspaces before it reports
   the removal, and moves them back before it reports a replug (spike S1). So the daemon
   runs the invariant only once these events have settled (300 ms with no further
   monitor or workspace-move event), reading the live monitor list.
6. **Focus from outside ymux:** if anything else (Super + Shift + K, the dock, a
   notification click, an app asking for focus) shows a workspace belonging to a
   different session, that session becomes the monitor's active session. ymux
   follows focus; it never bounces it back.
7. **A window moved to another monitor** (dragged with the mouse, or Super + Shift +
   Ctrl + arrows) joins the workspace it lands on, in the target monitor's active
   session. Both monitors stay on their current sessions. This is the same as Omarchy's
   behaviour today; ymux only records the session. No placeholder is left.
8. **Which kind of start:** the daemon saves Hyprland's session id
   (`HYPRLAND_INSTANCE_SIGNATURE`, inherited from Hyprland, which started it) in
   `state.json` on every write. At start:
   - **no `state.json`** (first install, or any install after an uninstall, which
     deletes all state, section 8.2): adoption (item 3);
   - **same id:** the daemon restarted while Hyprland kept running (a crash, or a new
     build installed during development): the restart rules below apply; no relaunch;
   - **different id:** Hyprland has just started: relaunch (section 7.3). Saved window
     addresses and pids (section 7.2) are discarded, since they belonged to the old
     Hyprland session.
   **Start scan, on every start (all three cases):** the daemon reads the live
   workspaces and windows. A live window not accounted for (not matched below, and
   not one the daemon spawned) is recorded: on a **known** ymux workspace, as a new
   tile of that workspace; on an **unknown** id, by the unknown-workspace rule (item
   10). So windows opened before the daemon connected to socket2 are never lost. On a
   **different-id start with relaunch on**, recording waits until the restore (section
   7.3) has finished claiming: windows claimed by a saved tile take that tile's record,
   and only windows still unclaimed afterwards are recorded by the rules above.
   **Landing (different id):** before spawning anything, the daemon switches each
   monitor to its active session's last-shown workspace, creating it if needed, so the
   user never stays on Hyprland's fresh default workspace (which belongs to no session)
   and the bar matches what is shown. Hyprland's default workspace, once left empty, is
   simply destroyed by Hyprland; it was never in ymux state. With relaunch on, the
   restore (section 7.3) then runs.
   **Restart rules (same id):** each live window is matched to a saved tile by window
   address **and** pid (both recorded in section 7.2; an address can be reused by a
   later window, so an address with a different pid is no match). A matched tile record
   stays with its window, so each live window has exactly one record. Saved app tiles
   with no live window are dropped (their apps closed while ymux was down); dropping
   a workspace's last saved tile counts as that tile closing (section 5.6), so such a
   workspace is deleted at restart unless it is the one shown. Saved
   placeholders with no live window are recreated next to their saved neighbour if their
   workspace's live mode (item 11) is `dwindle` or `scrolling` (not resized, so they
   take dwindle's split), and dropped if it is `floating`. A saved workspace left with no tiles is deleted by the
   normal rules (section 5.6). Names of surviving sessions and workspaces are kept.
   **Unknown workspaces found by the scan** go into the active session of **the monitor
   they are on** as new workspaces, renumbered into that monitor's ymux range
   (`hl.dsp.workspace.change_id`, moving the layout mode file with it, item 2), so no
   ymux workspace ever keeps an id inside an Omarchy block.
9. Omarchy's `o.workspaces.move_to_monitor` (which renumbers) is no longer reachable
   from any key: its binds are rebound to the session move (section 2, item 7).
   Hyprland sends `changeworkspaceid>>old,new` when a workspace changes id (spike S1).
   ymux follows renumbering it did not cause from that event, plus a fallback check
   of `hyprctl -j workspaces` every 30 s (alongside the geometry read,
   section 6.2): a known workspace's windows found under a new id move the mapping to
   that id, logged, **only if** the old id's workspace is gone from Hyprland and the
   new id is not already mapped in ymux state (otherwise windows moved by hand
   between two live workspaces would re-map one onto the other's id). Spike S1 records
   what `change_id` actually emits, including whether `renameworkspace` fires.
10. **Unknown workspaces while running:** a window opening on, or a workspace moving to,
    an id that is not in ymux state (for example a window rule naming a numeric
    workspace) is adopted live, **except** windows whose pid the daemon spawned itself
    (relaunched apps, placeholders), which the daemon places itself (section 7.3). It is
    adopted by the same rule as the start scan (item 8): it joins
    the active session of the monitor it is on as a new workspace, renumbered into that
    monitor's range exactly as in item 8.
11. **Layout mode: where ymux reads it.** Omarchy changes modes without telling
    ymux (Super + L runs `omarchy-hyprland-workspace-layout-toggle`;
    `omarchy-hyprland-workspace-layout-default` rewrites or deletes `<id>.lua` files).
    So ymux never caches the mode as truth: it reads it exactly as
    `omarchy-hyprland-workspace-layout-current` does: `<id>.lua` if it exists; else,
    if Omarchy's `default.lua` exists, the result of
    `omarchy-hyprland-workspace-layout-default` (which falls back to `general.layout`);
    else the workspace's live `tiledLayout` from `hyprctl -j workspaces`; else
    `dwindle` (so a ymux workspace with no live Hyprland workspace and no files reads
    as `dwindle`). It reads it whenever a decision depends on it
    (placeholders, section 2 item 2, section 5.7) and on the 30 s check (section 6.2).
    The value recorded for relaunch (section 7.1) is the last one read. If
    `omarchy-hyprland-workspace-layout-default` deletes the file of a ymux workspace
    that is currently empty in Hyprland, that workspace falls back to the default
    layout; this is accepted (section 11).

## 4. Bar widget

Replaces Omarchy's built-in workspace widget (`omarchy.workspaces`,
`/usr/share/omarchy/shell/plugins/bar/widgets/Workspaces.qml`) in the same place:
`bar.layout.left`, after `omarchy.menu`, in `~/.config/omarchy/shell.json`.

1. **Look:** like the tmux status line: session name first, then each workspace of that
   session as `name number`, the current one highlighted. Example:
   `SC Neural Nets | Net Extraction 1 | 2` (workspace 2 unnamed). When another session
   on any monitor has the same name, the session number is shown after it, e.g.
   `main (3)` (user decision at phase 3 acceptance). An unnamed workspace, or one
   holding only placeholders, shows its number only.
2. **Per monitor:** each monitor's bar shows that monitor's active session.
3. **Click:** clicking a workspace switches to it.
4. **Mode marker:** shows `LEADER` / `RESIZE` while those modes are active, and `MARK`
   while a tile is marked.
5. **Orientation:** works with the bar at top or bottom (the user plans to move it to
   the bottom later). Left/right bars: out of scope for v1; shows numbers only.
6. **Layout glyph:** not shown in v1. Omarchy's layout-toggle script asks
   `omarchy.workspaces` to refresh (`omarchy-hyprland-workspace-layout-toggle:89`); with
   that widget disabled the call does nothing, which is harmless.

## 5. Overlays and behaviour

All overlays are views of the one `menu` entry point (section 3, item 3).

1. **Tree picker (Leader, `w`):** collapsible tree, monitors > sessions > workspaces >
   tiles, with the current item preselected. Sessions show `name (number)` when names
   clash, as the bar does. Placeholders are not listed; a workspace holding only
   placeholders shows as empty. Keyboard: arrows or j/k to move, Enter to switch, `x`
   to kill (item 6), Escape to close. Picking a tile switches to its session and
   workspace and focuses it. Picking a session on another monitor focuses that monitor
   too. `h` / `l` fold and unfold a row; what is folded is remembered in state
   (`folded`, user request 2026-11-15), so the tree opens as it was left, across
   restarts and reboots; a deleted session or workspace drops its fold. `r` renames
   the selected row, whatever it is (user request 2026-11-15), with the rename prompt
   prefilled with its current name: a session or workspace (as Leader + `$` / `,`), a
   monitor (named by its key, make, model and serial, so the name follows the screen to
   any port; shown instead of connector and description), or an app (the window's
   tile; shown instead of class and title, kept through moves, daemon restarts and a
   relaunch). An empty name clears it, and the row shows its default label again.
2. **Rename prompt (Leader, `$` or `,`):** one text field prefilled with the current
   name. Enter saves, Escape cancels. An empty name resets a workspace to its automatic
   name; an empty session name is refused with a message in the prompt.
3. **Command prompt (Leader, `:`),** like tmux's `prefix + :`: one text field. v1
   commands:
   - `new`: create a session on the focused monitor with the next free session number
     (across all monitors), named after that number, with workspace 1, and switch to it;
   - `new <name>`: the same, named `<name>`.
   Any other input shows `unknown command: <input>` in the prompt. Enter runs, Escape
   cancels.
4. **Options modal (Leader, `o`):** v1 holds one setting:
   - **Relaunch apps on restart:** on/off, default on.
   Changing it rewrites that one line in `~/.ymux.conf` (section 6.3), leaving the
   rest of the file, including comments, as it was.
5. **Tile numbers (Leader, `q`),** like tmux's `prefix + q` (`display-panes`):
   - every visible tile on every monitor, placeholders included, gets a large number
     drawn over it;
   - numbering runs over monitors left to right, and within a monitor over tiles left
     to right, top to bottom; tiles 1-9 get digits, the 10th onwards get letters
     (a, b, c, ...), so one keypress always picks a tile;
   - pressing a shown number or letter focuses that tile immediately, switching
     monitor if needed (this is also how a placeholder is focused, e.g. to close it or
     to swap into it);
   - the overlay closes after 2 s (matching the user's tmux
     `display-panes-time 2000`, `~/.tmux.conf:46`), on Escape, or on any key that is
     not shown.
6. **Closing things, like tmux:**
   - **Tile:** Leader + `x` or Super + W (section 2), no confirmation. An app tile is
     closed by asking the app to close where it is; once it has really closed, a
     placeholder keeps its spot (section 5.7). An app that asks first (Brave's "close 2
     tabs?", an unsaved-changes dialog) stays where it is, untouched, for as long as it
     takes; cancelled, nothing changes.
   - **Workspace:** deletion is **event-driven**, never a sweep: a workspace is deleted
     when its last tile closes or moves away, or when it is left (stops being shown)
     while it has no tiles, even if it was renamed (tmux closes a window when its last
     pane exits). A workspace that is restored empty (relaunch off, section 7.5, or all
     its apps failed to relaunch, section 7.3) is therefore kept until such an event:
     until it is shown and left empty, or killed from the tree.
     A workspace that is shown is never deleted; deletion waits until it stops being
     shown. This also covers a workspace that never held a tile and has no placeholder:
     it stays while shown and goes when you leave it empty. (A fresh workspace from
     Leader + `c` or a new session starts with a placeholder, so it stays until that
     placeholder is closed.) The other
     workspaces keep their numbers; gaps are left, as tmux does by default. A workspace
     holding only placeholders (parked ones included, 5.7) is not empty and stays.
   - **Session:** when its last workspace is deleted, the session is deleted. Since a
     shown workspace is never deleted, the monitor's active session is never deleted
     while shown; it goes when you switch away from it empty.
   - **Kill from the tree:** in the tree picker, `x` on a session or workspace asks
     `Kill <name>? (y/n)`, then asks every window in it to close (placeholders close at
     once, and a saved floating layout, section 5.7, is dropped). The workspace or session is deleted through the rules above as its windows
     actually close; ymux never deletes state while windows remain. If any window is
     still open 10 s later, a notification names it. Killing the monitor's active session
     switches the monitor to its most recently used other session, or to a new `main`
     if it has none, once the killed session is gone.
7. **Placeholders** (only on `dwindle` and `scrolling` workspaces, section 1 item 5):
   - **What one is:** a `ymux placeholder` process showing one Wayland toplevel window
     with app id `ymux-placeholder`, a fully transparent buffer and no content.
     Window rules in `ymux.lua` give it no border, no shadow, no blur and no
     decoration. The daemon tracks each one (pid, window address, workspace).
   - **Where one appears** (user decision 2026-11-15):
     - when an app leaves its spot through a ymux move (Super + Shift + 1-0 or
       Super + Shift + Alt + 1-0);
     - when a tiled app is closed with Leader + `x` or Super + W (user decisions
       2026-11-15 and 2026-11-16): ymux records the workspace's layout and asks the app
       to close where it is, with no time limit, so a question the app asks first shows
       in place. When the app's window has really closed, a placeholder is opened,
       moved in next to the app's former neighbour, and the recorded layout is restored
       with the placeholder where the app was (the tree rebuild of the floating round
       trip, section 5.7; the workspace's windows briefly flicker). An app moved
       elsewhere before it closes leaves no spot. (Earlier the placeholder was swapped
       in first and the app hidden on the stash, which hid Brave's close question.)
     - in every **new workspace** ymux creates: Leader + `c`, a new session's
       workspace 1 (`new` in the command prompt), and the `main` a monitor gets when it
       has no sessions. One placeholder fills the workspace, like the shell a new tmux
       window starts with. Opening an app there takes its spot (filling rule below); the
       workspace then lives by its tiles, and closing the placeholder with Leader + `x`
       on an otherwise empty workspace deletes it (once left).
     An app closing by itself or closed any other way, dragging it, or moving it to
     another monitor leaves no placeholder.
     On a `floating` workspace no placeholder is created (section 1 item 5).
   - **How the spot is kept exactly:** the daemon opens the placeholder on a hidden
     special workspace (`special:ymux-stash`), swaps it with the leaving app (a swap
     keeps both tiles' positions and sizes), then moves the app on to its target. Filling
     a placeholder in the target workspace is the same swap in reverse, after which the
     placeholder is closed. This depends on Hyprland swapping windows across
     workspaces, which spike S4 confirms (the same primitive as Leader + `v`).
   - **Focus:** a placeholder takes focus like any window: clicking it, hovering it
     (Omarchy's focus-follows-mouse), Leader + `q`, or Alt + Tab skipping aside. This is
     the user's choice, like an idle tmux pane.
   - **Filling one:**
     - a **new app the user opens while a placeholder is focused** takes that
       placeholder's spot and size, and the placeholder closes: the daemon sees
       `openwindow`, swaps the new window with the focused placeholder, then closes the
       placeholder. It applies **only** when all of these hold: the window opens tiled
       (not a dialog or other floating window), on the same workspace as the focused
       placeholder, and its pid is not one the daemon spawned itself (placeholders,
       relaunched apps). It never applies during a restore (section 7.3);
     - Super + Shift + 1-0 into a workspace that has one: the app fills the first one
       (top-left first) and the placeholder closes;
     - Leader + `v` never closes a placeholder: it is a plain swap (section 2).
   - **Closing one:** Leader + `x` on it (focus it first with Leader + `q`, the tree
     cannot list it), or click it, then Leader + `x`. The other tiles fill the space.
   - **Hidden from switchers:** Alt + Tab skips placeholders (section 2, takeover item
     8). Super + Shift + K opens Omarchy's Window overview (user decision 2026-11-15;
     the old `~/.local/bin/window-switcher` script was deleted, so there is no script
     to filter). The tree and the bar do not show them.
   - **Relaunch:** placeholders are saved like tiles and recreated on restore (section
     7), so restored layouts keep their empty spots.
   - **Workspace switched to floating** (Super + L, or any mode change):
     - **Noticing:** Omarchy floats every window the moment the mode changes
       (`omarchy-hyprland-workspace-layout-toggle` applies it synchronously;
       `workspace-modes.lua` `place()` floats, resizes and centers them), so ymux
       cannot act first. It acts **as soon as it notices**: on the socket2 event
       Hyprland sends when a tracked placeholder becomes floating (the binary contains
       a `changefloatingmode` event; spike S4 confirms the exact name), with the 30 s
       mode check (section 3.2 item 11) as a fallback. At event time Omarchy's mode
       file lags (the toggle floats the windows before writing it and un-floats them
       before removing it), so the event path reads Omarchy's window tag instead: a
       workspace whose windows carry `omarchy-floating-workspace` is floating, otherwise
       its mode is Hyprland's own tiled layout (`dwindle` / `scrolling`).
     - **Parking** (redesign, user decision 2026-11-15; spikes.md "park"): ymux does
       not close the workspace's placeholders. It saves the workspace's layout (every
       tile, apps and placeholders, with its **last recorded tiled geometry** in state,
       section 6.2, never a fresh read, since by then the windows are already floating)
       and parks each placeholder: shrunk to 1 x 1 px (`hl.dsp.window.resize`,
       `relative = false`) and moved to the top-left corner of the workspace's monitor,
       under the bar, where it cannot block clicks. Parked placeholders are still the
       workspace's tiles, so the workspace stays (section 5.6).
     - **Returning:** when the workspace goes back to `dwindle` or `scrolling`, Omarchy
       re-tiles every window on it, the parked placeholders included, in one go, in an
       order that can give a different tree. ymux records the mode at once and, 300 ms
       later (once Hyprland has laid the windows out), restores the saved layout:
       - **Already right:** when every pair of saved windows sits the same way round
         (left of, above) as saved, only the sizes are set back (each tile resized to
         its saved size in reading order, `hl.dsp.window.resize`, `relative = false`),
         and only if the sizes differ.
       - **`dwindle`, otherwise: rebuild** (user decision 2026-11-15; spikes.md
         "rebuilding a dwindle tree"). ymux reads the saved split tree off the saved
         rectangles (the cut, vertical or horizontal, that separates them into two
         groups, recursively; a dwindle layout always has one), moves the workspace's
         windows to the stash, and brings them back one at a time: the first window of
         the tree fills the workspace; for each split, with the first window of its
         first half focused, the first window of its second half is moved in, which
         dwindle puts right of / below it; when the split box is wider than tall dwindle
         splits side by side, otherwise stacked, and where that is not the saved cut
         ymux sends `togglesplit` with the new window focused. Then each half is
         built the same way inside its window. Last, the tiles get their saved sizes in
         reading order, and focus goes back to the window that had it. Windows opened
         while floating are moved back in after the saved ones (and sizes are then left
         to dwindle); saved windows that closed meanwhile are left out of the tree. The
         windows briefly disappear and come back in place.
       - **`scrolling`:** each saved window is swapped into the spot nearest its saved
         position (`hl.dsp.window.swap`); columns keep Omarchy's widths.
       The saved layout is then dropped. Same fallback as noticing: the 30 s mode check
       parks or restores when an event was missed. The same rebuild restores layouts on
       relaunch (section 7).
     - **Lifecycle:** the saved layout is kept in `state.json` (section 6.1) and survives
       restarts; killing the workspace or session drops it. Uninstall deletes all state
       (section 8.2).

## 6. Persistence

1. **State file:** `~/.local/state/ymux/state.json`. Written atomically (write temp,
   rename) on every change ymux sees: session or workspace create, rename, switch,
   delete, window or placeholder opened, closed, moved or swapped, mark set or cleared,
   placeholders parked or rearranged.
   **Restore snapshot** (user decision 2026-11-15, after the first reboot test found
   the shutdown had emptied `state.json`): Hyprland closes every window as it shuts
   down, and ymux saw each close as the user's, so the saved tiles were gone by the
   next boot. ymux therefore also writes `relaunch.json` next to `state.json`, the
   same state, but only once no window has closed for 5 s and never while a relaunch
   runs; a shutdown's burst of closes never reaches it. When a new Hyprland starts
   (section 3.2 item 8), the daemon starts from `relaunch.json` (sessions, names,
   workspaces, tiles) instead of `state.json`; a daemon restart under the same
   Hyprland still uses `state.json`. A window closed less than 5 s before a reboot
   comes back.
2. **Tile sizes:** Hyprland sends no event when a split is resized by mouse or by
   Omarchy's resize keys, so geometry is captured by reading `hyprctl -j clients`
   whenever the state file is written, and additionally every 30 s unconditionally. The
   30 s read writes the state file only when the geometry differs from the last write.
   Order within each 30 s tick: the layout-mode check (section 3.2 item 11) and any
   placeholder parking (section 5.7) run **before** the geometry capture. A capture
   never replaces recorded geometry on a workspace whose recorded mode differs from its
   live mode (read as in section 3.2 item 11), that is, only in the short gap before the
   mode check catches up with a mode change. A window the user floats by hand on a
   workspace that stays `dwindle` or `scrolling` is captured normally.
3. **Config file: `~/.ymux.conf`,** one file for everything configurable, like
   `~/.tmux.conf`, with tmux-style lines:
   ```
   # leader and leader actions
   set leader SUPER+B
   bind c new-workspace
   bind : command-prompt
   bind w tree
   # ... one bind line per action in section 2's table
   bind-global SUPER+SHIFT+ALT+H previous-workspace
   bind-global SUPER+SHIFT+ALT+L next-workspace
   # options
   set relaunch on
   set leader-timeout 2000
   set display-panes-time 2000
   set resize-step 50
   # relaunch overrides for wrapped apps (section 7.4)
   launch-override <class> "<command>"
   ```
   - Install writes this file with the defaults **only if it does not exist**; an
     existing file (kept from an earlier install) is used as is.
   - Missing lines fall back to the defaults. A line ymux cannot parse is skipped
     and named in a desktop notification; the rest of the file still applies.
   - The daemon watches the file. On change it re-reads it and regenerates
     `~/.config/hypr/ymux.lua` (section 3 item 2) in memory; only if the generated
     content differs from the file on disk does it write it and reload Hyprland. Settings
     that only the daemon uses (`relaunch`, `launch-override`) never cause a reload, so
     the options modal (section 5.4) rewriting its line does not reload Hyprland.
   - It holds configuration only. Sessions, workspaces and placeholders are state
     (item 1), never written here.
4. **Load:** the daemon loads the state file at start. Sessions, names, numbers and real
   ids come back whatever the relaunch toggle says.
5. **Tile records are kept whatever the relaunch toggle says.** Window addresses,
   pids and geometry (section 7.2) are recorded with the toggle on or off: the restart
   rules (section 3.2 item 8) depend on them. The toggle only decides whether section
   7.3 relaunches anything.

## 7. Relaunch and layout restore

Recording (items 1 and 2) always happens (section 6.5); relaunching (items 3-5) needs
the relaunch toggle on.

1. **Recorded per workspace:** its layout mode (`dwindle`, `scrolling` or `floating`).
2. **Recorded per tile:** monitor, session, workspace, whether it is a placeholder,
   class, title, launch command (`/proc/<pid>/cmdline`), working directory
   (`/proc/<pid>/cwd`), floating or tiled, position and size, and its window address
   and pid (valid only within the Hyprland session id saved in state, section 3.2
   item 8).
3. **On Hyprland start** (a different saved Hyprland session id, section 3.2 item 8;
   never on a daemon restart while Hyprland runs, and never on the first start after an
   install, which adopts instead): for each saved
   workspace, set its layout mode first (section 3.2,
   item 2), then open its tiles one at a time into its real workspace, in the saved
   order (left to right, top to bottom): apps by their launch
   command, placeholders with `ymux placeholder`. **Claim before launch:** before
   launching a saved app, the daemon looks for an open window of the same class and
   launch command that no tile has claimed yet (for example a copy Omarchy's autostart
   already opened); if there is one, it moves that window into the saved spot instead of
   launching a second copy. Otherwise the daemon spawns the command itself (so it knows
   the pid; `hl.exec_cmd` returns none) and moves the window to its workspace once it
   appears. Wait for each spawned window by pid only; a window of the same class is
   accepted only if it is unclaimed and appeared after the spawn. Give up after 15 s
   and log it. Then:
   - `dwindle`: resize the splits to the saved sizes;
   - `scrolling`: windows come back in their saved left-to-right order; column widths
     are not restored in v1 (see limits);
   - `floating`: move and resize each window to its saved position and size.
4. **Honest limits:**
   - Apps come back fresh: no game state, unsaved buffers or scrollback.
   - A terminal comes back as a terminal in its saved directory, not running whatever
     was in it.
   - Tiled layouts are rebuilt by launch order then resizing, so a slow-starting app can
     leave a split slightly off until it is resized again.
   - Apps started through a wrapper (Flatpak, launchers, Electron helpers) may need a
     per-class `launch-override` line in `~/.ymux.conf` (section 6.3); v1 logs these
     instead of guessing.
   - `scrolling` workspaces get their window order back but not their column widths:
     `hyprctl -j clients` reports window geometry, not scroller columns.
5. **Toggle off:** sessions, names and empty workspaces only; no relaunch (apps or
   placeholders), no layout restore. Each monitor still lands on its active session
   (section 3.2 item 8), and the empty workspaces stay until a deletion event (section
   5.6).

## 8. Install and uninstall

1. **Install** (`ymux install`, run once per user; packaging decision 2026-11-16):
   - uses the binary where it is when run as the package's `/usr/bin/ymux` (removing
     an older `~/.local/bin/ymux`); any other build, e.g. one built from source, is
     copied to `~/.local/bin/ymux`, which then takes precedence;
   - writes `~/.ymux.conf` with the defaults if it does not exist (section 6.3);
   - adds `require("hypr.ymux")` to `~/.config/hypr/hyprland.lua` and generates
     `ymux.lua` from `~/.ymux.conf`;
   - installs the plugin, runs `omarchy plugin disable omarchy.workspaces`, enables
     `synju.ymux` and places it at `bar.layout.left` index 1. If disabling a built-in
     widget that way is not supported (verified in the Core phase), install instead
     edits `bar.layout.left` in `~/.config/omarchy/shell.json` directly, replacing
     `{ "id": "omarchy.workspaces" }` with `{ "id": "synju.ymux" }`;
   - reloads Hyprland **first**, so `ymux.lua` has switched Omarchy's reconcile off
     before anything is renumbered; `ymux.lua` then starts the daemon, which runs
     adoption (section 3.2, item 3) on its first start.
2. **Uninstall** (`ymux uninstall`, run per user before removing the package; it runs
   the uninstall script built into the binary). Nothing is kept except `~/.ymux.conf`.
   It:
   1. stops the daemon, then closes every placeholder window by class
      (`ymux-placeholder`);
   2. removes the `require("hypr.ymux")` line and deletes
      `~/.config/hypr/ymux.lua`, so Omarchy's own keys and its per-monitor workspace
      system come back;
   3. disables `synju.ymux`, removes `~/.config/omarchy/plugins/synju.ymux/`, and
      re-enables `omarchy.workspaces` in its old place;
   4. deletes the layout mode files in `~/.local/state/omarchy/workspace-layouts/`
      whose id is 100001 or higher (ymux's ranges);
   5. deletes all ymux state: `~/.local/state/ymux/` (sessions, workspaces,
      placeholders, saved layouts, tile records);
   6. removes `~/.local/bin/ymux` (the package's `/usr/bin/ymux` is removed with
      `pacman -R ymux`, which it says);
   7. reloads Hyprland. Omarchy's reconcile then pulls every occupied ymux workspace
      into each monitor's 1-10, so no window is lost; any that do not fit stay as extra
      workspaces after those ten.
   It prints what it did at each step.
   **Kept:** `~/.ymux.conf` only, like uninstalling tmux leaves `~/.tmux.conf`. A
   later install is a brand-new installation that picks up that config (section 6.3)
   and adopts the open workspaces into one `main` session per monitor (section 3.2
   item 3). To remove the config too, the user deletes that file.
   **Lost:** all sessions, workspace names, placeholders and saved layouts; and the layout
   modes of workspaces adopted at install, which come back in the default layout.

## 9. Delivery order

Each phase is usable and verified on the real desktop before the next starts.

1. **Spikes:**
   - S1: replacing `o.workspaces.reconcile` from `ymux.lua` switches off all
     renumbering (config reload, monitor unplug/replug); ymux ids (100001 and up)
     stay put; record the socket2 event order on unplug and on replug, and confirm the
     invariant of section 3.2 item 5 leaves every workspace on its session's monitor;
     record what `hl.dsp.workspace.change_id` emits on socket2, including whether
     `renameworkspace` fires (section 3.2 item 9).
   - S2: leader submap: catch-all bind availability, bare + SUPER binds, Super-release
     exit for resize mode.
   - S3: `hl.exec_cmd` from top-level `ymux.lua` code starts the daemon on config
     load and reload, and the daemon inherits `HYPRLAND_INSTANCE_SIGNATURE`; after
     Hyprland exits, the daemon exits on its own (section 3 item 2).
   - S4: placeholders: a transparent `ymux-placeholder` toplevel tiles like an app
     on `dwindle` and `scrolling` workspaces; `hl.dsp.window.swap` with a target window
     works across workspaces and monitors, including with one window on a special
     workspace (the stash); after a swap each window has the other's position and size,
     checked separately for a `dwindle` split and a `scrolling` column; a new window
     opened while a placeholder is focused can be swapped into its spot without other
     tiles moving; the exact socket2 event name for a window becoming floating
     (`changefloatingmode` or similar) fires for placeholders when Super + L switches a
     workspace to floating.
2. **Core:** daemon, socket, CLI, `ymux.lua`, state file, adoption, monitor
   handling, Leader + `c`, the taken-over keys and H / L. Session creation and switching
   via temporary CLI commands. Install and uninstall script.
3. **Bar widget** (with the plugin service).
4. **Overlays:** tree picker, command prompt, rename prompt, options modal, tile
   numbers.
5. **Tiles:** placeholders, Leader + `m` / `v` / `x`, resize mode.
6. **Relaunch, then layout restore.**

## 10. Out of scope (v1)

- Switching sessions by key outside the tree (next/previous session).
- Moving a single workspace between monitors (whole sessions move with
  Super + Shift + Alt + arrows).
- Command prompt commands other than `new`.
- Splitting a tile to make a new empty one (placeholders only appear when an app moves
  away).
- Renaming individual tiles.
- Changes to Super + Shift + K beyond the placeholder filter.
- Moving the bar to the bottom (the user will do this separately).
- Layout glyph in the widget; left/right bar positions.

## 11. Known side effects

- If the user changes the default layout (`omarchy-hyprland-workspace-layout-default`),
  that script deletes the layout file of every workspace it does not see as busy,
  including ymux workspaces that are currently empty in Hyprland; those fall back to
  the default layout (section 3.2 item 11).

- The rosakodu.dock plugin's "visible on workspace" setting only offers workspaces
  1-10 (its `BarWidget.qml` option builder), so it cannot name ymux workspaces.
  Its default ("all") is unaffected.
