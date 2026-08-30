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
for t in $(tab_ids); do
  cli activate-tab --tab-id "$t" >/dev/null
  no_dupes "activate tab $t"
done
no_dupes_settled "activating all tabs" 8
for t in $(tab_ids); do
  n=$(list | python3 -c 'import json,sys; t='"$t"'; print(sum(1 for p in json.load(sys.stdin) if p["tab_id"]==t and '"$is_marked"'))')
  [ "$n" -eq 1 ] || { geometry; fail "tab $t has $n sidebars after the lazy attach sweep"; }
done
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
vtest "$hot_content" reload
sleep 4
[ "$(tab_count)" -eq "$before_reload" ] || { geometry; fail "config reload changed the tab count"; }
no_dupes_settled "config reload" 8
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

# ---------------------------------------------------------------- item 9 ---
# Every width check waits for the sidebar to stop moving, then pins the number it settled on.
settled_width() { # tab_id -> prints the width once two reads agree, or after 6 s
  last=""
  for _ in $(seq 1 24); do
    now=$(width_of "$1" 2>/dev/null || echo "?")
    [ "$now" = "$last" ] && { echo "$now"; return 0; }
    last="$now"
    sleep 0.25
  done
  echo "$last"
}
want_width() { # tab_id want label
  got=$(settled_width "$1")
  [ "$got" = "$2" ] || { widths; geometry; fail "$3: sidebar is $got cols, want $2"; }
}

vtest "$hot_content" rail_mode
sleep 0.5
hot=$(tab_ids | cut -d' ' -f1)
other=$(tab_ids | cut -d' ' -f2)
hot_content=$(content_of "$hot")
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
want_width "$hot" 5 "collapse to the rail"
vtest "$hot_content" grow
sleep 2.5
trace "rail, +300 px"
want_width "$hot" 5 "grow while railed"
vtest "$hot_content" shrink
sleep 2.5
trace "rail, back to the start"
want_width "$hot" 5 "shrink while railed"
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
want_width "$hot" 5 "collapse before the tab switch"
cli activate-tab --tab-id "$other" >/dev/null
sleep 2.5
trace "collapsed, switched to tab $other"
want_width "$other" 5 "a background tab joining the rail"
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
want_width "$hot" 5 "collapse before the resize"
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
want_width "$other" 5 "the background tab under the rail"
vtest "$(content_of "$other")" toggle
sleep 2.5
want_width "$other" 28 "expanding on the lazily corrected tab"
cli activate-tab --tab-id "$hot" >/dev/null
sleep 2.5
want_width "$hot" 28 "the other tab after that expand"
no_dupes_settled "the width traces" 8
echo "ok: rail, activate a background tab, expand leaves every sidebar at 28"

echo "all stress checks passed"
