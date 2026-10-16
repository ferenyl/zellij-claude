#!/bin/sh
# Builds the plugin and installs it with the hook script into the Zellij plugin directory.
# Claude Code settings are not modified; merge examples/claude-settings.json yourself.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
dest=${ZELLIJ_PLUGIN_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/zellij/plugins}

cargo build --release --locked --target wasm32-wasip1 -p claude-sessions-plugin \
    --manifest-path "$root/Cargo.toml"

mkdir -p "$dest"
cp "$root/target/wasm32-wasip1/release/claude-sessions.wasm" "$dest/claude-sessions.wasm"
cp "$root/scripts/claude-zellij-hook.sh" "$dest/claude-zellij-hook.sh"
chmod +x "$dest/claude-zellij-hook.sh"

command -v jq >/dev/null 2>&1 || echo "warning: jq not found; the hook needs it to report status" >&2

cat <<MSG
Installed to $dest

Next steps:
  1. Add the hooks from $root/examples/claude-settings.json to ~/.claude/settings.json
  2. Add the keybinding from $root/examples/config.kdl to your Zellij config
  3. Optionally add a status bar from $root/examples/layout-*.kdl
MSG
