#!/bin/sh
# Recover the WezTerm GUI without sacrificing the local mux unless explicitly requested.
set -eu
# shellcheck source=scripts/lib.sh
# shellcheck disable=SC1091
. "$(dirname "$0")/lib.sh"

service=${VTABS_RESTART_MUX_SERVICE:-com.fredrir.wezterm-mux}
domain=${VTABS_RESTART_MUX_DOMAIN:-localmux}
gui_app=${VTABS_RESTART_GUI_APP:-WezTerm}
mux_socket=${VTABS_RESTART_MUX_SOCKET:-$data/wezterm/localmux.sock}
launch_target=gui/$(id -u)/$service

usage() {
  cat <<EOF
usage: just restart [--list | --pane PANE_ID | --mux] [--dry-run]

  no arguments     restart only GUIs attached to $domain; every mux pane survives
  --list           list the panes in $domain without changing anything
  --pane PANE_ID   kill only that pane, then restart the attached GUI
  --pane current   kill \$WEZTERM_PANE, then restart the attached GUI
  --mux            last resort: restart the mux service, killing all of its panes
  --dry-run        print the recovery action without changing anything
EOF
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "$1 not found"
}

mux_cli() {
  WEZTERM_UNIX_SOCKET="$mux_socket" \
    wezterm cli --prefer-mux --no-auto-start "$@"
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

mux_ready() {
  mux_cli list --format json >/dev/null 2>&1
}

wait_for_mux() {
  tries=$1
  while [ "$tries" -gt 0 ]; do
    mux_ready && return 0
    tries=$((tries - 1))
    sleep 0.25
  done
  return 1
}

ensure_mux() {
  mux_ready && return 0

  # Starting an absent service cannot destroy panes: there is no live mux process to own them.
  # A live but unresponsive service is deliberately left alone for the explicit --mux fallback.
  if [ -n "$(service_pid)" ]; then
    say "  waiting for mux domain $domain"
    wait_for_mux 20 && return 0
    die "mux service is running but $domain is unavailable; nothing was killed (use --mux only as a last resort)"
  fi

  say "  starting absent mux service"
  launchctl kickstart "$launch_target" >/dev/null 2>&1 || die "could not start mux service $service"
  wait_for_mux 60 || die "mux did not become ready at $mux_socket"
}

list_panes() {
  mux_cli list
}

pane_row() {
  pane=$1
  list_panes 2>/dev/null | awk -v pane="$pane" 'NR > 1 && $3 == pane { print; found = 1 } END { exit !found }'
}

pane_count() {
  panes=$(list_panes 2>/dev/null) || return 1
  printf '%s\n' "$panes" | awk 'NR > 1 && NF { count++ } END { print count + 0 }'
}

# The mux tells us which GUI clients are attached. Filtering both host and executable avoids
# touching another WezTerm instance (notably `just dev`) or a client on another machine.
attached_gui_pids() {
  local_user=$(id -un)
  local_host=$(hostname -s 2>/dev/null || hostname)
  rows=$(mux_cli list-clients 2>/dev/null || true)
  candidates=$(printf '%s\n' "$rows" |
    awk -v user="$local_user" -v host="$local_host" \
      'NR > 1 && $1 == user && $2 == host && $3 ~ /^[0-9]+$/ { print $3 }')

  for pid in $candidates; do
    command_name=$(ps -p "$pid" -o comm= 2>/dev/null || true)
    case "$command_name" in
      wezterm-gui | */wezterm-gui) printf '%s\n' "$pid" ;;
      *) warn "  ignoring mux client pid $pid ($command_name is not wezterm-gui)" >&2 ;;
    esac
  done
}

terminate_attached_guis() {
  gui_pids=$(attached_gui_pids)
  if [ -z "$gui_pids" ]; then
    say "  no GUI attached to $domain"
    return
  fi

  n=$(printf '%s\n' "$gui_pids" | awk 'NF { count++ } END { print count + 0 }')
  say "  stopping $n GUI client(s) attached to $domain"
  # shellcheck disable=SC2086 # gui_pids is a newline-separated list of validated numeric PIDs.
  kill -TERM $gui_pids 2>/dev/null || true
  if ! wait_for_exit "$gui_pids" 40; then
    remaining=$(running_pids "$gui_pids")
    if [ -n "$remaining" ]; then
      warn "  attached GUI did not stop after 10s; forcing only that client"
      # shellcheck disable=SC2086 # remaining is a space-separated list of numeric PIDs.
      kill -KILL $remaining 2>/dev/null || true
      wait_for_exit "$remaining" 20 || die "could not stop the attached WezTerm GUI"
    fi
  fi
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

open_gui() {
  say "  opening $gui_app"
  open -na "$gui_app"
}

restart_gui() {
  ensure_mux
  terminate_attached_guis
  # The server might have failed independently while its clients were detaching. Do not replace a
  # live, unresponsive server implicitly; ensure_mux preserves it and explains the --mux fallback.
  ensure_mux
  open_gui
  ok "GUI restarted; mux panes preserved"
}

restart_after_pane() {
  pane=$1
  say "recovering WezTerm after removing pane $pane"
  mux_cli kill-pane --pane-id "$pane"
  restart_gui
}

restart_mux() {
  say "recovering WezTerm by replacing the mux"
  terminate_attached_guis
  terminate_mux
  wait_for_mux 60 || die "mux did not become ready at $mux_socket"
  open_gui
  ok "mux and GUI restarted"
}

confirm_pane() {
  pane=$1
  row=$2
  say "This terminates exactly one mux pane and the process running in it:"
  say "$row"
  printf 'Kill pane %s and restart the GUI? [y/N] ' "$pane"
  IFS= read -r answer || answer=
  case "$answer" in
    y | Y | yes | YES | Yes) return 0 ;;
    *) say "restart cancelled"; return 1 ;;
  esac
}

