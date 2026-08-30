#!/bin/sh
# Stress sequences for the two reported bugs: duplicate sidebars (item 5) and size drift (item 9).
# `sh plugin/tests/stress.sh [local|mux]`; every step asserts the one-sidebar-per-tab invariant.
mode="${1:-local}"
export VTABS_E2E_COLLAPSED=rail
. "$(dirname "$0")/e2e-lib.sh"
e2e_launch
trap e2e_cleanup EXIT

for _ in $(seq 1 40); do
  [ -n "$(sidebar_panes 2>/dev/null)" ] && break
  sleep 0.5
done
[ -n "$(sidebar_panes)" ] || fail "no sidebar pane after startup"
first_tab=$(tab_ids | cut -d' ' -f1)
first_content=$(content_of "$first_tab")
no_dupes startup
echo "ok: startup leaves one sidebar on the first tab"

# `rail_titlebar = "widen"` widens the rail to the traffic-light reserve, so the expected rail width
# is `rail_width` everywhere the reserve is zero and the reserve itself under the macOS seam.
rail_want=5

# `VTABS_E2E_MACOS=1` fakes the traffic-light reserve on, which is the only way to run the rail's
# own strip geometry outside macOS. `rail_titlebar` defaults to "widen".
macos_rail() {
  mac_mark=$(mark)
  mac_sb=$(sidebar_of "$first_tab")
  vtest "$first_content" rail_mode
  sleep 0.5
  vtest "$first_content" toggle
  sleep 3
  rail_cols=$(cols_of "$mac_sb")
  mac_hits=$(probe_line "$first_content" probe_hits hits)
  reserve=$(probe_line "$first_content" probe_reserve reserve)
  echo "  macOS rail: $rail_cols cols; reserve $reserve; hits: $mac_hits"
  # The toggle is either a record of its own or a span inside an `action` record.
  toggle_hit=$(printf '%s\n' "$mac_hits" | tr ' ' '\n' | grep -E '^toggle/|,toggle@' | head -1)
  [ -n "$toggle_hit" ] || { sidebar_text "$mac_sb"; fail "the macOS rail has no toggle hit row"; }
  case "$toggle_hit" in
    *,toggle@*)
      toggle_span=${toggle_hit##*,toggle@}
      toggle_x2=${toggle_span%%,*}
      toggle_x2=${toggle_x2#*-}
      ;;
    *) toggle_x2=$(printf '%s' "$toggle_hit" | cut -d/ -f4 | cut -d, -f1 | cut -d- -f2) ;;
  esac
  [ "$toggle_x2" -le "$rail_cols" ] ||
    fail "the rail's toggle ends at column $toggle_x2 of a $rail_cols-column rail; it cannot be clicked"
  reserve_cols=$(printf '%s' "$reserve" | cut -d' ' -f1)
  [ "$rail_cols" -ge "$reserve_cols" ] ||
    fail "rail_titlebar=widen left the rail at $rail_cols cols, under the $reserve_cols-column reserve"
  # `soft` runs this in a subshell, so the width the later rail checks want travels through a file.
  printf '%s' "$rail_cols" >"$home/rail_want"
  no_warnings "$mac_mark" "the macOS rail"
  vtest "$first_content" toggle
  sleep 2.5
  no_dupes "the macOS rail"
  echo "ok: the macOS rail is wide enough for the traffic lights and keeps its toggle inside"
}
if [ -n "${VTABS_E2E_MACOS:-}" ]; then
  soft macos_rail
  [ -s "$home/rail_want" ] && rail_want=$(cat "$home/rail_want")
  echo "  the rail is expected at $rail_want cols under the macOS reserve"
fi

# ---------------------------------------------------------------- item 5 ---
# 1. Ten `cli spawn` inside one poll: every spawn activates its tab, so the poll that lands
#    mid-burst attaches to a tab the next spawn has already replaced as active.
before=$(tab_count)
i=0
jobs=""
while [ "$i" -lt 10 ]; do
  i=$((i + 1))
  cli spawn --pane-id "$first_content" >/dev/null 2>&1 &
  jobs="$jobs $!"
done
# Bare `wait` would also wait for the wezterm this harness backgrounded.
for j in $jobs; do wait "$j" || true; done
for _ in $(seq 1 40); do
  [ "$(tab_count)" -ge $((before + 10)) ] && break
  sleep 0.25
done
[ "$(tab_count)" -ge $((before + 10)) ] || fail "the spawn burst only reached $(tab_count) tabs"
no_dupes_settled "a burst of 10 spawns" 10
echo "ok: 10 spawns inside one poll leave one sidebar per tab"

# The same again one at a time, asserting between each.
i=0
while [ "$i" -lt 4 ]; do
  i=$((i + 1))
  cli spawn --pane-id "$first_content" >/dev/null
  no_dupes "sequential spawn $i"
done
no_dupes_settled "4 sequential spawns" 10
echo "ok: sequential spawns leave one sidebar per tab"

# 2. Activating every tab drives the lazy attach; a tab that already has a pending sidebar must
#    not get a second one.
sweep_mark=$(mark)
for t in $(tab_ids); do
  cli activate-tab --tab-id "$t" >/dev/null
  wait_attached "$t" 12
done
no_dupes_settled "activating all tabs" 10
no_warnings "$sweep_mark" "the lazy attach sweep"
echo "ok: lazy attach gives every one of $(tab_count) tabs exactly one sidebar"

# Back down to a workable tab count.
keep=$(tab_ids | cut -d' ' -f1,2)
for t in $(tab_ids); do
  case " $keep " in
    *" $t "*) ;;
    *) cli kill-pane --pane-id "$(content_of "$t")" >/dev/null 2>&1 || true ;;
  esac
