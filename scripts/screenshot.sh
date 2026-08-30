#!/bin/sh
# Shoots the sidebar in a throwaway WezTerm. Run under a display: `just screenshot [state]`.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
out=${VTABS_SHOTS:-$root/.claude/team/shots}
bin="${VTABS_BIN:-$root/backend/target/release/wez-vtabs}"
cols=120
sidebar_cols=28

[ -n "${DISPLAY:-}" ] || { echo "no DISPLAY; run under xvfb-run (just screenshot)"; exit 1; }
for tool in wezterm import magick xdotool python3; do
  command -v "$tool" >/dev/null || { echo "missing: $tool"; exit 1; }
done
[ -x "$bin" ] || (cd "$root/backend" && cargo build --locked --release)
mkdir -p "$out"

# state | scheme | opts variant (screenshot.lua) | setup step | check that the feature is really there
STATES="tabs|Catppuccin Mocha|default|scene|always
hover|Catppuccin Mocha|default|hover|always
collapsed|Catppuccin Mocha|hidden|probe:toggle|no_sidebar
private|Catppuccin Mocha|default|probe:private|always
light|Catppuccin Latte|default|scene|always
zeroconfig|Catppuccin Mocha|zeroconfig|scene|always
elevation|Catppuccin Mocha|elevation|scene|always
elevation-1|Catppuccin Mocha|elevation-1|scene|always
press|Catppuccin Mocha|press|hover|always
popover|Catppuccin Mocha|default|rclick|in_sidebar:Duplicate tab
popover-space|Catppuccin Mocha|default|rclick_space|in_sidebar:Spaces arrive in P4
rename|Catppuccin Mocha|default|key:r|in_sidebar:esc cancel
rail|Catppuccin Mocha|rail|probe:toggle|rail_width
tooltip|Catppuccin Mocha|tooltip|dwell|tooltip_only
anim-mid|Catppuccin Mocha|anim|toggle_fast|animating
popover-mid|Catppuccin Mocha|anim|rclick_mid|animating
confirm|Catppuccin Mocha|confirm|confirm_close|in_sidebar:Cancel
new-tab-hover|Catppuccin Mocha|default|hover_new_tab|always
padded|Catppuccin Mocha|padded|scene|always
strip-macos|Catppuccin Mocha|macos|scene|always
rail-macos|Catppuccin Mocha|macos-rail|probe:toggle|rail_width"

cli() { wezterm cli --no-auto-start "$@"; }
list() { cli list --format json; }
pick() { list | python3 -c "import json,sys;$1"; }
sidebars() { pick 'print("\n".join(str(p["pane_id"]) for p in json.load(sys.stdin) if p["title"].startswith("wez-vtabs")))'; }
tab_ids() { pick 'print(" ".join(str(t) for t in sorted({p["tab_id"] for p in json.load(sys.stdin)})))'; }
content_of() { pick "t=$1;print([p['pane_id'] for p in json.load(sys.stdin) if p['tab_id']==t and not p['title'].startswith('wez-vtabs')][0])"; }
sidebar_of() { pick "t=$1;print([p['pane_id'] for p in json.load(sys.stdin) if p['tab_id']==t and p['title'].startswith('wez-vtabs')][0])"; }
# The popover only paints in the active tab's sidebar, so a step that switches tabs says so here.
shot_tab=""
active_sidebar() { sidebar_of "${shot_tab:-$(tab_ids | cut -d' ' -f2)}"; }
# Pixel centre of a sidebar column, so a click lands inside a three-column span.
col_x() { echo $((X + WIDTH * (2 * $1 - 1) / (2 * cols))); }
sidebar_text() { cli get-text --pane-id "$(active_sidebar)"; }
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

# Xvfb has no window manager, so nothing gives the window input focus; without it wezterm drops
# every non-wheel mouse event and every key (`is_focused`, mouseevent.rs:730).
focus_window() {
  win=$(window_id)
  [ -n "$win" ] || return 1
  eval "$(xdotool getwindowgeometry --shell "$win")"
  xdotool windowfocus "$win"
  sleep 0.5
}

card_y() { # y pixel of the card whose title is $1, from the sidebar's own text
  row=$(sidebar_text | python3 -c "import sys;rows=sys.stdin.read().split('\n');print(next((i for i,l in enumerate(rows) if '$1' in l),2))")
  echo $((Y + HEIGHT * (2 * row + 1) / 68))
}

# How long to let the click settle before the shot; a mid-animation state shortens it.
click_settle=1.5
point_at() { # button ("" = move only), title
  focus_window || return 1
  x=$((X + WIDTH * 5 / cols))
  y=$(card_y "$2")
  # the first motion only moves focus into the sidebar, and reports are per cell, so cross rows
  xdotool mousemove --sync "$x" $((y - 3 * HEIGHT / 34))
  sleep 0.8
  xdotool mousemove --sync "$x" "$y"
  sleep 1
  [ -z "$1" ] || { xdotool click "$1"; sleep "$click_settle"; }
}

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

