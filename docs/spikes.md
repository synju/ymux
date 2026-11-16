# ymux spikes S1-S4 (2026-11-15)

Run on the live desktop (Hyprland 0.56.2, Omarchy 4.5.0; eDP-2 laptop panel
`BOE 0x0A3B` at x=1920, DP-1 `HKC OVERSEAS LIMITED GN02 0000000000001` at x=0).
All changes reverted afterwards: `~/.config/hypr/hyprland.lua` identical to its
backup, no spike binds left, layout files `1.lua` / `2.lua` still `scrolling`.

## S3: daemon start from top-level Lua

- `hl.exec_cmd(...)` at the top level of a required Lua file runs on every config
  evaluation. The spawned process inherits `HYPRLAND_INSTANCE_SIGNATURE`, equal to the
  directory under `$XDG_RUNTIME_DIR/hypr/`.
- **At boot** the top-level exec runs while Hyprland is still reading its config,
  before `.socket2.sock` and the request socket exist (found after the first real
  reboot, 2026-11-15: the daemon exited on the socket2 connect). The daemon now waits
  up to 30 s for both (and a monitor) before starting, and logs fatal errors to
  `~/.local/state/ymux/daemon.log`, since Hyprland discards the child's output.
- The boot-time daemon also inherits `WAYLAND_DISPLAY` empty (found after the second
  reboot test: every relaunched app timed out; `/proc/<daemon>/environ` had
  `WAYLAND_DISPLAY=`). `hyprctl -j instances` reports the instance's `wl_socket`
  (`wayland-1`); the daemon now sets `WAYLAND_DISPLAY` from it before starting anything,
  so apps and placeholders it launches reach the display.
- Hyprland **reloads by itself when a config file changes** (editing the spike file
  produced a `configreloaded` event). Writing `ymux.lua` therefore triggers a reload
  without `hyprctl reload`.

## S1: reconcile off, socket2 formats, unplug and replug

- `o.workspaces.reconcile = function() end` in a user Lua file stops Omarchy's
  renumbering: workspace 100001 survived `hyprctl reload`, and 100002 kept its id
  through an unplug and replug of DP-1.
- socket2 lines (addresses have no `0x`; ymux adds it):
  - `openwindow>>560362726330,12,ymux-spike,foot` (address, workspace name, class,
    title)
  - `closewindow>>560362726330`
  - `movewindowv2>>560362726330,100001,100001` (address, workspace id, workspace name)
  - `createworkspacev2>>100001,100001` / `destroyworkspacev2>>100002,100002` (id, name)
  - `moveworkspacev2>>12,12,eDP-2` (id, name, monitor)
  - `workspacev2>>100002,100002` (id, name)
  - `focusedmonv2>>DP-1,12` (monitor, workspace id)
  - `monitorremovedv2>>1,DP-1,HKC OVERSEAS LIMITED GN02 0000000000001` (id, name,
    description)
  - `monitoraddedv2>>1,DP-1,HKC OVERSEAS LIMITED GN02 0000000000001`
  - `configreloaded>>`
  - `changeworkspaceid>>100001,100002` (old id, new id). **This event exists**, contrary
    to spec 3.2.9, which assumed it did not.
  - `submap>>cmxspike` on entering a submap, `submap>>` on leaving.
- **Event order on unplug:** Hyprland first emits `moveworkspacev2` for every workspace
  of the leaving monitor (to the remaining monitor), and only then
  `monitorremovedv2`. **On replug:** `moveworkspacev2` back to the returning monitor
  first, then `monitoraddedv2`. Hyprland itself returns the workspaces on replug.

## S2: submap, catch-all, unbind, timers

- Binds inside `hl.define_submap(name, function() ... end)` work with `hl.bind(keys,
  fn, { description = ... })`.
- Bare `C` inside the submap fires.
- `hl.bind("catchall", fn, opts)` inside a submap is accepted. **Correction (phase 3
  acceptance, verified with real key events):** a press catch-all fires on EVERY key
  press, alongside any specific bind, including modifier presses such as Shift; no
  no-op bind stops it, and the callback gets no arguments. With `release = true` it
  fires only for keys whose press and release happen in the submap with no modifier
  change, so modifier presses are ignored and `Shift + code:13` (`$`) works.
- `SUPER + C` inside the submap fired once no-op binds for `Super_L` / `Super_R` (with
  and without `SUPER + `) were added; before that, the catch-all took the key, most
  likely on the Super key press itself. (User decision since: leader action keys are
  plain only, see below.)
- Two binds on one key spec are both removed by **one** `hl.unbind(keys)`; it returns
  `nil` whether or not something was removed.
- A `oneshot` timer fires once; after it has fired, `set_enabled(true)` does **not**
  re-arm it. A fresh timer is needed per arming.

## S4: cross-workspace swap, floating event

