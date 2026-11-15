# ymux

**omarchymux**: tmux-style sessions, workspaces and tiles for the
[Omarchy](https://omarchy.org) desktop on Hyprland.

Each monitor holds sessions, each session holds workspaces, and each workspace holds
tiles, much like tmux's sessions, windows and panes. A tmux-style status line on the bar
shows the current session and its workspaces. A leader key (Super + B) opens a tree of
everything, a command prompt, renames, an options panel and more. Empty tiles stay
open as invisible placeholders, so a layout keeps its shape. After a reboot, ymux
starts your apps again and puts them back in their layout.

## Requirements

- Omarchy 4.5 or newer (its Lua helpers, workspace modes and shell plugin system)
- Hyprland 0.55 or newer (Lua configuration)
- `jq` and `libnotify` (`notify-send`)

## Install

From the AUR:

```bash
yay -S ymux
```

Then, once for each user:

```bash
ymux install
```

`ymux install` writes `~/.ymux.conf` (unless it exists), generates
`~/.config/hypr/ymux.lua`, adds `require("hypr.ymux")` to
`~/.config/hypr/hyprland.lua`, installs the bar plugin `synju.ymux` in place of
Omarchy's workspace widget, and starts the daemon. On the first start it gathers each
monitor's open workspaces into a session called `main`.

From source:

```bash
cargo build --release
./target/release/ymux install
```

A build installed this way is copied to `~/.local/bin/ymux`.

## Uninstall

```bash
ymux uninstall
```

Run it before removing the package (`sudo pacman -R ymux`). It stops the daemon,
removes the Lua file, the `require` line and the plugin, puts Omarchy's workspace
widget back, and deletes ymux's state. Only `~/.ymux.conf` is kept.

## Keys

| Keys | Action |
|---|---|
| Super + B | Leader (then one of the keys below) |
| Leader, `w` | Tree of monitors, sessions, workspaces and windows: Enter switches, `r` renames, `x` kills, `h` / `l` fold |
| Leader, `c` | New workspace in the current session |
| Leader, `:` | Command prompt (`new <name>` creates a session) |
| Leader, `$` / `,` | Rename the session / the workspace |
| Leader, `.` | Renumber the workspace |
| Leader, `o` | Options (relaunch on or off) |
| Leader, `q` | Number every visible tile; press a number to focus it |
| Leader, `m` / `v` | Mark a tile / swap it with the marked one |
| Leader, `x` or Super + W | Close the focused window, keeping its spot open |
| Leader, then arrows with Super held | Resize the focused tile |
| Super + 1-0 | Workspace 1-10 of the current session |
| Super + Shift + 1-0 | Move the window there, leaving its spot open |
| Super + Shift + Alt + arrows | Move the session to another monitor |
| Shift + Ctrl + Alt + H / L | Previous / next workspace |
| Alt + Tab | Next window, skipping empty spots |

## Configuration

`~/.ymux.conf` uses tmux-style lines, for example:

```
set leader SUPER+B
set relaunch on
bind w tree
bind-global SHIFT+CTRL+ALT+L next-workspace
launch-override steam "steam -silent"
```

Edits apply within a couple of seconds. `launch-override` gives the command used to
start an app again after a reboot, for apps started through a wrapper.

## Relaunch

With `set relaunch on`, ymux saves each window's command, working directory, position
and size. When Hyprland starts again it reopens them one at a time, reusing any copy
that is already open, and rebuilds each workspace's layout. Apps come back fresh: no
unsaved work, scrollback or game state.

## License

MIT, see [LICENSE](LICENSE).