# Each step leaves the window in the state to shoot. `quick` skips the settle before capture.
step() {
  quick=
  shot_tab=
  case $1 in
    scene) ;;
    hover) point_at "" logs || echo "  no X window; hover skipped" ;;
    hover_new_tab) point_at "" "New tab" || echo "  no X window; hover skipped" ;;
    dwell)
      # hover first, snapshot inside the delay, then rest past it: the diff is the tooltip
      point_at "" logs || return 1
      sidebar_text >"$home/before.txt"
      sleep 4
      ;;
    rclick) point_at 3 logs ;;
    # `popover_in` really runs for 90 ms, which `import` cannot catch; the probe stretches it 10x
    # so the shot lands at the same 44% blend a 40 ms capture of the real fade would.
    rclick_mid)
      probe "$(content_of "$(tab_ids | cut -d' ' -f2)")" slow_popover
      sleep 1
      click_settle=0.4
      point_at 3 logs || return 1
      click_settle=1.5
      quick=1
      ;;
    rclick_space)
      point_at 3 logs
      xdotool key Down Down Down Return
      sleep 1.5
      ;;
    key:*)
      focus_window
      probe "$(content_of "$(tab_ids | cut -d' ' -f2)")" focus
      sleep 1.5
      xdotool key "${1#key:}"
      sleep 1.5
      ;;
    # The x only asks when closing would prompt, so the victim tab has to be busy first.
    confirm_close)
      logs_tab=$(tab_ids | cut -d' ' -f3)
      cli send-text --no-paste --pane-id "$(content_of "$logs_tab")" "sleep 1000
"
      sleep 1.5
      cli activate-tab --tab-id "$logs_tab"
      shot_tab=$logs_tab
      sleep 1
      focus_window || return 1
      # `close_x = card_x2 - 1`; at width 28 with padding.right = 2 that is column 25.
      x=$(col_x 25)
      y=$(card_y logs)
      xdotool mousemove --sync "$x" $((y - 3 * HEIGHT / 34))
      sleep 0.8
      xdotool mousemove --sync "$x" "$y"
      sleep 1
      xdotool click 1
      sleep 1.5
      ;;
    probe:*)
      probe "$(content_of "$(tab_ids | cut -d' ' -f2)")" "${1#probe:}"
      sleep 2.5
      ;;
    toggle_fast)
      focus_window
      probe "$(content_of "$(tab_ids | cut -d' ' -f2)")" toggle
      sleep 0.12
      quick=1
      ;;
  esac
}

# Loud checks: a state whose feature has not landed must not leave a stale or misleading PNG.
check() {
  failed=
  case $1 in
    always) ;;
    no_sidebar) [ -z "$(sidebars)" ] || fail_state "the sidebar pane is still there" ;;
    in_sidebar:*) sidebar_text | grep -qF "${1#in_sidebar:}" || fail_state "sidebar has no '${1#in_sidebar:}'" ;;
    rail_width)
      have_sidebar || fail_state "rail detached the pane instead of narrowing it"
      w=$(pick "p=$(active_sidebar);print([q['size']['cols'] for q in json.load(sys.stdin) if q['pane_id']==p][0])")
      [ "$w" -le 9 ] || fail_state "sidebar is $w cols, not a rail"
      [ "$w" -eq 5 ] || echo "  warn: rail is $w cols, not rail_width=5 (geometry.lua MIN_WIDTH clamps it)"
      ;;
    tooltip_only)
      sidebar_text >"$home/after.txt"
      cmp -s "$home/before.txt" "$home/after.txt" &&
        fail_state "resting on a card changed nothing; no tooltip"
      if grep -qF 'Duplicate tab' "$home/after.txt"; then fail_state "that is a popover, not a tooltip"; fi
      ;;
    animating)
      # the backend answers every animation with anim_done, so this proves one really played
      wait_for 5 grep -q '"t":"anim_done"' "$home/log" || fail_state "no animation reached the backend"
      ;;
  esac
  [ -z "$failed" ]
}

fail_state() {
  failed=1
  echo "FAIL: $state — $1"
  rm -f "$out/$state.png" "$out/$state-sidebar.png"
  cp "$home/log" "$out/$state.log"
}

capture() {
  if [ -z "$quick" ]; then
    # the probes are typed at a shell, so wipe their echo out of every content pane
    for p in $(pick 'print(" ".join(str(p["pane_id"]) for p in json.load(sys.stdin) if not p["title"].startswith("wez-vtabs")))'); do
      cli send-text --no-paste --pane-id "$p" "clear
"
    done
    sleep 1.5
  fi
  import -window root "$out/$state.png"
  win=$(window_id)
  [ -n "$win" ] || return 0
  eval "$(xdotool getwindowgeometry --shell "$win")"
  magick "$out/$state.png" -crop "${WIDTH}x$HEIGHT+$X+$Y" +repage "$out/$state.png"
  magick "$out/$state.png" -crop "$((WIDTH * sidebar_cols / cols))x${HEIGHT}+0+0" +repage "$out/$state-sidebar.png"
}

shoot() { # state, scheme, opts variant, step, check
  state=$1
  rm -f "$out/$state.png" "$out/$state-sidebar.png" "$out/$state.log"
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
    step "$4" || true
    capture
    if check "$5"; then
      echo "ok: $out/$state.png"
      [ -z "${VTABS_SHOT_KEEP_LOG:-}" ] || cp "$home/log" "$out/$state.log"
    fi
  else
    echo "FAIL: $state — no sidebar at startup; see $out/$state.log"
    cp "$home/log" "$out/$state.log"
  fi
  kill $pid 2>/dev/null || true
  wait $pid 2>/dev/null || true
  rm -rf "$home"
}

# Any number of state names; none means all. Only the requested states are re-shot, so a run
# never disturbs the PNGs it was not asked for.
only=$*
for want_state in $only; do
  echo "$STATES" | cut -d'|' -f1 | grep -qx "$want_state" ||
    { echo "unknown state: $want_state; one of $(echo "$STATES" | cut -d'|' -f1 | tr '\n' ' ')"; exit 1; }
done
echo "$STATES" | while IFS='|' read -r state scheme variant setup want; do
  [ -n "$state" ] || continue
  if [ -n "$only" ]; then
    case " $only " in
      *" $state "*) ;;
      *) continue ;;
    esac
  fi
  shoot "$state" "$scheme" "$variant" "$setup" "$want"
done
echo "shots in $out"