done
sleep 3
no_dupes_settled "closing the extra tabs" 8
first_tab=$(tab_ids | cut -d' ' -f1)
first_content=$(content_of "$first_tab")
cli activate-tab --tab-id "$first_tab" >/dev/null

# 3. `vtabs.action.new_tab` splits the sidebar itself, outside `ensure`'s per-window guard, right
#    after an awaiting `spawn_tab`; a poll landing in that await attaches to the same tab first.
i=0
while [ "$i" -lt 8 ]; do
  i=$((i + 1))
  vtest "$first_content" new_tab
  sleep 0.35
  no_dupes "action.new_tab $i"
done
no_dupes_settled "8 action.new_tab calls" 8
echo "ok: the new_tab action never doubles a sidebar"

# 4. hidden -> expand -> switch tabs, the sequence the report names.
hot=$(tab_ids | cut -d' ' -f1)
other=$(tab_ids | cut -d' ' -f2)
hot_content=$(content_of "$hot")
cli activate-tab --tab-id "$hot" >/dev/null
vtest "$hot_content" hidden_mode
sleep 0.5
vtest "$hot_content" toggle
sleep 2
no_dupes "collapse to hidden"
vtest "$hot_content" toggle
sleep 2
no_dupes "expand from hidden"
cli activate-tab --tab-id "$other" >/dev/null
sleep 1.5
no_dupes "switch tab after expand"
cli activate-tab --tab-id "$hot" >/dev/null
sleep 1.5
no_dupes_settled "switch back after expand" 8
echo "ok: hidden toggle and tab switches keep one sidebar per tab"

# 5. rail <-> hidden while collapsed: the mode changes under a collapsed window, so the expand
#    path has to reattach exactly the panes the collapse detached.
vtest "$hot_content" rail_mode
sleep 0.5
vtest "$hot_content" toggle
sleep 1.5
no_dupes "collapse to rail"
vtest "$hot_content" hidden_mode
sleep 1.5
no_dupes "rail -> hidden while collapsed"
vtest "$hot_content" toggle
sleep 2.5
no_dupes "expand after mode switch"
vtest "$hot_content" rail_mode
sleep 1
vtest "$hot_content" toggle
sleep 1.5
vtest "$hot_content" hidden_mode
sleep 1
vtest "$hot_content" toggle
sleep 2.5
no_dupes_settled "rail/hidden switching under a toggle" 8
echo "ok: switching rail and hidden across a toggle keeps one sidebar per tab"

# 6. `wezterm.reload_configuration()` rebuilds the Lua VM: every module cache is lost and only
#    `wezterm.GLOBAL` survives, so attach must not re-run for tabs that already have a sidebar.
before_reload=$(tab_count)
reload_mark=$(mark)
sb_before=$(sidebar_panes | sort -n | tr '\n' ' ')
# The tab may be mid-reattach after the mode dance, so the width is only checked when there is one.
width_before=$(width_of "$hot" 2>/dev/null || echo "")
vtest "$hot_content" reload
sleep 6
[ "$(tab_count)" -eq "$before_reload" ] || { geometry; fail "config reload changed the tab count"; }
no_dupes_settled "config reload" 8
# A reload rebuilds the Lua VM: the panes it left behind have to be recognised, not replaced.
[ "$(sidebar_panes | sort -n | tr '\n' ' ')" = "$sb_before" ] ||
  { geometry; fail "config reload replaced sidebar panes: $sb_before -> $(sidebar_panes | sort -n | tr '\n' ' ')"; }
[ -z "$width_before" ] || [ "$(width_of "$hot" 2>/dev/null || echo "")" = "$width_before" ] ||
  { widths; fail "config reload moved the sidebar from $width_before cols"; }
no_warnings "$reload_mark" "a config reload"
for t in $(tab_ids); do
  cli activate-tab --tab-id "$t" >/dev/null
  no_dupes "activate tab $t after reload"
done
no_dupes_settled "activating tabs after a reload" 8
echo "ok: a config reload keeps one sidebar per tab"

hot=$(tab_ids | cut -d' ' -f1)
hot_content=$(content_of "$hot")
cli activate-tab --tab-id "$hot" >/dev/null

# 7. Resizing while collapsed: the rail is our own width, so a resize must not be read as a drag
#    and must not make a second attach look necessary.
vtest "$hot_content" rail_mode
sleep 0.5
vtest "$hot_content" toggle
sleep 1.5
vtest "$hot_content" grow
sleep 2
no_dupes "grow while collapsed"
vtest "$hot_content" shrink
sleep 2
no_dupes "shrink while collapsed"
vtest "$hot_content" toggle
sleep 2
no_dupes_settled "expand after resizing while collapsed" 8
echo "ok: resizing a collapsed window keeps one sidebar per tab"

# 8. Tear-off: `move_to_new_window` awaits and the new window's own poll can attach first.
vtest "$hot_content" hidden_mode
sleep 0.5
torn=$(tab_ids | cut -d' ' -f2)
cli activate-tab --tab-id "$torn" >/dev/null
sleep 1
vtest "$(content_of "$torn")" tear_off
for _ in $(seq 1 20); do
  [ "$(window_count)" -ge 2 ] && break
  sleep 0.5
done
[ "$(window_count)" -ge 2 ] || { geometry; fail "tear_off did not open a second window"; }
no_dupes_settled "tear-off" 8
echo "ok: tear-off gives the moved tab exactly one sidebar"

