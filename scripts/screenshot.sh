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
popover-space|Catppuccin Mocha|default|rclick_space|in_sidebar:Move to space
rename|Catppuccin Mocha|default|key:r|in_sidebar:esc cancel
rail|Catppuccin Mocha|rail|probe:toggle|rail_width
tooltip|Catppuccin Mocha|tooltip|dwell|tooltip_only
anim-mid|Catppuccin Mocha|anim|toggle_fast|animating
popover-mid|Catppuccin Mocha|anim|rclick_mid|animating
confirm|Catppuccin Mocha|confirm|confirm_close|in_sidebar:Cancel
new-tab-hover|Catppuccin Mocha|default|hover_new_tab|always
padded|Catppuccin Mocha|padded|scene|always
strip-macos|Catppuccin Mocha|macos|scene|always
rail-macos|Catppuccin Mocha|macos-rail|probe:toggle|rail_widened
rail-macos-plain|Catppuccin Mocha|macos-rail-plain|probe:toggle|rail_width
settings|Catppuccin Mocha|default|settings|settings_tab
settings-behaviour|Catppuccin Mocha|default|settings_behaviour|settings_locked
zen|Catppuccin Mocha|zen|scene|zen_frame
zen-square|Catppuccin Mocha|zen-square|scene|always
zen-rail|Catppuccin Mocha|zen-rail|probe:toggle|always
zen-statusline|Catppuccin Mocha|zen|statusline|zen_statusline"

cli() { wezterm cli --no-auto-start "$@"; }
# A gui busy with a spawn or a resize can answer an empty body; every helper here parses this, and
# `set -e` would kill the whole run over one transient. Retry instead.
list() {
  for _ in 1 2 3 4 5; do
    body=$(cli list --format json 2>/dev/null || true)
    case "$body" in
      \[*) printf '%s' "$body"; return 0 ;;
    esac
    sleep 0.3
  done
  echo "[]"
}
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
pane_text() { cli get-text --pane-id "$1"; }
pane_cols() { pick "p=$1;print([q['size']['cols'] for q in json.load(sys.stdin) if q['pane_id']==p][0])"; }
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
    # `Move to space` is disabled, and `popover.move` skips disabled rows, so it cannot be selected
    # and Return would run whatever the cursor skipped onto. The row is visible in the root menu.
    rclick_space)
      point_at 3 logs
      xdotool key Down Down
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
    # `Tab` cycles the nav; Behaviour is the fourth group, and the harness config sets poll_ms,
    # confirm_close and debug, so those rows are the ones precedence has to lock.
    settings_behaviour)
      probe "$(content_of "$(tab_ids | cut -d' ' -f2)")" settings
      sleep 3.5
      shot_tab=$(pick 'ps=[p["tab_id"] for p in json.load(sys.stdin) if p["title"].startswith("wez-vtabs-settings")];print(ps[0] if ps else "")' 2>/dev/null || echo "")
      focus_window || return 1
      xdotool key Tab
      sleep 0.4
      xdotool key Tab
      sleep 0.4
      xdotool key Tab
      sleep 2
      ;;
    probe:*)
      probe "$(content_of "$(tab_ids | cut -d' ' -f2)")" "${1#probe:}"
      sleep 2.5
      ;;
    # The settings page opens in a tab of its own; the shot is of that tab and its sidebar.
    settings)
      probe "$(content_of "$(tab_ids | cut -d' ' -f2)")" settings
      sleep 3.5
      shot_tab=$(pick 'print([p["tab_id"] for p in json.load(sys.stdin) if p["title"].startswith("wez-vtabs-settings")][0])' 2>/dev/null || echo "")
      sleep 1
      ;;
    # A tmux/vim status line, reduced to what makes it damage the frame: one full-width row of
    # explicit-bg cells on the content pane's last row. `sleep` holds it there past the capture,
    # and `quick` keeps the pre-shot `clear` from wiping it.
    statusline)
      bar_pane=$(content_of "$(tab_ids | cut -d' ' -f2)")
      bar=$(awk -v n="$(pane_cols "$bar_pane")" 'BEGIN{while(n-->0)printf " "}')
      # `clear` runs after the echo of the line that carries it, so only the bar is left on screen.
      cli send-text --no-paste --pane-id "$bar_pane" "clear; printf '\\033[999;1H\\033[48;5;33m$bar\\033[0m'; sleep 300
"
      sleep 2.5
      quick=1
      ;;
    toggle_fast)
      focus_window
      probe "$(content_of "$(tab_ids | cut -d' ' -f2)")" toggle
      sleep 0.12
      quick=1
      ;;
  esac
}

