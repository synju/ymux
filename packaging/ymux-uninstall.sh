#!/usr/bin/env bash
# Uninstall ymux (spec 8.2). Keeps only ~/.ymux.conf, like uninstalling tmux
# keeps ~/.tmux.conf. Run it with `ymux uninstall`.
set -u

HYPR_LUA="$HOME/.config/hypr/hyprland.lua"
LAYOUTS="$HOME/.local/state/omarchy/workspace-layouts"

echo "1. Stopping the ymux daemon"
pkill -x ymux 2>/dev/null
for _ in 1 2 3 4 5 6 7 8 9 10; do pgrep -x ymux >/dev/null || break; sleep 0.3; done
echo "   closing ymux placeholder windows"
hyprctl -j clients 2>/dev/null | jq -r '.[] | select(.class == "ymux-placeholder") | .pid' | xargs -r kill 2>/dev/null

echo "2. Removing ymux from your Hyprland config"
if [ -f "$HYPR_LUA" ]; then
    sed -i '/^require("hypr\.ymux")$/d' "$HYPR_LUA"
fi
rm -f "$HOME/.config/hypr/ymux.lua"

echo "3. Deleting ymux layout-mode files (ids 100001 and up)"
if [ -d "$LAYOUTS" ]; then
    for f in "$LAYOUTS"/*.lua; do
        [ -e "$f" ] || continue
        id=$(basename "$f" .lua)
        case "$id" in
            ''|*[!0-9]*) continue ;;
        esac
        if [ "$id" -ge 100001 ]; then rm -f "$f"; fi
    done
fi

echo "4. Putting Omarchy's workspace widget back on the bar"
PLACE_FILE="$HOME/.local/state/ymux/bar-place"
SECTION=left; INDEX=1
if [ -f "$PLACE_FILE" ]; then read -r SECTION INDEX < "$PLACE_FILE"; fi
SHELL_JSON="$HOME/.config/omarchy/shell.json"
ok=1
omarchy plugin disable synju.ymux >/dev/null 2>&1 || ok=0
rm -rf "$HOME/.config/omarchy/plugins/synju.ymux"
omarchy-shell shell rescanPlugins >/dev/null 2>&1
omarchy plugin enable omarchy.workspaces --section "$SECTION" --index "$INDEX" >/dev/null 2>&1 || ok=0
if [ "$ok" = 0 ] && [ -f "$SHELL_JSON" ]; then
    echo "   omarchy plugin commands failed; editing shell.json directly"
    tmp="$SHELL_JSON.ymux-tmp"
    jq --arg s "$SECTION" --argjson i "$INDEX" '
      .bar.layout |= (with_entries(.value |= map(select(.id != "synju.ymux")))) |
      if ([.bar.layout[][] | .id] | index("omarchy.workspaces")) == null
      then .bar.layout[$s] = ((.bar.layout[$s] // [])[:$i] + [{"id":"omarchy.workspaces"}] + (.bar.layout[$s] // [])[$i:])
      else . end' "$SHELL_JSON" > "$tmp" && mv "$tmp" "$SHELL_JSON"
fi

echo "5. Deleting all ymux state (sessions, workspaces)"
rm -rf "$HOME/.local/state/ymux"

echo "6. Removing the ymux binary"
rm -f "$HOME/.local/bin/ymux"
if [ -e /usr/bin/ymux ]; then
    echo "   the package's /usr/bin/ymux stays: remove it with: sudo pacman -R ymux"
fi

echo "7. Reloading Hyprland (Omarchy's own workspace keys and numbering come back)"
hyprctl reload >/dev/null 2>&1

echo "Kept: ~/.ymux.conf (delete it yourself to remove your ymux config too)."
rm -f -- "$0"