if [ "$mode" = mux ]; then
  # 9. A GUI restart against a live mux: the surviving backend panes are adopted, and the lazy
  #    attach must not race the adoption into a second split.
  before_count=$(tab_count)
  kill $pid 2>/dev/null || true; wait $pid 2>/dev/null || true
  sleep 1
  HOME="$home" XDG_RUNTIME_DIR="$home/run" VTABS_ROOT="$root" VTABS_BIN="$bin" WEZTERM_LOG=info \
    wezterm --config-file "$root/plugin/tests/wezterm-e2e.lua" connect e2emux --class vtabs-e2e >>"$log" 2>&1 &
  pid=$!
  export WEZTERM_UNIX_SOCKET="$home/run/wezterm/gui-sock-$pid"
  for _ in $(seq 1 200); do
    [ -S "$WEZTERM_UNIX_SOCKET" ] && cli list >/dev/null 2>&1 && break
    sleep 0.1
  done
  cli list >/dev/null 2>&1 || fail "the reattached gui never answered"
  [ "$(tab_count)" -eq "$before_count" ] || { geometry; fail "the mux lost tabs across the restart"; }
  no_dupes_settled "gui restart" 12
  for t in $(tab_ids); do
    cli activate-tab --tab-id "$t" >/dev/null
    no_dupes "activate tab $t after the restart"
  done
  no_dupes_settled "lazy attach after a gui restart" 12
  echo "ok: a gui restart against the mux keeps one sidebar per tab"
  hot=$(tab_ids | cut -d' ' -f1)
  hot_content=$(content_of "$hot")
  cli activate-tab --tab-id "$hot" >/dev/null
  sleep 1
fi

# Width helpers every group below uses, so they must sit outside the fast-mode guard.
# What `correct` is aiming for right now, and the width the user dragged to. With a split content
# pane `fits` charges MIN_CONTENT per band, so the target is below `desired` and that is correct.
target_now() { probe_line "$1" probe_target target | sed -n 's/^\([0-9-]*\) .*/\1/p'; }
desired_now() { probe_line "$1" probe_target target | sed -n 's/^[0-9-]* want \([0-9-]*\) .*/\1/p'; }

settled_width() { # tab_id -> the width once three reads in a row agree, else the last one seen
  last=""; same=0
  for _ in $(seq 1 32); do
    now=$(width_of "$1" 2>/dev/null || echo "?")
    if [ "$now" = "$last" ]; then
      same=$((same + 1))
      [ "$same" -ge 2 ] && { echo "$now"; return 0; }
    else
      same=0
    fi
    last="$now"
    sleep 0.25
  done
  echo "$last"
}
want_width() { # tab_id want label — waits for the wanted width, then fails with what it settled on
  for _ in $(seq 1 40); do
    [ "$(width_of "$1" 2>/dev/null || echo '?')" = "$2" ] && return 0
    sleep 0.25
  done
  widths; geometry; fail "$3: sidebar settled at $(settled_width "$1") cols, want $2"
}

