#!/bin/sh
# Destructively restart the local WezTerm GUI + launchd mux stack.
set -eu
# shellcheck source=scripts/lib.sh
# shellcheck disable=SC1091
. "$(dirname "$0")/lib.sh"

service=${VTABS_RESTART_MUX_SERVICE:-com.fredrir.wezterm-mux}
domain=${VTABS_RESTART_MUX_DOMAIN:-localmux}
gui_app=${VTABS_RESTART_GUI_APP:-WezTerm}
mux_socket=${VTABS_RESTART_MUX_SOCKET:-$data/wezterm/localmux.sock}
launch_target=gui/$(id -u)/$service

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "$1 not found"
}

confirm_restart() {
  say "This kills every WezTerm GUI, every mux pane, and the processes running in those panes."
  say "It then restarts $service and opens a fresh $gui_app GUI."

  while :; do
    printf 'Continue? [Y/n] '
    if ! IFS= read -r answer; then
      say ""
      say "restart cancelled"
      return 1
    fi
    case "$answer" in
      "" | y | Y | yes | YES | Yes) return 0 ;;
      n | N | no | NO | No)
        say "restart cancelled"
        return 1
        ;;
      *) say "please answer y or n" ;;
    esac
  done
}

running_pids() {
  kept=
  for pid in $1; do
    if kill -0 "$pid" 2>/dev/null; then
      kept="$kept $pid"
    fi
  done
  printf '%s' "$kept"
}

wait_for_exit() {
  watched=$1
  tries=$2
  while [ "$tries" -gt 0 ]; do
    watched=$(running_pids "$watched")
    [ -z "$watched" ] && return 0
    tries=$((tries - 1))
    sleep 0.25
  done
  return 1
}

terminate_guis() {
  gui_pids=$(pgrep -x wezterm-gui 2>/dev/null || true)
  if [ -z "$gui_pids" ]; then
    say "  no running GUI"
    return
  fi

  n=$(printf '%s\n' "$gui_pids" | wc -l | tr -d ' ')
  say "  stopping $n GUI process(es)"
  # shellcheck disable=SC2086 # gui_pids is a newline-separated list of numeric PIDs.
  kill -TERM $gui_pids 2>/dev/null || true
  if ! wait_for_exit "$gui_pids" 40; then
    remaining=$(running_pids "$gui_pids")
    if [ -n "$remaining" ]; then
      warn "  GUI did not stop after 10s; forcing it"
      # shellcheck disable=SC2086 # remaining is a space-separated list of numeric PIDs.
      kill -KILL $remaining 2>/dev/null || true
      wait_for_exit "$remaining" 20 || die "could not stop every WezTerm GUI"
    fi
  fi
}

service_pid() {
  launchctl print "$launch_target" 2>/dev/null |
    sed -n 's/^[[:space:]]*pid = \([0-9][0-9]*\)$/\1/p' |
    sed -n '1p'
}

child_mux_pids() {
  parent=$1
  [ -n "$parent" ] || return 0
  pgrep -P "$parent" -x wezterm-mux-server 2>/dev/null || true
}

terminate_mux() {
  old_service_pid=$(service_pid)
  old_mux_pids=$(child_mux_pids "$old_service_pid")

  say "  stopping mux service"
  launchctl kill SIGTERM "$launch_target" 2>/dev/null || true

  old_pids="$old_service_pid $old_mux_pids"
  if ! wait_for_exit "$old_pids" 40; then
    remaining=$(running_pids "$old_pids")
    if [ -n "$remaining" ]; then
      warn "  mux did not stop after 10s; forcing it"
      # shellcheck disable=SC2086 # remaining is a space-separated list of numeric PIDs.
      kill -KILL $remaining 2>/dev/null || true
      wait_for_exit "$remaining" 20 || die "could not stop the old mux service"
    fi
  fi

  # KeepAlive will usually have started it already; kickstart also bypasses launchd's throttle delay.
  if ! launchctl kickstart "$launch_target" >/dev/null 2>&1; then
    [ -n "$(service_pid)" ] || die "could not start mux service $service"
  fi
}

wait_for_mux() {
  tries=60
  while [ "$tries" -gt 0 ]; do
    if WEZTERM_UNIX_SOCKET="$mux_socket" \
      wezterm cli --prefer-mux --no-auto-start list --format json >/dev/null 2>&1; then
      return 0
    fi
    tries=$((tries - 1))
    sleep 0.25
  done
  die "mux did not become ready at $mux_socket"
}

restart_stack() {
  say "restarting WezTerm"
  terminate_guis
  terminate_mux
  say "  waiting for mux domain $domain"
  wait_for_mux
  say "  opening $gui_app"
  open -na "$gui_app"
  ok "restart complete"
}

require_command launchctl
require_command open
require_command pgrep
require_command wezterm
[ "$(uname -s)" = Darwin ] || die "restart currently requires macOS launchd"
launchctl print "$launch_target" >/dev/null 2>&1 || die "launchd service not loaded: $launch_target"

if [ "${1:-}" = --confirmed ] && [ "${VTABS_RESTART_CONFIRMED:-}" = 1 ]; then
  restart_stack
  exit 0
fi
[ "$#" -eq 0 ] || die "usage: just restart"

confirm_restart || exit 0

# A foreground coordinator launched inside WezTerm would be killed with its own pane. Ignore HUP and
# hand it off first; callers from another terminal retain synchronous progress and failures.
if [ -n "${WEZTERM_PANE:-}" ]; then
  # shellcheck disable=SC2154 # assigned by scripts/lib.sh
  mkdir -p "$devlogs"
  # shellcheck disable=SC2154 # assigned by scripts/lib.sh
  restart_log=$devlogs/restart.log
  VTABS_RESTART_CONFIRMED=1 nohup sh "$0" --confirmed >"$restart_log" 2>&1 </dev/null &
  say "restart handed off; progress is in $restart_log"
  exit 0
fi

restart_stack
