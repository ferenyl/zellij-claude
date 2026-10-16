#!/bin/sh
# Claude Code hook: reports session status to the zellij-claude sessions plugin.
#
# Reads the hook JSON from stdin, writes a status file for this Zellij pane and
# sends it to the plugin over a `zellij pipe`. Always exits 0 and never blocks,
# so Claude keeps working when Zellij, jq or the plugin is missing.
#
# Status directory: $CLAUDE_ZELLIJ_STATUS_DIR, else $XDG_CACHE_HOME/claude-zellij,
# else ~/.cache/claude-zellij (must match the plugin's `status_dir`).

main() {
    [ -n "${ZELLIJ_SESSION_NAME:-}" ] || return 0
    case "${ZELLIJ_PANE_ID:-}" in
        '' | *[!0-9]*) return 0 ;;
    esac
    command -v jq >/dev/null 2>&1 || return 0

    input=$(cat) || return 0
    event=$(printf '%s' "$input" | jq -r '.hook_event_name // empty' 2>/dev/null) || return 0
    [ -n "$event" ] || return 0

    dir=${CLAUDE_ZELLIJ_STATUS_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/claude-zellij}
    mkdir -p "$dir" || return 0
    file="$dir/$ZELLIJ_SESSION_NAME-$ZELLIJ_PANE_ID.json"

    record=$(printf '%s' "$input" | jq -c \
        --arg session "$ZELLIJ_SESSION_NAME" \
        --argjson pane "$ZELLIJ_PANE_ID" \
        --argjson now "$(date +%s)" \
        '{
            zellij_session: $session,
            pane_id: $pane,
            hook_event: .hook_event_name,
            cwd: .cwd,
            claude_session_id: .session_id,
            updated_at: $now
        }' 2>/dev/null) || return 0

    # The file is written before the pipe message: the plugin drops sessions
    # without a status file when it re-reads the directory.
    if [ "$event" = "SessionEnd" ]; then
        rm -f "$file"
    else
        tmp=$(mktemp "$dir/.tmp.XXXXXX") || return 0
        if printf '%s\n' "$record" >"$tmp"; then
            mv -f "$tmp" "$file"
        else
            rm -f "$tmp"
        fi
    fi

    command -v zellij >/dev/null 2>&1 || return 0
    # Backgrounded and detached from Claude's stdio: `zellij pipe` waits for every
    # plugin listening on the pipe and can hang, so it is also time-limited.
    if command -v timeout >/dev/null 2>&1; then
        timeout 5 zellij pipe --name claude-status -- "$record" </dev/null >/dev/null 2>&1 &
    else
        zellij pipe --name claude-status -- "$record" </dev/null >/dev/null 2>&1 &
    fi
}

main
exit 0