# `VTABS_STRESS_FAST=1` skips the width traces, which take most of a run, so the groups after
# them can be exercised on their own.
if [ -z "${VTABS_STRESS_FAST:-}" ]; then
  # ---------------------------------------------------------------- item 9 ---
  # Every width check waits for the sidebar to stop moving, then pins the number it settled on.

  vtest "$hot_content" rail_mode
  sleep 0.5
  same_window=$(busiest_window_tabs)
  hot=$(echo "$same_window" | cut -d' ' -f1)
  other=$(echo "$same_window" | cut -d' ' -f2)
  hot_content=$(content_of "$hot")
  cli activate-tab --tab-id "$hot" >/dev/null
  vtest "$(content_of "$hot")" rail_mode
  sleep 0.5
  cli activate-tab --tab-id "$hot" >/dev/null
  want_width "$hot" 28 "before the resize traces"
  trace "baseline $(total_cols) cols"

  # A. +30 / -30 cols while expanded.
  vtest "$hot_content" grow
  sleep 2.5
  trace "expanded, +300 px"
  want_width "$hot" 28 "grow while expanded"
  vtest "$hot_content" shrink
  sleep 2.5
  trace "expanded, back to the start"
  want_width "$hot" 28 "shrink while expanded"
  echo "ok: an expanded sidebar keeps its width across grow and shrink"

  # B. The same while collapsed to the rail.
  vtest "$hot_content" toggle
  sleep 2
  want_width "$hot" "$rail_want" "collapse to the rail"
  vtest "$hot_content" grow
  sleep 2.5
  trace "rail, +300 px"
  want_width "$hot" "$rail_want" "grow while railed"
  vtest "$hot_content" shrink
  sleep 2.5
  trace "rail, back to the start"
  want_width "$hot" "$rail_want" "shrink while railed"
  vtest "$hot_content" toggle
  sleep 2.5
  want_width "$hot" 28 "expand after resizing the rail"
  echo "ok: a railed sidebar keeps the rail width across grow and shrink"

  # C. Repeated resizes 100 ms apart: what a divider-free window drag actually sends.
  vtest "$hot_content" drag_shrink
  sleep 4
  trace "after a 10-step shrink drag"
  want_width "$hot" 28 "a shrink drag"
  vtest "$hot_content" drag_grow
  sleep 4
  trace "after a 10-step grow drag"
  want_width "$hot" 28 "a grow drag"
  echo "ok: ten resizes 100 ms apart leave the sidebar at its configured width"

  # D. The reported sequence: expand -> collapse -> change tab -> expand.
  vtest "$hot_content" toggle
  sleep 2
  want_width "$hot" "$rail_want" "collapse before the tab switch"
  cli activate-tab --tab-id "$other" >/dev/null
  sleep 2.5
  trace "collapsed, switched to tab $other"
  want_width "$other" "$rail_want" "a background tab joining the rail"
  vtest "$(content_of "$other")" toggle
  sleep 2.5
  trace "expanded on tab $other"
  want_width "$other" 28 "expand after switching tabs"
  cli activate-tab --tab-id "$hot" >/dev/null
  sleep 2.5
  trace "back on tab $hot"
  want_width "$hot" 28 "the first tab after expand on another tab"
  echo "ok: expand, collapse, switch tab, expand keeps both sidebars at 28"

  # E. collapse -> resize -> expand, so the expand target is computed at a width nobody observed.
  vtest "$(content_of "$hot")" toggle
  sleep 2
  want_width "$hot" "$rail_want" "collapse before the resize"
  vtest "$(content_of "$hot")" grow
  sleep 2.5
  vtest "$(content_of "$hot")" toggle
  sleep 2.5
  trace "expanded at the grown size"
  want_width "$hot" 28 "expand after a resize taken while collapsed"
  vtest "$(content_of "$hot")" shrink
  sleep 2.5
  want_width "$hot" 28 "shrink back after that expand"
  echo "ok: collapse, resize, expand restores the configured width"

  # F. rail -> activate a background tab (lazy attach) -> expand.
  vtest "$(content_of "$hot")" toggle
  sleep 2
  cli activate-tab --tab-id "$other" >/dev/null
  sleep 2.5
  want_width "$other" "$rail_want" "the background tab under the rail"
  vtest "$(content_of "$other")" toggle
  sleep 2.5
  want_width "$other" 28 "expanding on the lazily corrected tab"
  cli activate-tab --tab-id "$hot" >/dev/null
  sleep 2.5
  want_width "$hot" 28 "the other tab after that expand"
  no_dupes_settled "the width traces" 8
  echo "ok: rail, activate a background tab, expand leaves every sidebar at 28"

  # G. A divider drag is the one width the plugin must adopt and keep. `cli adjust-pane-size` moves
  #    the split exactly the way dragging the divider does, so this is the real gesture.
  same_window=$(busiest_window_tabs)
  hot=$(echo "$same_window" | cut -d' ' -f1)
  other=$(echo "$same_window" | cut -d' ' -f2)
  hot_content=$(content_of "$hot")
  cli activate-tab --tab-id "$hot" >/dev/null
  want_width "$hot" 28 "before the drag trace"
  drag_mark=$(mark)
  cli adjust-pane-size --pane-id "$(sidebar_of "$hot")" --amount 12 Right >/dev/null 2>&1 ||
    cli adjust-pane-size --pane-id "$(sidebar_of "$hot")" --amount 12 Left >/dev/null 2>&1 || true
  sleep 1
  dragged=$(settled_width "$hot")
  trace "after a 12-cell divider drag ($dragged cols)"
  [ "$dragged" != 28 ] || fail "adjust-pane-size did not move the divider; the drag trace cannot run"
  # Two polls plus geometry's settle window is what turns an observed width into the adopted one.
  sleep 3
  echo "  desired after the drag: $(probe_line "$hot_content" probe_desired "desired width")"
  want_width "$hot" "$dragged" "a divider drag"
  no_warnings "$drag_mark" "the divider drag"
  echo "ok: a divider drag to $dragged cols is adopted and held"

  vtest "$hot_content" grow
  sleep 3
  trace "dragged width, +300 px"
  want_width "$hot" "$dragged" "growing the window after a drag"
  vtest "$hot_content" shrink
  sleep 3
  trace "dragged width, back to the start"
  want_width "$hot" "$dragged" "shrinking the window after a drag"
  echo "ok: the dragged width survives a grow and a shrink"

  vtest "$hot_content" rail_mode
  sleep 0.5
  vtest "$hot_content" toggle
  sleep 2.5
  want_width "$hot" "$rail_want" "collapsing after a drag"
  vtest "$hot_content" toggle
  sleep 3
  trace "expanded again after the rail"
  want_width "$hot" "$dragged" "expanding back to the dragged width"
  cli activate-tab --tab-id "$other" >/dev/null
  sleep 3
  want_width "$other" "$dragged" "a second tab under the dragged width"
  cli activate-tab --tab-id "$hot" >/dev/null
  sleep 2
  no_dupes "the divider drag trace"
  echo "ok: rail and back, and a tab switch, all return to the dragged width"

  # Soft: a resize drag over split content currently ends on a width WezTerm dealt rather than the
  # adopted one, so this group pins an open bug.
  split_content_drag() {
    # H. A split content pane is the branch where `correct` activates the sidebar and restores focus
    #    on every single correction — including on every `window-resized` of a drag.
    split_mark=$(mark)
    extra=$(cli split-pane --pane-id "$hot_content" --right 2>/dev/null || echo "")
    if [ -n "$extra" ]; then
      max_panes=3
      sleep 2
      before_active=$(probe_line "$hot_content" probe_active "active pane")
      trace "content split in two"
      vtest "$hot_content" drag_shrink
      sleep 5
      trace "after a shrink drag with the content split"
      echo "  split-band target: $(probe_line "$hot_content" probe_target target)"
      want_width "$hot" "$(target_now "$hot_content")" "a resize drag over a split content pane"
      [ "$(desired_now "$hot_content")" = "$dragged" ] ||
        fail "the drag width was forgotten: desired is $(desired_now "$hot_content"), want $dragged"
      [ "$(list | python3 -c 'import json,sys; t='"$hot"'; print(sum(1 for p in json.load(sys.stdin) if p["tab_id"]==t))')" -eq 3 ] ||
        { geometry; fail "the resize drag lost a content pane"; }
      after_active=$(probe_line "$hot_content" probe_active "active pane")
      [ "$before_active" = "$after_active" ] ||
        fail "the resize drag moved focus from pane $before_active to $after_active"
      no_warnings "$split_mark" "a resize drag over a split content pane"
      vtest "$hot_content" drag_grow
      sleep 5
      echo "  split-band target: $(probe_line "$hot_content" probe_target target)"
      want_width "$hot" "$(target_now "$hot_content")" "a grow drag over a split content pane"
      [ "$(desired_now "$hot_content")" = "$dragged" ] ||
        fail "the drag width was forgotten: desired is $(desired_now "$hot_content"), want $dragged"
      no_dupes "the split-content drag"
      echo "ok: a resize drag over split content keeps the width, the panes and the focus"
      cli kill-pane --pane-id "$extra" >/dev/null 2>&1 || true
      sleep 3
      # One band again: the clamp lifts and the dragged width has to come back in full.
      want_width "$hot" "$dragged" "closing the extra content band"
      max_panes=2
    fi
  }
  soft split_content_drag

  # I. The rail in a private window: `render` takes its private branch there, and a throw leaves the
  #    pane showing its last frame, which reads exactly like "expand -> collapse acts weird".
  priv_mark=$(mark)
  before_windows=$(window_count)
  known_tabs=" $(tab_ids) "
  vtest "$hot_content" private_window
  for _ in $(seq 1 24); do
    [ "$(window_count)" -gt "$before_windows" ] && break
    sleep 0.5
  done
  if [ "$(window_count)" -gt "$before_windows" ]; then
    priv_tab=""
    for t in $(tab_ids); do
      case "$known_tabs" in
        *" $t "*) ;;
        *) priv_tab=$t ;;
      esac
    done
    if [ -n "$priv_tab" ]; then
      wait_attached "$priv_tab" 12
      priv_content=$(content_of "$priv_tab")
      priv_sb=$(sidebar_of "$priv_tab")
      renders "$priv_sb" "a private window before the rail"
      vtest "$priv_content" rail_mode
      sleep 0.5
      vtest "$priv_content" toggle
      sleep 2.5
      renders "$priv_sb" "a private window under the rail"
      no_warnings "$priv_mark" "a private window under the rail"
      vtest "$priv_content" toggle
      sleep 2.5
      renders "$priv_sb" "a private window expanded again"
      no_warnings "$priv_mark" "expanding a private window from the rail"
      no_dupes "the private window rail trace"
      echo "ok: a private window renders under the rail and after expanding"
      cli kill-pane --pane-id "$priv_content" >/dev/null 2>&1 || true
      sleep 2
    fi
  fi

  # J. The rail with a footer hook: the footer adds rows the rail grid has to place too.
  foot_mark=$(mark)
  hot=$(tab_ids | cut -d' ' -f1)
  hot_content=$(content_of "$hot")
  hot_sb=$(sidebar_of "$hot")
  cli activate-tab --tab-id "$hot" >/dev/null
  vtest "$hot_content" footer_hook
  sleep 2
  renders "$hot_sb" "an expanded sidebar with a footer hook"
  vtest "$hot_content" toggle
  sleep 2.5
  renders "$hot_sb" "a railed sidebar with a footer hook"
  no_warnings "$foot_mark" "a railed sidebar with a footer hook"
  vtest "$hot_content" toggle
  sleep 2.5
  renders "$hot_sb" "an expanded sidebar with a footer hook"
  no_warnings "$foot_mark" "expanding a sidebar with a footer hook"
  not_frozen "$hot_sb" "$hot" "the footer-hook trace"
  vtest "$hot_content" no_footer_hook
  sleep 1.5
  no_dupes "the footer-hook trace"
  echo "ok: the rail renders with a footer hook and keeps repainting"


