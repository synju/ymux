#!/usr/bin/env bash
# Install ymux (omarchymux) for the current user:
#   curl -fsSL https://raw.githubusercontent.com/synju/ymux/main/install.sh | bash
# It checks the desktop, downloads the ymux binary from the latest GitHub release (or
# builds it from source when there is no binary for this machine), then runs
# `ymux install`. Remove ymux again with `ymux uninstall`.
set -euo pipefail

REPO="synju/ymux"
VERSION="${YMUX_VERSION:-latest}"

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

say "Checking your desktop"
command -v hyprctl >/dev/null || die "Hyprland (hyprctl) not found: ymux runs on Omarchy with Hyprland."
command -v omarchy >/dev/null || command -v omarchy-shell >/dev/null || die "Omarchy not found: ymux needs Omarchy's shell and plugins."
[ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ] || die "Run this from inside your Hyprland session (a terminal on the desktop)."
for tool in jq notify-send curl; do
    command -v "$tool" >/dev/null || die "$tool not found: install it first (sudo pacman -S ${tool/notify-send/libnotify})."
done
hypr_version=$(hyprctl version -j | jq -r '.version // empty')
if [ -n "$hypr_version" ]; then
    IFS=. read -r major minor _ <<<"$hypr_version"
    if [ "$major" -eq 0 ] && [ "$minor" -lt 55 ]; then
        die "Hyprland $hypr_version is too old: ymux needs 0.55 or newer (Lua config)."
    fi
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
arch=$(uname -m)
if [ "$VERSION" = latest ]; then
    url="https://github.com/$REPO/releases/latest/download/ymux-$arch"
else
    url="https://github.com/$REPO/releases/download/$VERSION/ymux-$arch"
fi

say "Downloading ymux for $arch"
if curl -fsSL -o "$work/ymux" "$url"; then
    chmod +x "$work/ymux"
elif command -v cargo >/dev/null && command -v git >/dev/null; then
    say "No ready-made binary for $arch; building from source (this takes a minute)"
    ref=()
    [ "$VERSION" = latest ] || ref=(--branch "$VERSION")
    git clone --quiet --depth 1 "${ref[@]}" "https://github.com/$REPO.git" "$work/src"
    cargo build --quiet --release --manifest-path "$work/src/Cargo.toml"
    cp "$work/src/target/release/ymux" "$work/ymux"
else
    die "No ready-made binary for $arch, and building from source needs cargo and git."
fi

say "Setting up ymux"
"$work/ymux" install

say "Done. Press Super + B, then ? for the list of keys."
say "To remove ymux later: ymux uninstall"
