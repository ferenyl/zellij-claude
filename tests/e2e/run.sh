#!/bin/sh
# Best-effort end-to-end check: runs Zellij with the built plugin in its default
# layout, attached through a pseudo-terminal (`script`), with an isolated HOME,
# cache and socket directory. The check is a side effect: the plugin must delete
# the status file of a long-gone session (needs permissions, the status dir
# remap, polling, SessionUpdate and pruning to work) and keep the live one.
#
# Usage: tests/e2e/run.sh [path/to/claude-sessions.wasm]
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
wasm=$(cd "$(dirname "${1:-$root/target/wasm32-wasip1/release/claude-sessions.wasm}")" && pwd)/claude-sessions.wasm
[ -f "$wasm" ] || { echo "wasm not found: $wasm" >&2; exit 1; }

work=$(mktemp -d)
# Socket paths are limited to ~107 bytes, so the socket dir must be short even when $TMPDIR is not.
sock=$(mktemp -d /tmp/czs.XXXXXX)
session="claude-e2e-$$"
export HOME="$work/home" XDG_CACHE_HOME="$work/cache" ZELLIJ_SOCKET_DIR="$sock"
unset ZELLIJ ZELLIJ_SESSION_NAME ZELLIJ_PANE_ID XDG_CONFIG_HOME
mkdir -p "$HOME" "$XDG_CACHE_HOME/zellij" "$XDG_CACHE_HOME/claude-zellij"

# shellcheck disable=SC2317,SC2329 # invoked through the EXIT trap
cleanup() {
    zellij kill-session "$session" >/dev/null 2>&1 || true
    [ -n "${client:-}" ] && kill "$client" 2>/dev/null
    rm -rf "$work" "$sock"
}
trap cleanup EXIT

# Pre-grant permissions so no prompt blocks the plugin.
cat >"$XDG_CACHE_HOME/zellij/permissions.kdl" <<KDL
"$wasm" {
    ReadApplicationState
    ChangeApplicationState
    ReadCliPipes
    FullHdAccess
    ReadSessionEnvironmentVariables
}
KDL

status="$XDG_CACHE_HOME/claude-zellij"
printf '{"zellij_session":"%s","pane_id":0,"hook_event":"Stop","updated_at":%s}\n' \
    "$session" "$(date +%s)" >"$status/$session-0.json"
printf '{"zellij_session":"gone","pane_id":1,"hook_event":"Stop","updated_at":1}\n' \
    >"$status/gone-1.json"

# Zellij answers permission requests through a client, so a detached session is
# not enough: a real client is attached through a pseudo-terminal below.
mkdir -p "$HOME/.config/zellij/layouts"
cat >"$HOME/.config/zellij/layouts/default.kdl" <<KDL
layout {
    pane
    pane size=1 borderless=true {
        plugin location="file:$wasm" {
            mode "bar"
        }
    }
}
KDL

TERM=xterm-256color script -qfec "zellij --session $session" /dev/null >/dev/null 2>&1 &
client=$!

i=0
while [ "$i" -lt 30 ]; do
    sleep 0.5
    if [ ! -e "$status/gone-1.json" ]; then
        if [ -e "$status/$session-0.json" ]; then
            echo "e2e: plugin pruned the stale status file and kept the live one"
            exit 0
        fi
        echo "e2e: plugin also deleted the live session's status file" >&2
        exit 1
    fi
    i=$((i + 1))
done

echo "e2e: stale status file was not pruned within 15s" >&2
exit 1