fi

# ------------------------------------------------- reports A, B and C -----
# A. "1 -> 3 via the shortcut is laggy and passes through tab 2". The shortcut is
#    `actions.action.activate_tab(n)` -> `activate_index` -> `model.ordered(model.build(...))`,
#    which reads a title, an icon, unseen output and a cwd for EVERY tab just to turn an index
#    into a tab id. `watch_active` samples the active tab at 10 ms so a pass-through is visible.
same_window=$(busiest_window_tabs)
first=$(echo "$same_window" | cut -d' ' -f1)
third=$(echo "$same_window" | cut -d' ' -f3)
[ -n "$third" ] || third=$(echo "$same_window" | cut -d' ' -f2)
cli activate-tab --tab-id "$first" >/dev/null
sleep 1
first_content=$(content_of "$first")
watch_mark=$(mark)
vtest "$first_content" watch_active
sleep 0.3
switch=$(probe_line "$first_content" activate_2 "activate 2")
sleep 2.5
trace_line=$(since "$watch_mark" | sed -n 's/.*e2e: active trace //p' | tail -1)
echo "  switch cost: $switch"
echo "  active trace: $trace_line"
[ -n "$switch" ] || fail "the activate probe never answered"
build_ms=$(printf '%s' "$switch" | sed -n 's/.*build \([0-9]*\).*/\1/p')
act_ms=$(printf '%s' "$switch" | sed -n 's/.*activate \([0-9]*\).*/\1/p')
[ "${build_ms:-0}" -lt 300 ] ||
  fail "resolving a tab index cost ${build_ms}ms of model.build before anything was activated"
[ "${act_ms:-0}" -lt 400 ] || fail "activating a tab by index took ${act_ms}ms"
# One transition, or two when the sampler starts before the switch: never three.
hops=$(printf '%s' "$trace_line" | wc -w | tr -d ' ')
[ "${hops:-0}" -le 2 ] ||
  fail "switching by index passed through $((hops - 1)) tabs: $trace_line"
no_dupes "a tab switch by index"
echo "ok: a tab switch by index costs ${build_ms}ms of lookup and lands on one tab"