# --- zen frame sampling ----------------------------------------------------
# `frame.margin` and `frame.radius` defaults; the shots never set them, and the corner assertions
# below are sized from the radius (an 8 px arc leaves ~8 tint pixels in an 8x8 corner box).
frame_margin=8
frame_radius=8
dominant() { # png x y w h — the most common colour in a region, as RRGGBB
  magick "$1" -crop "$4x$5+$2+$3" +repage -depth 8 -format %c histogram:info: |
    sort -rn | sed -n 's/.*#\([0-9A-Fa-f]\{6\}\).*/\1/p' | head -1
}
colour_count() { # png x y w h hex — how many pixels of that exact colour the region holds
  magick "$1" -crop "$4x$5+$2+$3" +repage -depth 8 -format %c histogram:info: |
    awk -v want="#$6" '{n=$1; sub(/:$/,"",n); if ($3==want) {print n; f=1}} END{if(!f) print 0}'
}
# The same rectangle `frame.lua M.rect` computes: the card starts one divider column right of the
# sidebar and is `frame.margin` in from every window edge.
zen_rect() { # png — sets WIN_W WIN_H CARD_X CARD_Y CARD_W CARD_H SB_W TINT
  size=$(magick identify -format "%w %h" "$1" 2>/dev/null || true)
  [ -n "$size" ] || return 1
  WIN_W=${size% *}
  WIN_H=${size#* }
  grid=$((WIN_W - 2 * frame_margin))
  SB_W=$((sidebar_cols * grid / cols))
  CARD_X=$((frame_margin + (sidebar_cols + 1) * grid / cols))
  CARD_Y=$frame_margin
  CARD_W=$((WIN_W - frame_margin - CARD_X))
  CARD_H=$((WIN_H - 2 * frame_margin))
  TINT=$(dominant "$1" 0 0 "$WIN_W" "$frame_margin")
}
corner_tint() { # png corner_x corner_y — tint pixels left in the arc's corner box
  colour_count "$1" "$2" "$3" "$frame_radius" "$frame_radius" "$TINT"
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
    # `rail_titlebar = "widen"` grows the rail to the traffic-light reserve, so it is wider than
    # `rail_width` by design and only has to stay a rail.
    rail_widened)
      have_sidebar || fail_state "rail detached the pane instead of narrowing it"
      w=$(pick "p=$(active_sidebar);print([q['size']['cols'] for q in json.load(sys.stdin) if q['pane_id']==p][0])")
      [ "$w" -lt "$sidebar_cols" ] || fail_state "sidebar is $w cols, not a rail"
      [ "$w" -ge 5 ] || fail_state "sidebar is $w cols, under rail_width=5"
      ;;
    # implementer-1 is still building the page, so the check is the tab and its marker, never
    # what the page draws.
    settings_tab)
      settings_pane=$(pick 'ps=[p for p in json.load(sys.stdin) if p["title"].startswith("wez-vtabs-settings")];print(ps[0]["pane_id"] if ps else "")')
      [ -n "$settings_pane" ] || fail_state "no pane carries the wez-vtabs-settings: marker"
      settings_tab_id=$(pick "s=$settings_pane;print([p['tab_id'] for p in json.load(sys.stdin) if p['pane_id']==s][0])")
      beside=$(pick "t=$settings_tab_id;print(sum(1 for p in json.load(sys.stdin) if p['tab_id']==t and p['title'].startswith('wez-vtabs:')))")
      [ "$beside" -eq 1 ] || fail_state "the settings tab has $beside sidebars beside it, want 1"
      sidebar_text | grep -qF Settings || fail_state "the sidebar does not list a Settings card"
      # A page that painted only its chrome passes "not blank"; a nav row beside its divider does not.
      pane_text "$settings_pane" | grep -q "Layout.*│" ||
        fail_state "the settings page has no nav row beside its divider"
      pane_text "$settings_pane" | sed -n 1p | grep -qF "Settings" ||
        fail_state "the settings page has no header on row 1"
      pane_text "$settings_pane" | grep -vE "^[[:space:]]*$" | tail -1 | grep -qF "esc" ||
        fail_state "the settings page has no hint bar on its last row"
      ;;
    # `frame = "zen"` insets the terminal as a rounded card inside a tinted frame, so the corner
    # pixels of the content rect are the frame and the middle is the terminal background.
    zen_frame)
      png=$out/$state.png
      [ -s "$png" ] || { fail_state "no window shot to read the frame from"; return 1; }
      zen_rect "$png" || { fail_state "could not sample the window shot"; return 1; }
      middle=$(dominant "$png" $((CARD_X + CARD_W / 4)) $((CARD_Y + CARD_H / 2)) \
        $((CARD_W / 2)) $((CARD_H / 4)))
      [ -n "$TINT" ] && [ -n "$middle" ] || { fail_state "could not sample the window shot"; return 1; }
      [ "$TINT" != "$middle" ] ||
        fail_state "the frame tint and the terminal background are the same colour (#$TINT)"
      echo "  zen frame #$TINT, terminal #$middle"
      # The sidebar hides the frame image by painting every cell with an explicit bg of the same
      # colour the frame is filled with, so the seam between the two is only invisible while the
      # two bytes match exactly. Anything else draws a rectangle around the sidebar.
      interior=$(dominant "$png" "$frame_margin" "$frame_margin" "$SB_W" "$CARD_H")
      bottom=$(dominant "$png" 0 $((WIN_H - frame_margin)) "$WIN_W" "$frame_margin")
      [ "$bottom" = "$TINT" ] ||
        fail_state "the frame margin is not one colour (top $TINT, bottom $bottom)"
      # Soft until implementer-2 lands the fix for the (3,3,6) mismatch; make it `fail_state` then.
      [ "$interior" = "$TINT" ] ||
        xfail_state "the sidebar interior is #$interior, the frame margin #$TINT"
      ;;
    # A full-width explicit-bg bottom row is opaque, so it squares the two corners it covers. That
    # is accepted damage (P1-addendum-3 §"Apps that paint an explicit background"); what the check
    # pins is that it stays local — the top corners keep their arc and the band below is untouched.
    zen_statusline)
      png=$out/$state.png
      [ -s "$png" ] || { fail_state "no window shot to read the frame from"; return 1; }
      zen_rect "$png" || { fail_state "could not sample the window shot"; return 1; }
      bottom_y=$((CARD_Y + CARD_H - frame_radius))
      right_x=$((CARD_X + CARD_W - frame_radius))
      bl=$(corner_tint "$png" "$CARD_X" "$bottom_y")
      br=$(corner_tint "$png" "$right_x" "$bottom_y")
      tl=$(corner_tint "$png" "$CARD_X" "$CARD_Y")
      tr=$(corner_tint "$png" "$right_x" "$CARD_Y")
      echo "  corner tint px: tl $tl tr $tr bl $bl br $br (tint #$TINT)"
      [ "$tl" -gt 0 ] || fail_state "the top-left corner lost its arc; the damage is not local"
      [ "$tr" -gt 0 ] || fail_state "the top-right corner lost its arc; the damage is not local"
      [ "$bl" -eq 0 ] || fail_state "the bar left $bl tint px in the bottom-left corner; it never reached it"
      [ "$br" -eq 0 ] || fail_state "the bar left $br tint px in the bottom-right corner; it never reached it"
      # Below the card is the frame's own band: no cell reaches it, so an opaque row cannot mark it.
      band_h=$((WIN_H - CARD_Y - CARD_H))
      band=$(colour_count "$png" 0 $((CARD_Y + CARD_H)) "$WIN_W" "$band_h" "$TINT")
      [ "$band" -eq $((WIN_W * band_h)) ] ||
        fail_state "the band below the card is $band of $((WIN_W * band_h)) px tint; the bar bled into it"
      ;;
    # An option the host set in wezterm.lua cannot be edited here; the badge is the whole point of
    # the Behaviour frame, so a page that merely reached the group does not pass.
    settings_locked)
      settings_pane=$(pick 'ps=[p for p in json.load(sys.stdin) if p["title"].startswith("wez-vtabs-settings")];print(ps[0]["pane_id"] if ps else "")')
      [ -n "$settings_pane" ] || fail_state "no pane carries the wez-vtabs-settings: marker"
      pane_text "$settings_pane" | grep -q "Behaviour" ||
        fail_state "the nav is not on the Behaviour group"
      pane_text "$settings_pane" | grep -q "poll_ms.*LOCKED" ||
        fail_state "the poll_ms row carries no LOCKED badge"
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
# A check that pins a defect someone else is fixing: loud, but it keeps the PNG, so the shot still
# reaches the baseline and design review instead of vanishing until the fix lands.
xfail_state() {
  echo "XFAIL: $state — $1"
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
  shoot "$state" "$scheme" "$variant" "$setup" "$want" || echo "FAIL: $state — the state aborted"
done
echo "shots in $out"
