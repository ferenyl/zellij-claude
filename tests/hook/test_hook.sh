#!/bin/sh
# Tests scripts/claude-zellij-hook.sh in a hermetic PATH with a stub `zellij`
# and a temporary status directory. Requires jq. Exits non-zero on failure.
set -u

root=$(cd "$(dirname "$0")/../.." && pwd)
hook="$root/scripts/claude-zellij-hook.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failures=0

# Only the tools the hook needs, so a real zellij is never called.
mkdir -p "$work/tools" "$work/stub" "$work/home"
for tool in sh cat jq mkdir mktemp mv rm date timeout sleep; do
    path=$(command -v "$tool") || { echo "missing tool: $tool" >&2; exit 1; }
    ln -s "$path" "$work/tools/$tool"
done
cat >"$work/stub/zellij" <<'STUB'
#!/bin/sh
sleep "${STUB_SLEEP:-0}"
printf '%s\n' "$*" >>"$STUB_LOG"
STUB
chmod +x "$work/stub/zellij"

status="$work/status"
log="$work/pipe.log"

# run_hook <payload> [VAR=value ...]: later assignments override the defaults.
run_hook() {
    payload=$1
    shift
    printf '%s' "$payload" | env -i \
        PATH="$work/stub:$work/tools" \
        HOME="$work/home" \
        ZELLIJ_SESSION_NAME=work \
        ZELLIJ_PANE_ID=7 \
        CLAUDE_ZELLIJ_STATUS_DIR="$status" \
        STUB_LOG="$log" \
        "$@" \
        "$work/tools/sh" "$hook"
}

reset() {
    rm -rf "$status" "$log" "$work/home/.cache"
}

# wait_for_log <lines>: the pipe call runs in the background.
wait_for_log() {
    i=0
    while [ "$i" -lt 30 ]; do
        [ -f "$log" ] && [ "$(wc -l <"$log")" -ge "$1" ] && return 0
        sleep 0.1
        i=$((i + 1))
    done
    return 1
}

check() {
    if eval "$2"; then
        echo "ok   - $1"
    else
        echo "FAIL - $1"
        failures=$((failures + 1))
    fi
}

file="$status/work-7.json"

reset
run_hook '{"hook_event_name":"Notification","session_id":"abc","cwd":"/code \"q\""}'
check "exits 0" '[ $? -eq 0 ]'
check "writes status file" '[ -f "$file" ]'
check "status file fields" 'jq -e ".zellij_session == \"work\" and .pane_id == 7 and .hook_event == \"Notification\" and .cwd == \"/code \\\"q\\\"\" and .claude_session_id == \"abc\" and (.updated_at | type) == \"number\"" "$file" >/dev/null'
check "no temp files left" '[ "$(ls -A "$status" | wc -l)" -eq 1 ]'
check "sends pipe message" 'wait_for_log 1 && grep -q "^pipe --name claude-status -- {.*\"hook_event\":\"Notification\"" "$log"'

run_hook '{"hook_event_name":"SessionEnd","session_id":"abc"}'
check "SessionEnd removes status file" '[ ! -e "$file" ]'
check "SessionEnd is piped" 'wait_for_log 2 && tail -n 1 "$log" | grep -q "\"hook_event\":\"SessionEnd\""'

reset
run_hook '{"hook_event_name":"Stop"}' ZELLIJ_SESSION_NAME=
check "outside Zellij does nothing" '[ $? -eq 0 ] && [ ! -e "$status" ]'

reset
run_hook '{"hook_event_name":"Stop"}' ZELLIJ_PANE_ID=abc
check "non-numeric pane id does nothing" '[ $? -eq 0 ] && [ ! -e "$status" ]'

reset
run_hook 'not json'
check "invalid JSON does nothing" '[ $? -eq 0 ] && [ ! -e "$file" ]'

reset
run_hook '{"hook_event_name":"Stop"}' PATH="$work/stub"
check "missing jq does nothing" '[ $? -eq 0 ] && [ ! -e "$status" ]'

reset
run_hook '{"hook_event_name":"Stop"}' PATH="$work/tools"
check "missing zellij still writes status file" '[ $? -eq 0 ] && [ -f "$file" ]'

reset
run_hook '{"hook_event_name":"Stop"}' CLAUDE_ZELLIJ_STATUS_DIR= XDG_CACHE_HOME="$work/home/.cache"
check "defaults to XDG_CACHE_HOME/claude-zellij" '[ -f "$work/home/.cache/claude-zellij/work-7.json" ]'

reset
start=$(date +%s)
run_hook '{"hook_event_name":"Stop"}' STUB_SLEEP=10
elapsed=$(($(date +%s) - start))
check "does not wait for a hanging zellij pipe" '[ "$elapsed" -le 2 ]'

if [ "$failures" -gt 0 ]; then
    echo "$failures test(s) failed"
    exit 1
fi
echo "all hook tests passed"
