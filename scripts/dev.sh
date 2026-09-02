#!/bin/sh
# Watch, rebuild and hot-swap the backend. Lua reloads itself via WezTerm's config watcher.
set -eu
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"

live=0
profile=
mode=watch
while [ $# -gt 0 ]; do
  case "$1" in
    --live) live=1 ;;
    --release) profile=release ;;
    --debug) profile=debug ;;
    --once) mode=once ;;
    --check) mode=check ;;
    -h | --help) say "usage: just dev [--live] [--release|--debug]"; exit 0 ;;
    *) die "unknown flag: $1" ;;
  esac
  shift
done

# Live mode reuses your config's backend.path, which points at target/release.
[ -n "$profile" ] || { [ "$live" = 1 ] && profile=release || profile=debug; }
bin=$(bin_for "$profile")

if [ "$mode" = check ]; then
  cd "$root/plugin"
  if lua tests/run.lua >/tmp/vtabs-dev-test.log 2>&1; then
    ok "lua $(tail -1 /tmp/vtabs-dev-test.log)"
  else
    printf '%slua tests failed%s\n' "$red" "$off"
    tail -15 /tmp/vtabs-dev-test.log
  fi
  command -v luacheck >/dev/null 2>&1 && { luacheck -q init.lua vtabs || true; }
  exit 0
fi

if [ "$mode" = once ]; then
  build "$profile" || exit 0
  restart_sidebars "$bin"
  exit 0
fi

build "$profile" || die "initial build failed"

pids=""
# WezTerm's panic hook writes into its own log under the sandbox HOME; keep the last 10 runs.
keep_logs() {
  dest="$devlogs/$(date '+%Y%m%d-%H%M%S')"
  mkdir -p "$dest"
  cp "$sandbox_home"/.local/share/wezterm/wezterm-gui-log-*.txt "$sandbox_home"/wezterm.log \
    "$sandbox_home"/wez-vtabs.log "$dest" 2>/dev/null
  ls -1dt "$devlogs"/*/ 2>/dev/null | tail -n +11 | xargs rm -rf 2>/dev/null
  printf '%slogs kept in %s%s\n' "$dim" "$dest" "$off"
}
cleanup() {
  set +e
  # shellcheck disable=SC2086  # word splitting is intended: pids is a list
  [ -n "$pids" ] && kill $pids 2>/dev/null
  pkill -f "class vtabs-dev" 2>/dev/null
  # The log tailer is a grandchild; kill it by the temp path it holds open.
  [ -n "${sandbox_home:-}" ] && pkill -f "$sandbox_home" 2>/dev/null
  [ -n "${sandbox_home:-}" ] && keep_logs
  [ -n "${sandbox_home:-}" ] && rm -rf "$sandbox_home"
  printf '\n%sdev stopped%s\n' "$dim" "$off"
}
trap cleanup EXIT INT TERM

if [ "$live" = 0 ]; then
  sandbox_home=$(mktemp -d /tmp/vtabs-dev.XXXXXX)
  mkdir -p "$sandbox_home/.local/share/wezterm"
  say "${dim}sandbox HOME=$sandbox_home${off}"
  log="$sandbox_home/wezterm.log"
  HOME="$sandbox_home" VTABS_ROOT="$root" VTABS_BIN="$bin" VTABS_LOG="$sandbox_home/wez-vtabs.log" \
    RUST_BACKTRACE=1 WEZTERM_LOG=info \
    wezterm --config-file "$root/scripts/dev-config.lua" \
    start --always-new-process --class vtabs-dev >"$log" 2>&1 &
  pids="$pids $!"
  ( tail -n +1 -f "$log" | grep --line-buffered -E 'vtabs:|WARN|ERROR|panic' ) &
  pids="$pids $!"
else
  say "${dim}live: hot-swapping sidebars in your running WezTerm${off}"
fi

watchexec -q --postpone -w "$root/backend/src" -e rs --debounce 200ms -n -- sh "$0" --once --"$profile" &
pids="$pids $!"
watchexec -q --postpone -w "$root/plugin" -e lua --debounce 200ms -n -- sh "$0" --check &
pids="$pids $!"

ok "watching backend/src (rebuild+swap) and plugin (tests) — ^C to stop"
wait
