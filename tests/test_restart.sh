#!/bin/sh
# The restart ladder controls destructive host operations; exercise it against command fakes.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
fixture=$(mktemp -d "${TMPDIR:-/tmp}/vtabs-restart-test.XXXXXX")
mock_bin=$fixture/bin
actions=$fixture/actions
gui_pid=

cleanup() {
  if [ -n "$gui_pid" ] && kill -0 "$gui_pid" 2>/dev/null; then
    kill "$gui_pid" 2>/dev/null || true
    wait "$gui_pid" 2>/dev/null || true
  fi
  rm -rf "$fixture"
}
trap cleanup EXIT INT TERM

mkdir -p "$mock_bin" "$fixture/state"
: >"$actions"

cat >"$mock_bin/mock" <<'EOF'
#!/bin/sh
set -eu
command_name=${0##*/}
printf '%s %s\n' "$command_name" "$*" >>"$VTABS_TEST_ACTIONS"

case "$command_name" in
  uname) printf '%s\n' Darwin ;;
  hostname) printf '%s\n' testhost ;;
  pgrep) exit 1 ;;
  ps) printf '%s\n' /Applications/WezTerm.app/Contents/MacOS/wezterm-gui ;;
  open) ;;
  launchctl)
    if [ "${1:-}" = print ]; then
      printf '%s\n' '    pid = 999999'
    fi
    ;;
  wezterm)
    case " $* " in
      *' list-clients '*)
        printf '%s\n' 'USER HOST PID CONNECTED IDLE WORKSPACE FOCUS SSH_AUTH_SOCK'
        if [ -n "${VTABS_TEST_CLIENT_PID:-}" ]; then
          printf '%s %s %s 1s 0s default 9 -\n' \
            "$(id -un)" "${VTABS_TEST_CLIENT_HOST:-testhost}" "$VTABS_TEST_CLIENT_PID"
        fi
        ;;
      *' kill-pane '*) ;;
      *' list --format json '*) printf '%s\n' '[]' ;;
      *' list '*)
        [ "${VTABS_TEST_LIST_FAIL:-0}" = 0 ] || exit 1
        printf '%s\n' 'WINID TABID PANEID WORKSPACE SIZE TITLE CWD'
        printf '%s\n' '    1     4      9 default   28x60 wez-vtabs:test file:///tmp/'
        ;;
      *) exit 2 ;;
    esac
    ;;
  *) exit 2 ;;
esac
EOF
chmod +x "$mock_bin/mock"
for command_name in hostname launchctl open pgrep ps uname wezterm; do
  ln -s mock "$mock_bin/$command_name"
done

run_restart() {
  PATH="$mock_bin:$PATH" \
    XDG_STATE_HOME="$fixture/state" \
    VTABS_RESTART_MUX_SOCKET="$fixture/localmux.sock" \
    VTABS_TEST_ACTIONS="$actions" \
    VTABS_TEST_CLIENT_HOST="${VTABS_TEST_CLIENT_HOST:-}" \
    VTABS_TEST_CLIENT_PID="${VTABS_TEST_CLIENT_PID:-}" \
    VTABS_TEST_LIST_FAIL="${VTABS_TEST_LIST_FAIL:-0}" \
    WEZTERM_PANE='' \
    sh "$root/scripts/restart.sh" "$@"
}

reset_actions() {
  : >"$actions"
}

assert_logged() {
  grep -F "$1" "$actions" >/dev/null || {
    printf 'expected action not found: %s\n' "$1" >&2
    exit 1
  }
}

assert_not_logged() {
  if grep -F "$1" "$actions" >/dev/null; then
    printf 'unexpected action found: %s\n' "$1" >&2
    exit 1
  fi
}

# The default may replace its attached GUI client, but it never touches a pane or the mux service.
sleep 30 &
gui_pid=$!
VTABS_TEST_CLIENT_PID=$gui_pid run_restart >/dev/null
wait "$gui_pid" 2>/dev/null || true
gui_pid=
assert_logged 'open -na WezTerm'
assert_not_logged 'kill-pane'
assert_not_logged 'launchctl kill'

# A client reported from another host is not ours to terminate.
reset_actions
sleep 30 &
gui_pid=$!
VTABS_TEST_CLIENT_PID=$gui_pid VTABS_TEST_CLIENT_HOST=elsewhere run_restart >/dev/null
kill -0 "$gui_pid"
kill "$gui_pid"
wait "$gui_pid" 2>/dev/null || true
gui_pid=
assert_logged 'open -na WezTerm'
assert_not_logged 'launchctl kill'

# Pane recovery defaults to no, then kills exactly the pane that was confirmed.
reset_actions
run_restart --pane 9 </dev/null >/dev/null
assert_not_logged 'kill-pane'
printf 'y\n' | run_restart --pane 9 >/dev/null
[ "$(grep -c 'kill-pane --pane-id 9' "$actions")" -eq 1 ]
assert_not_logged 'launchctl kill'

# Full mux replacement also defaults to no and remains usable when the mux cannot report a count.
reset_actions
run_restart --mux </dev/null >/dev/null
assert_not_logged 'launchctl kill'
VTABS_TEST_LIST_FAIL=1 run_restart --mux --dry-run |
  grep -F 'destroy every mux pane (current count unavailable)' >/dev/null
printf 'y\n' | run_restart --mux >/dev/null
assert_logged 'launchctl kill SIGTERM'
assert_not_logged 'kill-pane'

printf '%s\n' 'restart recovery tests passed'