- `hl.dsp.window.swap({ target = "address:<a>", window = "address:<b>" })` works
  (the `target` form). Verified swapping: two dwindle workspaces; a window on
  `special:ymux-stash` with a dwindle tile; a `scrolling` workspace (column width
  922 taken over); two monitors. Each window takes the other's workspace, position and
  size (on another monitor, it takes the tile at that monitor's size).
- Super + L to floating emits `changefloatingmode>>560362726330,1` per window
  (and `,0` when it leaves floating); the floated window is tagged
  `omarchy-floating-workspace`, resized to 1100x716 and centered.
- `hyprctl eval 'o.workspace_mode({ workspace = "100003", mode = "scrolling" })'` sets a
  mode without writing a layout file.

## Decisions made from these results

- User: after the leader, action keys are **plain only** (no `SUPER + ` variant).
  Holding Super at the action key reaches the catch-all, which ends leader mode. Resize
  stays SUPER + arrow.
- Ruling: leader timeout uses a fresh `oneshot` timer per leader press (the previous
  one disabled), since a fired oneshot cannot be re-armed.
- Ruling: one `hl.unbind` per key spec is enough (`unbind_removes_all = true`).
- Ruling: the daemon uses `changeworkspaceid` for outside renumbering, keeping the 30 s
  check as a fallback.
- Ruling: the monitor invariant runs after events settle (300 ms with no further
  monitor or workspace-move event), reading live monitors, because Hyprland moves
  workspaces before reporting the monitor removal or addition.
- Ruling: ymux.lua generation still calls `hyprctl reload` only when needed; since
  Hyprland auto-reloads on file change, writing the file is the reload.

## Verdicts

S1: pass - reconcile off holds through reload, unplug and replug; formats recorded.
S2: pass - submap, catch-all, unbind and timer behaviour recorded.
S3: pass - top-level exec runs on load and reload, signature inherited.
S4: pass - swap works across workspaces, stash, scrolling and monitors; float event recorded.

## Phase 3 spike: active window (2026-11-15)

- Line format: `activewindowv2>>56036213b7e0` (address without `0x`). The empty form
  `activewindowv2>>` (nothing focused) was not observed in this run; the parser
  treats empty data as no focus.
- Order when a window opens: `openwindow>>56036213b7e0,100001,cmx-aw,foot` first, then
  `activewindowv2>>56036213b7e0` (sent twice). Closing: focus moves first
  (`activewindowv2>>560362ab14e0`), then `closewindow>>56036213b7e0`.
- Also confirmed: `hl.dsp.focus({ window = "address:0x..." })` and
  `hl.dsp.window.close({ window = "address:0x..." })` both work via `hyprctl dispatch`.

## Phase 3 spike: plugin load (2026-11-15)

- Copying `plugin/synju.ymux/` to `~/.config/omarchy/plugins/` and running
  `omarchy-shell shell rescanPlugins` makes the plugin known only after a few seconds
  (discovery is asynchronous); `omarchy plugin enable` run immediately after reports
  "not known". Wait (poll `omarchy plugin list`) before enabling.
- `omarchy plugin enable synju.ymux --section right` placed it and the widget
  rendered live: `2: main 1` with the current workspace highlighted.
- Shell QML warnings go to the user journal: `journalctl --user` (unit output of
  `omarchy-shell`). No ymux QML errors; one scanner warning that the module path is
  outside the config folder, which every third-party plugin gets.
- Reverted: plugin disabled and removed, `bar.layout` identical to before.

## Phase 5 spike: placeholder (2026-11-15)

- The window rule `o.window("^ymux-placeholder$", { workspace = "special:ymux-stash silent", border_size = 0, no_border = true, no_shadow = true, no_blur = true, decorate = false, no_dim = true, rounding = 0, no_anim = true })`
  loads with no `hyprctl configerrors`; `ymux placeholder` opens on
  `special:ymux-stash`, tiled.
- `hl.dsp.window.swap({ target = "address:<p>", window = "address:<f>" })` across a
  regular workspace and the stash works and emits `movewindowv2` for BOTH windows.
- `hl.dsp.window.close({ window = "address:<p>" })` makes the placeholder exit 0.
- Absolute geometry on a floating window works:
  `hl.dsp.window.move({ x = 100, y = 100, relative = false, window = "address:<a>" })`
  and `hl.dsp.window.resize({ x = 800, y = 600, relative = false, window = "address:<a>" })`;
  `hl.dsp.window.float({ action = "enable", window = "address:<a>" })` floats it.
- Moving a window to an unknown id (100090) was adopted live and renumbered into the
  focused monitor's range, as designed.
- Not verified here (needs eyes on screen): that the placeholder is fully invisible
  with no border or shadow when focused. Covered by phase 5 acceptance.

## Phase 5 spike: mouse drag of a tiled window (2026-11-15)

Logged socket2 plus `hyprctl -j clients` / `cursorpos` while the user dragged with
Super + left mouse (focus follows mouse on this machine).

- A drag starts with `changefloatingmode>>addr,1` (the window floats while dragged;
  the source workspace re-tiles at once) and ends with `changefloatingmode>>addr,0` at
  the drop. No other event marks the drop.
- Across monitors, `movewindowv2>>addr,<target id>,<target name>` arrives mid-drag, as
  the window crosses onto the other monitor's workspace.
- Within one workspace there is no move event, only the float pair.
- At the drop, Hyprland splits the tile under the cursor: dropped onto a placeholder,
  the app and the placeholder share its spot (app on top, placeholder below).
- `hyprctl -j cursorpos` gives `{"x":..,"y":..}` in the same global coordinates as
  client `at`.

## Phase 5 spike: release hook on Super + left drag (2026-11-15)

- A second bind `hl.bind("SUPER + mouse:272", fn, { release = true })` added next to
  Omarchy's `o.bind("SUPER + mouse:272", "Move window", hl.dsp.window.drag(), { mouse = true })`
  is accepted, and the drag still works (the window follows the mouse).
- The release callback runs about 7 ms AFTER `changefloatingmode>>addr,0`. By then the
  drop is complete: the window is tiled again and has already split the tile under the
  cursor. So a release hook cannot replace the drop; it only sees its result.
- Reverted with `hyprctl reload` (only Omarchy's "Move window" bind remains).

## Phase 5 spike: dwindle reshape messages (2026-11-15, read-only)

- The installed Hyprland 0.56.2 contains the dwindle layout messages `togglesplit`,
  `swapsplit` and `movetoroot` (binary strings), sent through the `hl.dsp.layout`
  dispatcher (`/usr/share/hypr/stubs/hl.meta.lua:880`; Omarchy binds
  `hl.dsp.layout("togglesplit")`, `tiling.lua:5`).
- Live: `hyprctl dispatch 'hl.dsp.layout("swapsplit")'` swaps the FOCUSED window's split:
  with a top terminal and a bottom placeholder, focusing the top one and sending it put
  the terminal at the bottom and the placeholder at the top; sending it again restored
  them. So focusing a window that is a direct child of the root split and sending
  `swapsplit` mirrors the whole layout across that split. Reverted.

## Phase 5 spike: parking a placeholder (2026-11-15)

- A spare `ymux placeholder` on the stash, after
  `hl.dsp.window.float({ action = "enable", window = ... })`,
  `hl.dsp.window.resize({ x = 1, y = 1, relative = false, window = ... })` and
  `hl.dsp.window.move({ x = 1920, y = 480, relative = false, window = ... })`, read back
  as floating, at (1920,480), size 1 x 1: Hyprland does not enforce a minimum size here.
  Closed afterwards.

## Phase 5 spike: rebuilding a dwindle tree (2026-11-15, laptop workspace 100003)

Three test terminals (`ghostty --gtk-single-instance=false`, opened with
`hl.exec_cmd(cmd, { workspace = "<id> silent" })`; plain ghostty ignores the rule because
the window comes from its running process). Omarchy sets `dwindle:force_split = 2`.
- Moving every window to `special:ymux-stash` (`hl.dsp.window.move({ workspace =
  "special:ymux-stash", follow = false, window = ... })`) empties the workspace; ymux
  keeps it because it is shown.
- The first window moved back fills the workspace. With window A focused, the next window
  moved in splits A, landing right of A when A is wider than tall and below A otherwise
  (force_split 2: always right / bottom; orientation by the split box's aspect).
- `hl.dsp.layout("togglesplit")` with the new window focused flips that split (stacked to
  side by side and back).
- `hl.dsp.window.resize({ x, y, relative = false })` on a tiled window sets its size; the
  sibling takes the rest.

## Phase 6 acceptance (2026-11-15)

- Reboot 1: nothing came back. The shutdown's closewindow burst had emptied
  `state.json` (fixed: `relaunch.json` snapshot, written after 5 quiet seconds).
- Reboot 2: all three apps timed out ("did not come back"). The boot-time daemon had
  `WAYLAND_DISPLAY` empty (fixed: set from `hyprctl -j instances` `wl_socket`).
- Reboot 3: pass. Ghostty, Claude Desktop and Vivaldi came back on the external
  monitor's workspace (user confirmed).
- Not yet run: relaunch off across a reboot, a Flatpak with a `launch-override`.

## Fullscreen and swap (2026-11-16, laptop test terminal)

- `hl.dsp.window.swap` on a fullscreen window fails with `Can't swap fullscreen window`
  (seen live: Vivaldi fullscreen on YouTube).
- `hyprctl -j clients` reports `fullscreen` 0 (none), 1 (maximized), 2 (fullscreen).
- `hl.dsp.window.fullscreen({ mode = "fullscreen" | "maximized", action = "unset" | "set" | "toggle", window = "address:..." })`
  works by address: `unset` took a fullscreen window to 0, `set` back to 2; the
  `maximized` mode gives 1.