divider_drag() {
  # B. "mouse resizing of the sidebar glitches". A divider drag never fires `window-resized`; the
  #    poll only sees the sidebar's own column count change. Every `AdjustPaneSize` the plugin
  #    issues is logged by the wrapper in wezterm-e2e.lua, so the fight is countable.
  cli activate-tab --tab-id "$first" >/dev/null
  sleep 2
  # The drag trace above adopts a width, so the baseline is whatever the sidebar sits at now.
  base=$(settled_width "$first")
  drag_mark=$(mark)
  drag_sb=$(sidebar_of "$first")
  for step in 1 2 3 4; do
    cli adjust-pane-size --pane-id "$drag_sb" --amount 3 Right >/dev/null 2>&1 || true
    sleep 0.1
  done
  sleep 0.4
  mid=$(width_of "$first")
  adjusts=$(since "$drag_mark" | grep -c "e2e: adjust at" || true)
  echo "  divider drag: from $base cols, 4 steps of +3 -> $mid cols, plugin issued $adjusts adjusts"
  # The plugin must not fight a drag it can see: it may adopt the new width, never undo it.
  [ "$mid" -ge $((base + 6)) ] ||
    fail "the plugin pulled the divider back mid-drag: 12 cols dragged from $base, sidebar is $mid"
  sleep 3
  settled=$(settled_width "$first")
  echo "  divider drag settled at $settled cols"
  [ "$settled" -ge $((base + 6)) ] ||
    fail "the dragged width was undone after the drag: $base -> $settled cols"
  no_dupes "a divider drag"
  echo "ok: a four-step divider drag is adopted, not fought ($adjusts adjusts)"
  # Hand the width back so the checks after this one start where they started.
  cli adjust-pane-size --pane-id "$(sidebar_of "$first")" --amount $((settled - base)) Left >/dev/null 2>&1 || true
  sleep 3
}
soft divider_drag

# C. A user's own split keybinding while the sidebar holds focus. WezTerm splits whichever pane is
#    active, so the shell lands in the sidebar's column; `sidebar.rescue_splits` has to move it to
#    the content side, leave the sidebar's width and identity alone, and rank the moved pane as
#    nothing. Panes are addressed by the columns they occupy, which is the only way to say
#    "inside the sidebar's strip".
in_sidebar_columns() { # tab_id
  list | python3 -c '
import json,sys
t='"$1"'
panes=[p for p in json.load(sys.stdin) if p["tab_id"]==t]
sb=[p for p in panes if p["title"].startswith("wez-vtabs")]
if not sb: print(""); raise SystemExit
l=sb[0]["left_col"]; r=l+sb[0]["size"]["cols"]
print(" ".join(str(p["pane_id"]) for p in panes
                if p["pane_id"]!=sb[0]["pane_id"] and p["left_col"]>=l and p["left_col"]+p["size"]["cols"]<=r))'
}
lowest_content() { # tab_id -> the content pane furthest down the tab
  list | python3 -c '
import json,sys
t='"$1"'
panes=[p for p in json.load(sys.stdin) if p["tab_id"]==t]
sb=[p for p in panes if p["title"].startswith("wez-vtabs")]
rest=sorted((p for p in panes if not sb or p["pane_id"]!=sb[0]["pane_id"]), key=lambda p: p["top_row"])
print(rest[-1]["pane_id"] if len(rest)>1 else "")'
}
panes_in() { list | python3 -c 'import json,sys; t='"$1"'; print(sum(1 for p in json.load(sys.stdin) if p["tab_id"]==t))'; }

# One rescue, asserted end to end. $1 is the probe that performs the split.
rescue_case() {
  split_mark=$(mark)
  cli activate-tab --tab-id "$first" >/dev/null
  sleep 1
  before_sb=$(sidebar_of "$first")
  # The rescue refuses a sidebar that has not echoed its token, and correctly does nothing. Splitting
  # before then would read as a miss, so wait for ready rather than for the marker title.
  n=0
  while [ "$n" -lt 40 ]; do
    case "$(probe_line "$first_content" probe_ranks ranks)" in
      *"$before_sb:backend=true,ready=true"*) break ;;
    esac
    n=$((n + 1)); sleep 0.5
  done
  [ "$n" -lt 40 ] || { geometry; fail "sidebar $before_sb never authenticated before $1"; }
  before_width=$(settled_width "$first")
  before_panes=$(panes_in "$first")
  echo "  before $1: $(probe_line "$first_content" probe_ranks ranks)"
  vtest "$first_content" "$1"
  sleep 4
  max_panes=$((before_panes + 1))
  echo "  after the split: $(probe_line "$first_content" probe_tree tree)"
  [ "$(panes_in "$first")" -eq "$max_panes" ] ||
    { geometry; fail "the split did not add exactly one pane ($before_panes -> $(panes_in "$first"))"; }
  # The rescue runs on a poll, so give it a few before deciding it did not happen.
  for _ in $(seq 1 24); do
    [ -z "$(in_sidebar_columns "$first")" ] && break
    sleep 0.5
  done
  stuck=$(in_sidebar_columns "$first")
  [ -z "$stuck" ] || { geometry; fail "pane(s) $stuck are still inside the sidebar's columns"; }
  echo "  rescued: $(probe_line "$first_content" probe_tree tree)"
  moved=$(lowest_content "$first")
  [ -n "$moved" ] || { geometry; fail "the split pane did not end up beside the content pane"; }
  [ "$(sidebar_of "$first")" = "$before_sb" ] ||
    { geometry; fail "the rescue changed the sidebar pane from $before_sb to $(sidebar_of "$first")"; }
  ranks=$(probe_line "$first_content" probe_ranks ranks)
  echo "  ranks after the rescue: $ranks"
  case "$ranks" in
    *"$before_sb:backend=true,ready=true"*) ;;
    *) fail "the sidebar $before_sb is no longer a ready backend: $ranks" ;;
  esac
  case "$ranks" in
    *"$moved:backend=true"*) fail "the moved pane $moved is ranked as a backend: $ranks" ;;
  esac
  want_width "$first" "$before_width" "after the split was rescued"
  renders "$before_sb" "a rescued split"
  not_frozen "$before_sb" "$first" "a rescued split"
  no_warnings "$split_mark" "rescuing a split off the sidebar"
  no_dupes "the split rescue"
  echo "ok: $1 is moved to the content side, sidebar intact at $before_width cols"
}

