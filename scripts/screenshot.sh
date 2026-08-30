#!/bin/sh
# Shoots the sidebar in a throwaway WezTerm. Run under a display: `just screenshot`.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
out=${VTABS_SHOTS:-/tmp/vtabs-team/p1-shots}
bin="${VTABS_BIN:-$root/backend/target/release/wez-vtabs}"
cols=120
sidebar_cols=28

[ -n "${DISPLAY:-}" ] || { echo "no DISPLAY; run under xvfb-run (just screenshot)"; exit 1; }
for tool in wezterm import xdotool python3; do
  command -v "$tool" >/dev/null || { echo "missing: $tool"; exit 1; }
done
[ -x "$bin" ] || (cd "$root/backend" && cargo build --locked --release)
mkdir -p "$out"

# state | scheme | opts variant (screenshot.lua) | probe run after the scene is built
STATES="tabs|Catppuccin Mocha|default|
hover|Catppuccin Mocha|default|hover
collapsed|Catppuccin Mocha|default|toggle
private|Catppuccin Mocha|default|private
light|Catppuccin Latte|default|
zeroconfig|Catppuccin Mocha|zeroconfig|
elevation|Catppuccin Mocha|elevation|
elevation-1|Catppuccin Mocha|elevation-1|
press|Catppuccin Mocha|press|hover"

cli() { wezterm cli --no-auto-start "$@"; }
list() { cli list --format json; }
pick() { list | python3 -c "import json,sys;$1"; }
sidebars() { pick 'print("\n".join(str(p["pane_id"]) for p in json.load(sys.stdin) if p["title"].startswith("wez-vtabs")))'; }
tab_ids() { pick 'print(" ".join(str(t) for t in sorted({p["tab_id"] for p in json.load(sys.stdin)})))'; }
content_of() { pick "t=$1;print([p['pane_id'] for p in json.load(sys.stdin) if p['tab_id']==t and not p['title'].startswith('wez-vtabs')][0])"; }
sidebar_of() { pick "t=$1;print([p['pane_id'] for p in json.load(sys.stdin) if p['tab_id']==t and p['title'].startswith('wez-vtabs')][0])"; }
probe() { cli send-text --no-paste --pane-id "$1" "printf '\\033]1337;SetUserVar=vtabs_shot=$(printf %s "$2" | base64)\\a'
"; }

wait_for() { # seconds, command...
  n=$(($1 * 5)); shift
  while [ "$n" -gt 0 ]; do
    "$@" >/dev/null 2>&1 && return 0
    n=$((n - 1)); sleep 0.2
  done
  return 1
}
have_sidebar() { [ -n "$(sidebars 2>/dev/null)" ]; }

# Newest mapped window of our class; the private state opens a second one on top.
window_id() { xdotool search --class vtabs-shot 2>/dev/null | tail -1; }

build_scene() { # three tabs: one pinned, one with unseen output, tab 2 active
  first=$(tab_ids | cut -d' ' -f1)
  cli set-tab-title --tab-id "$first" server
  cli spawn --pane-id "$(content_of "$first")" >/dev/null
  wait_for 15 test 2 -eq "$(sidebars | wc -l | tr -d ' ')" || true
  second=$(tab_ids | cut -d' ' -f2)
  cli set-tab-title --tab-id "$second" editor
  cli spawn --pane-id "$(content_of "$second")" >/dev/null
  wait_for 15 test 3 -eq "$(sidebars | wc -l | tr -d ' ')" || true
  third=$(tab_ids | cut -d' ' -f3)
  cli set-tab-title --tab-id "$third" logs
  cli activate-tab --tab-id "$first"
  sleep 0.5
  probe "$(content_of "$first")" pin
  sleep 1
  cli activate-tab --tab-id "$second"
  sleep 0.5
  # output in a background tab is what sets has_unseen_output
  cli send-text --no-paste --pane-id "$(content_of "$third")" "date; ls /etc | head -3
"
  sleep 1.5
}

hover_row() { # pointer over a tab row, through the real X server
  win=$(window_id)
  [ -n "$win" ] || return 1
  eval "$(xdotool getwindowgeometry --shell "$win")"
  # Xvfb has no window manager, so nothing gives the window input focus; without it wezterm
  # drops every non-wheel mouse event (`is_focused`, mouseevent.rs:730).
  xdotool windowfocus "$win"
  sleep 0.5
  row=$(cli get-text --pane-id "$(sidebar_of "$(tab_ids | cut -d' ' -f3)")" |
    python3 -c 'import sys;rows=sys.stdin.read().split("\n");print(next((i for i,l in enumerate(rows) if "logs" in l),2))')
  x=$((X + WIDTH * 5 / cols))
  y=$((Y + HEIGHT * (2 * row + 1) / 68))
  # the first motion only moves focus into the sidebar, and reports are per cell, so cross rows
  xdotool mousemove --sync "$x" $((y - 3 * HEIGHT / 34))
  sleep 0.8
  xdotool mousemove --sync "$x" "$y"
  sleep 1.5
}

shoot() { # state, scheme, opts variant, probe
  state=$1
  home=$(mktemp -d /tmp/vtshot.XXXXXX)
  mkdir -p "$home/run"
  HOME="$home" XDG_RUNTIME_DIR="$home/run" VTABS_ROOT="$root" VTABS_BIN="$bin" VTABS_SHOT_SCHEME="$2" \
    VTABS_SHOT_OPTS="$3" \
    wezterm --config-file "$root/scripts/screenshot.lua" start --always-new-process --class vtabs-shot \
    >"$home/log" 2>&1 &
  pid=$!
  export WEZTERM_UNIX_SOCKET="$home/run/wezterm/gui-sock-$pid"
  if wait_for 20 have_sidebar; then
    build_scene
    case $4 in
      hover) hover_row || echo "  hover: no X window, skipped" ;;
      "") ;;
      *) probe "$(content_of "$(tab_ids | cut -d' ' -f2)")" "$4"; sleep 2.5 ;;
    esac
    # the probes are typed at a shell, so wipe their echo out of every content pane
    for p in $(pick 'print(" ".join(str(p["pane_id"]) for p in json.load(sys.stdin) if not p["title"].startswith("wez-vtabs")))'); do
      cli send-text --no-paste --pane-id "$p" "clear
"
    done
    sleep 1.5
    import -window root "$out/$state.png"
    win=$(window_id)
    if [ -n "$win" ]; then
      eval "$(xdotool getwindowgeometry --shell "$win")"
      magick "$out/$state.png" -crop "${WIDTH}x$HEIGHT+$X+$Y" +repage "$out/$state.png"
      magick "$out/$state.png" -crop "$((WIDTH * sidebar_cols / cols))x${HEIGHT}+0+0" +repage "$out/$state-sidebar.png"
    fi
    echo "ok: $out/$state.png"
    [ -z "${VTABS_SHOT_KEEP_LOG:-}" ] || cp "$home/log" "$out/$state.log"
  else
    echo "FAIL: $state has no sidebar; see $home/log"
    cp "$home/log" "$out/$state.log"
  fi
  kill $pid 2>/dev/null || true
  wait $pid 2>/dev/null || true
  rm -rf "$home"
}

only=${1:-}
echo "$STATES" | while IFS='|' read -r state scheme variant extra; do
  [ -n "$state" ] || continue
  [ -z "$only" ] || [ "$only" = "$state" ] || continue
  shoot "$state" "$scheme" "$variant" "$extra"
done
echo "shots in $out"