confirm_mux() {
  count=$1
  if [ "$count" = unknown ]; then
    say "LAST RESORT: restarting $service terminates every mux pane and its process (current count unavailable)."
  else
    say "LAST RESORT: restarting $service terminates all $count mux pane(s) and their processes."
  fi
  printf 'Restart the mux anyway? [y/N] '
  IFS= read -r answer || answer=
  case "$answer" in
    y | Y | yes | YES | Yes) return 0 ;;
    *) say "restart cancelled"; return 1 ;;
  esac
}

handoff() {
  handoff_mode=$1
  handoff_pane=${2:-}
  # A coordinator inside the pane it is about to kill cannot finish the restart. The mux-wide
  # fallback always kills its caller's pane, so both paths detach into a HUP-proof coordinator.
  # shellcheck disable=SC2154 # assigned by scripts/lib.sh
  mkdir -p "$devlogs"
  restart_log=$devlogs/restart.log
  VTABS_RESTART_COORDINATOR=1 nohup sh "$0" --coordinator "$handoff_mode" "$handoff_pane" \
    >"$restart_log" 2>&1 </dev/null &
  say "restart handed off; progress is in $restart_log"
}

run_mode() {
  run_mode_name=$1
  run_pane=${2:-}
  case "$run_mode_name" in
    gui) say "recovering WezTerm without touching mux panes"; restart_gui ;;
    pane) restart_after_pane "$run_pane" ;;
    mux) restart_mux ;;
    *) die "invalid restart mode: $run_mode_name" ;;
  esac
}

require_command launchctl
require_command open
require_command pgrep
require_command ps
require_command wezterm
[ "$(uname -s)" = Darwin ] || die "restart currently requires macOS launchd"
launchctl print "$launch_target" >/dev/null 2>&1 || die "launchd service not loaded: $launch_target"

# Private entry point used only by handoff() after the destructive prompt has already succeeded.
if [ "${1:-}" = --coordinator ] && [ "${VTABS_RESTART_COORDINATOR:-}" = 1 ]; then
  [ "$#" -ge 2 ] && [ "$#" -le 3 ] || die "invalid restart coordinator"
  run_mode "$2" "${3:-}"
  exit 0
fi

mode=gui
pane_id=
dry_run=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --list)
      [ "$mode" = gui ] || die "choose only one of --list, --pane, and --mux"
      mode=list
      ;;
    --pane)
      [ "$mode" = gui ] || die "choose only one of --list, --pane, and --mux"
      [ "$#" -ge 2 ] || die "--pane needs a pane id (use --list to find one)"
      mode=pane
      pane_id=$2
      shift
      ;;
    --mux)
      [ "$mode" = gui ] || die "choose only one of --list, --pane, and --mux"
      mode=mux
      ;;
    --dry-run) dry_run=1 ;;
    -h | --help) usage; exit 0 ;;
    *) die "unknown argument: $1 (try --help)" ;;
  esac
  shift
done

if [ "$mode" = list ]; then
  [ "$dry_run" = 0 ] || die "--dry-run has no effect with --list"
  list_panes
  exit 0
fi

if [ "$mode" = pane ]; then
  if [ "$pane_id" = current ]; then
    [ -n "${WEZTERM_PANE:-}" ] || die "--pane current requires WEZTERM_PANE; pass an id from --list"
    pane_id=$WEZTERM_PANE
  fi
  case "$pane_id" in
    '' | *[!0-9]*) die "invalid pane id: $pane_id" ;;
  esac
  row=$(pane_row "$pane_id") || die "pane $pane_id is not in mux domain $domain"
  if [ "$dry_run" = 1 ]; then
    say "would kill exactly this pane, restart attached GUI clients, and preserve every other pane:"
    say "$row"
    exit 0
  fi
  confirm_pane "$pane_id" "$row" || exit 0
  if [ "${WEZTERM_PANE:-}" = "$pane_id" ]; then
    handoff pane "$pane_id"
    exit 0
  fi
  run_mode pane "$pane_id"
  exit 0
fi

if [ "$mode" = mux ]; then
  count=$(pane_count) || count=unknown
  if [ "$dry_run" = 1 ]; then
    if [ "$count" = unknown ]; then
      say "would restart $service and destroy every mux pane (current count unavailable), then open a new GUI"
    else
      say "would restart $service and destroy all $count mux pane(s), then open a new GUI"
    fi
    exit 0
  fi
  confirm_mux "$count" || exit 0
  if [ -n "${WEZTERM_PANE:-}" ]; then
    handoff mux
    exit 0
  fi
  run_mode mux
  exit 0
fi

if [ "$dry_run" = 1 ]; then
  say "would restart only GUI clients attached to $domain; every mux pane and process would survive"
  exit 0
fi
run_mode gui