split_net() {
  # Vertical halves the sidebar's rows and stays inside its columns; horizontal halves its columns
  # and the new pane reaches past them. Only the second exercises the edge test.
  rescue_case split_sidebar
  restore_split_panes
  rescue_case split_sidebar_h
  restore_split_panes

  # `vtabs.action.split` targets the content pane even with the sidebar focused, so nothing has to
  # be rescued at all.
  act_mark=$(mark)
  act_before=$(panes_in "$first")
  max_panes=$((act_before + 1))
  echo "  $(probe_line "$first_content" action_split_bottom "action split from")"
  sleep 3
  [ "$(panes_in "$first")" -eq "$max_panes" ] ||
    { geometry; fail "vtabs.action.split did not add a pane ($act_before -> $(panes_in "$first"))"; }
  [ -z "$(in_sidebar_columns "$first")" ] ||
    { geometry; fail "vtabs.action.split put a pane in the sidebar's columns: $(in_sidebar_columns "$first")"; }
  [ "$(sidebar_of "$first")" = "$before_sb" ] || { geometry; fail "vtabs.action.split moved the sidebar"; }
  want_width "$first" "$before_width" "after vtabs.action.split"
  no_warnings "$act_mark" "vtabs.action.split from the sidebar"
  no_dupes "vtabs.action.split"
  echo "ok: vtabs.action.split targets the content pane with the sidebar focused"

  # "Down" is an alias of "Bottom": it splits, below the content, never in the sidebar's columns.
  down_mark=$(mark)
  down_before=$(panes_in "$first")
  max_panes=$((down_before + 1))
  echo "  $(probe_line "$first_content" action_split_down "split down panes")"
  sleep 3
  [ "$(panes_in "$first")" -eq "$max_panes" ] ||
    { geometry; fail "split \"Down\" did not add a pane ($down_before -> $(panes_in "$first"))"; }
  [ -z "$(in_sidebar_columns "$first")" ] ||
    { geometry; fail "split \"Down\" put a pane in the sidebar's columns: $(in_sidebar_columns "$first")"; }
  [ "$(sidebar_of "$first")" = "$before_sb" ] || { geometry; fail "split \"Down\" moved the sidebar"; }
  no_warnings "$down_mark" "split \"Down\""
  echo "ok: split \"Down\" is an alias of \"Bottom\" and lands beside the content"

  # A direction the vocabulary does not name warns and splits nothing.
  bogus_mark=$(mark)
  bogus_before=$(panes_in "$first")
  echo "  $(probe_line "$first_content" action_split_bogus "split bogus panes")"
  sleep 2
  [ "$(panes_in "$first")" -eq "$bogus_before" ] ||
    { geometry; fail "an unknown split direction added a pane ($bogus_before -> $(panes_in "$first"))"; }
  bogus_warn=$(vtabs_warnings "$bogus_mark")
  case "$bogus_warn" in
    *"split direction"*) echo "ok: an unknown split direction warns instead of splitting" ;;
    *) fail "an unknown split direction neither split nor warned: $bogus_warn" ;;
  esac

}

# Outside the group on purpose: `soft` runs it in a subshell, so a failed assertion skips the rest
# of the group and would otherwise strand its extra panes in every later pane-count assertion.
restore_split_panes() {
  # One at a time and re-read between kills: a pane that is still starting does not always die on
  # the first ask, and the ids shift as the tab relayouts.
  n=0
  while [ "$n" -lt 8 ] && [ "$(panes_in "$first")" -gt 2 ]; do
    n=$((n + 1))
    victim=$(lowest_content "$first")
    [ -n "$victim" ] || break
    cli kill-pane --pane-id "$victim" >/dev/null 2>&1 || true
    sleep 1.5
  done
  left=$(panes_in "$first")
  [ "$left" -le 2 ] || { geometry; echo "  warn: tab $first still holds $left panes after cleanup"; }
  max_panes=2
  no_dupes_settled "closing the rescued splits" 8
}

soft split_net
restore_split_panes

# Both groups below pin bugs that are still open; `VTABS_STRESS_SOFT=1` prints XFAIL for them
# and lets the run continue.
close_confirmation() {
  # --------------------------------------------------- close confirmation ---
  # A card's x on a tab whose shell would prompt: the question belongs in the sidebar, and no
  # WezTerm overlay pane may ever be created for it.
  vtest "$hot_content" hidden_mode
  sleep 0.5
  vtest "$hot_content" confirm_on
  sleep 0.5
  victim=$(tab_ids | cut -d' ' -f2)
  wait_attached "$victim" 12
  cli set-tab-title --tab-id "$victim" victim
  cli send-text --no-paste --pane-id "$(content_of "$victim")" "sleep 1000
  "
  sleep 2
  cli activate-tab --tab-id "$victim" >/dev/null
  sleep 1.5
  victim_sb=$(sidebar_of "$victim")
  frame_shows "$victim_sb" victim 8 || { sidebar_text "$victim_sb" | head -20; fail "the sidebar never listed the victim tab"; }
  victim_row=$(row_of "$victim_sb" victim)
  pane_set() { list | python3 -c 'import json,sys; print(" ".join(sorted("%d/%d"%(p["tab_id"],p["pane_id"]) for p in json.load(sys.stdin))))'; }
  before_set=$(pane_set)
  before_tabs=$(tab_count)
  survivor=$(tab_ids | cut -d' ' -f1)
  survivor_sb=$(sidebar_of "$survivor")
  survivor_content=$(content_of "$survivor")
  survivor_cols=$(cols_of "$survivor_content")
  echo "  $(probe_line "$survivor_content" probe_confirm confirm)"

  # The close span moves with the card grid, so it is read from the live hit map, never guessed.
  victim_hits=$(probe_line "$survivor_content" probe_hits hits)
  close_col=$(printf '%s\n' "$victim_hits" | tr ' ' '\n' | grep ',close@' | head -1 |
    sed -n 's/.*,close@[0-9]*-\([0-9]*\).*/\1/p')
  [ -n "$close_col" ] || { echo "$victim_hits"; fail "the victim card has no close span"; }
  echo "  close span ends at column $close_col"
  press "$victim_sb" "$close_col" "$victim_row" 0
  sleep 1
  [ "$(tab_count)" -eq "$before_tabs" ] || { geometry; fail "the press on x closed the tab before the release"; }
  [ "$(pane_set)" = "$before_set" ] || { geometry; fail "the press on x created a pane"; }
  release "$victim_sb" "$close_col" "$victim_row" 0
  sleep 1.5
  [ "$(tab_count)" -eq "$before_tabs" ] || { geometry; fail "x closed a confirmable tab without asking"; }
  [ "$(pane_set)" = "$before_set" ] || { geometry; fail "the confirmation created a WezTerm overlay pane"; }
  level=$(probe_line "$survivor_content" popover_level "popover level")
  case "$level" in
    confirm*) ;;
    *) sidebar_text "$victim_sb" | head -20; fail "x did not open the confirm level (level: $level)" ;;
  esac
  sidebar_text "$victim_sb" | grep -q "Cancel" || { sidebar_text "$victim_sb" | head -20; fail "the confirm level has no Cancel row"; }
  echo "ok: x on a confirmable tab asks inside the sidebar and creates no overlay pane"

  # Cancel first: nothing may change.
  cancel_row=$(row_of "$victim_sb" "Cancel")
  click "$victim_sb" 6 "$cancel_row" 0
  sleep 1.5
  [ "$(tab_count)" -eq "$before_tabs" ] || { geometry; fail "Cancel closed the tab"; }
  [ "$(pane_set)" = "$before_set" ] || { geometry; fail "Cancel changed the pane set"; }
  [ "$(probe_line "$survivor_content" popover_level "popover level")" = none ] || fail "Cancel left the popover open"
  echo "ok: Cancel leaves the tab and every pane untouched"

  # Then the affirmative: the row above Cancel.
  press "$victim_sb" "$close_col" "$victim_row" 0
  sleep 0.5
  release "$victim_sb" "$close_col" "$victim_row" 0
  sleep 1.5
  cancel_row=$(row_of "$victim_sb" "Cancel")
  click "$victim_sb" 6 $((cancel_row - 1)) 0
  for _ in $(seq 1 20); do
    [ "$(tab_count)" -lt "$before_tabs" ] && break
    sleep 0.5
  done
  [ "$(tab_count)" -eq $((before_tabs - 1)) ] || { geometry; fail "the confirmation did not close the tab"; }
  [ "$(sidebar_of "$survivor")" = "$survivor_sb" ] || { geometry; fail "the confirmation disturbed the other tab's sidebar"; }
  [ "$(content_of "$survivor")" = "$survivor_content" ] || { geometry; fail "the confirmation disturbed the other tab's content"; }
  [ "$(cols_of "$survivor_content")" -eq "$survivor_cols" ] || { geometry; fail "the confirmation resized the other tab"; }
  no_dupes "the close confirmation"
  vtest "$survivor_content" confirm_off
  sleep 0.5
  echo "ok: confirming closes only the asked-about tab"
}
soft close_confirmation

deterministic_pins() {
  # ------------------------------------------------ deterministic pins ------
  # Two polls inside one mux lag: `correct` reads the same stale `cols` twice and issues the same
  # AdjustPaneSize twice, and the double-applied overshoot is what the drag heuristic then adopts.
  hot=$(busiest_window_tabs | cut -d' ' -f1)
  cli activate-tab --tab-id "$hot" >/dev/null
  sleep 1
  vtest "$(content_of "$hot")" grow
  sleep 1
  echo "  two corrects on one stale width: $(probe_line "$(content_of "$hot")" double_correct "double correct")"
  case "$(probe_line "$(content_of "$hot")" double_correct "double correct")" in
    *"true true"*) fail "correct re-issued an AdjustPaneSize that was still in flight" ;;
  esac
  vtest "$(content_of "$hot")" shrink
  sleep 2
  echo "ok: a second correct does not re-issue an adjust that is still in flight"

  # Last, because they are expected to fail until the fix lands: everything above is timing, this
  # is the invariant itself.
  hot=$(tab_ids | cut -d' ' -f1)
  cli activate-tab --tab-id "$hot" >/dev/null
  sleep 1
  echo "  attach on an attached tab: $(probe_line "$(content_of "$hot")" double_attach "double attach")"
  sleep 2
  no_dupes "attach on a tab that already has a sidebar"
  echo "ok: attach is refused on a tab that already has a sidebar"
}
soft deterministic_pins

echo "all stress checks passed"
