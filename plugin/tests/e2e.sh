#!/bin/sh
# Drives a throwaway WezTerm instance and exercises the sidebar through the CLI.
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
bin="${VTABS_BIN:-}"
if [ -z "$bin" ]; then
  # No override: build, so the run tests the working tree and not a stale binary.
  (cd "$root/backend" && cargo build --locked --release) || { echo "backend build failed"; exit 1; }
  bin="$root/backend/target/release/wez-vtabs"
fi
[ -x "$bin" ] || { echo "backend binary not found: $bin"; exit 1; }
log=$(mktemp -t vtabs-e2e.XXXXXX)

mode="${1:-local}"
home=$(mktemp -d /tmp/vte2e.XXXXXX)
if [ "$mode" = mux ]; then
  export VTABS_E2E_MUX="$home/mux.sock"
  launch="connect e2emux"
else
  launch="start --always-new-process"
fi
echo "mode: $mode"
# Isolated HOME keeps sockets and the mux pid file away from the real wezterm.
# shellcheck disable=SC2086
mkdir -p "$home/run"
HOME="$home" XDG_RUNTIME_DIR="$home/run" VTABS_ROOT="$root" VTABS_BIN="$bin" VTABS_E2E_COLLAPSED=hidden WEZTERM_LOG=info wezterm --config-file "$root/plugin/tests/wezterm-e2e.lua" \
  $launch --class vtabs-e2e >"$log" 2>&1 &
pid=$!
cleanup() {
  set +e
  kill $pid 2>/dev/null || true; wait $pid 2>/dev/null || true
  if [ "$mode" = mux ]; then
    sleep 1
    for p in $(pgrep -x wezterm-mux-server); do
      if ps -p "$p" -E -o command= | grep -c "$home" >/dev/null; then
        kill "$p" || true
      fi
    done
  fi
  [ -n "${VTABS_E2E_LOG:-}" ] && cp "$log" "$VTABS_E2E_LOG"
  rm -rf "$home"
  grep -i "vtabs: \(user-var\|update-status\|window-\)\|WARN\|ERROR" "$log" | grep -v "Broken pipe" | tail -20
  rm -f "$log"
}
trap cleanup EXIT

export WEZTERM_UNIX_SOCKET="$home/run/wezterm/gui-sock-$pid"
for _ in $(seq 1 50); do
  [ -S "$WEZTERM_UNIX_SOCKET" ] && wezterm cli --no-auto-start list >/dev/null 2>&1 && break
  sleep 0.2
done
sleep 2

fail() { echo "FAIL: $*"; exit 1; }
cli() { wezterm cli --no-auto-start "$@"; }
mark() { wc -l <"$log" | tr -d ' '; }
since() { tail -n "+$(($1 + 1))" "$log"; }
# Runs a probe defined in wezterm-e2e.lua by making the pane print an OSC 1337 SetUserVar.
vtest() { cli send-text --no-paste --pane-id "$1" "printf '\\033]1337;SetUserVar=vtabs_test=$(printf %s "$2" | base64)\\a'
"; }
list() { cli list --format json; }
is_sb='(p["title"].startswith("wez-vtabs") or (p["left_col"]==0 and p["size"]["cols"]==28))'
sidebar_panes() { list | python3 -c 'import json,sys; print("\n".join(str(p["pane_id"]) for p in json.load(sys.stdin) if '"$is_sb"'))'; }
sidebar_of() { list | python3 -c 'import json,sys; t='"$1"'; print([p["pane_id"] for p in json.load(sys.stdin) if p["tab_id"]==t and '"$is_sb"'][0])'; }
content_of() { list | python3 -c 'import json,sys; t='"$1"'; print([p["pane_id"] for p in json.load(sys.stdin) if p["tab_id"]==t and not '"$is_sb"'][0])'; }
tab_ids() { list | python3 -c 'import json,sys; print(" ".join(str(t) for t in sorted({p["tab_id"] for p in json.load(sys.stdin)})))'; }
tab_count() { tab_ids | wc -w | tr -d ' '; }
sidebar_text() { cli get-text --pane-id "$1"; }
# Reads the active tab title from the plugin side; the card's chrome is theme-dependent.
probe_line() {
  m=$(mark)
  vtest "$1" "$2"
  for _ in $(seq 1 25); do
    v=$(since "$m" | sed -n "s/.*e2e: $3 //p" | tail -1)
    [ -n "$v" ] && { echo "$v"; return 0; }
    sleep 0.2
  done
  echo ""
}
tab_of_pane() { list | python3 -c 'import json,sys; s='"$1"'; print([p["tab_id"] for p in json.load(sys.stdin) if p["pane_id"]==s][0])'; }
active_title() { probe_line "$(content_of "$(tab_of_pane "$1")")" probe_active_title "active title"; }
row_of() { sidebar_text "$1" | python3 -c 'import sys; rows=sys.stdin.read().split("\n"); print(next(i+1 for i,l in enumerate(rows) if "'"$2"'" in l))'; }
click() { cli send-text --no-paste --pane-id "$1" "$(printf '\033[<%s;%s;%sM\033[<%s;%s;%sm' "$4" "$2" "$3" "$4" "$2" "$3")"; }
geometry() { list | python3 -c 'import json,sys; [print("  win", p["window_id"], "tab", p["tab_id"], "pane", p["pane_id"], p["title"], "left", p["left_col"], "cols", p["size"]["cols"]) for p in json.load(sys.stdin)]'; }

for _ in $(seq 1 40); do
  [ -n "$(sidebar_panes 2>/dev/null)" ] && break
  sleep 0.5
done
sb1=$(sidebar_panes | head -1)
[ -n "$sb1" ] || fail "no sidebar pane after startup"
first_tab=$(list | python3 -c 'import json,sys; s='"$sb1"'; print([p["tab_id"] for p in json.load(sys.stdin) if p["pane_id"]==s][0])')
sleep 1
echo "ok: sidebar pane $sb1 present"
cli set-tab-title --tab-id "$first_tab" one

cli spawn --pane-id "$(content_of "$first_tab")" >/dev/null
sleep 1.5
[ "$(sidebar_panes | wc -l | tr -d ' ')" -eq 2 ] || fail "second tab did not get a sidebar"
echo "ok: sidebar attached to spawned tab"
list | python3 -c 'import json,sys; [print("  win", p["window_id"], "tab", p["tab_id"], "pane", p["pane_id"], p["title"], "left", p["left_col"], "cols", p["size"]["cols"]) for p in json.load(sys.stdin)]'
second_tab=$(tab_ids | cut -d' ' -f2)
cli set-tab-title --tab-id "$second_tab" two
sleep 1
sb2=$(sidebar_of "$second_tab")
sidebar_text "$sb1" | grep -c "one" >/dev/null || fail "first sidebar does not list tab one"
sidebar_text "$sb1" | grep -c "two" >/dev/null || fail "first sidebar does not list tab two"
echo "ok: both sidebars render both tabs"

width_of() { list | python3 -c 'import json,sys; t='"$1"'; print([p["size"]["cols"] for p in json.load(sys.stdin) if p["tab_id"]==t and '"$is_sb"'][0])'; }
settle_width() { # tab_id
  for _ in $(seq 1 24); do
    [ "$(width_of "$1")" -eq 28 ] && return 0
    sleep 0.25
  done
  return 1
}
cols_of() { list | python3 -c 'import json,sys; p='"$1"'; print([q["size"]["cols"] for q in json.load(sys.stdin) if q["pane_id"]==p][0])'; }
toggle() { vtest "$1" toggle; }
# Detach/attach contract; the harness default is the rail, so ask for hidden explicitly.
vtest "$(content_of "$first_tab")" hidden_mode
sleep 0.5
toggle "$(content_of "$first_tab")"
sleep 1.5
[ -z "$(sidebar_panes)" ] || fail "toggle did not remove sidebars"
[ "$(tab_count)" -eq 2 ] || fail "toggle closed a tab"
toggle "$(content_of "$first_tab")"
sleep 2.5
# expand splits the active tab only; the other one attaches when it is next activated
[ "$(sidebar_panes | wc -l | tr -d ' ')" -eq 1 ] || fail "expand split more than the active tab"
cli activate-tab --tab-id "$second_tab"
sleep 1.5
cli activate-tab --tab-id "$first_tab"
sleep 1.5
[ "$(sidebar_panes | wc -l | tr -d ' ')" -eq 2 ] || fail "toggle did not restore sidebars"
sb1=$(sidebar_of "$first_tab")
sb2=$(sidebar_of "$second_tab")
sleep 1
sidebar_text "$sb1" | grep -c "one" >/dev/null || fail "restored sidebar does not render"
echo "ok: toggle hides and restores sidebars without touching content"

# collapsed = "rail" is the shipped default: the pane stays and only narrows. Background tabs
# follow when they are next activated (P0 0.3's lazy width correction).
vtest "$(content_of "$first_tab")" rail_mode
sleep 0.5
toggle "$(content_of "$first_tab")"
for _ in $(seq 1 20); do
  [ "$(width_of "$first_tab")" -eq 5 ] && break
  sleep 0.25
done
[ "$(width_of "$first_tab")" -eq 5 ] || fail "the rail did not narrow the sidebar ($(width_of "$first_tab") cols)"
[ "$(sidebar_panes | wc -l | tr -d ' ')" -eq 2 ] || fail "the rail dropped a sidebar pane"
[ "$(tab_count)" -eq 2 ] || fail "the rail closed a tab"
[ "$(cols_of "$(content_of "$first_tab")")" -gt 90 ] || fail "the content pane did not take the freed columns"
cli activate-tab --tab-id "$second_tab"
for _ in $(seq 1 20); do
  [ "$(width_of "$second_tab")" -eq 5 ] && break
  sleep 0.25
done
[ "$(width_of "$second_tab")" -eq 5 ] || fail "a background tab did not join the rail once activated"
cli activate-tab --tab-id "$first_tab"
toggle "$(content_of "$first_tab")"
settle_width "$first_tab" || fail "the rail did not restore the full width ($(width_of "$first_tab") cols)"
cli activate-tab --tab-id "$second_tab"
settle_width "$second_tab" || fail "a background tab did not leave the rail once activated"
cli activate-tab --tab-id "$first_tab"
vtest "$(content_of "$first_tab")" hidden_mode
sleep 0.5
echo "ok: the rail narrows every sidebar and restores them"


click "$sb2" 5 "$(row_of "$sb2" two)" 0
sleep 1
[ "$(active_title "$sb2")" = "two" ] || fail "click on 'two' did not activate it (active: $(active_title "$sb2"))"
click "$sb2" 5 "$(row_of "$sb2" one)" 0
sleep 1
[ "$(active_title "$sb1")" = "one" ] || fail "click on 'one' did not activate it (active: $(active_title "$sb1"))"
echo "ok: left click switches tabs"

click "$sb1" 5 "$(row_of "$sb1" two)" 1
sleep 1.5
[ "$(tab_count)" -eq 1 ] || fail "middle click did not close tab two"
sidebar_text "$sb1" | grep -c "two" >/dev/null && fail "closed tab still rendered"
[ "$(active_title "$sb1")" = "one" ] || fail "wrong tab closed (active: $(active_title "$sb1"))"
echo "ok: middle click closes the clicked tab, not the active one"

click "$sb1" 5 "$(row_of "$sb1" "New tab")" 0
sleep 1.5
[ "$(tab_count)" -eq 2 ] || fail "click on the New tab card did not spawn a tab"
third_tab=$(tab_ids | cut -d' ' -f2)
sidebar_of "$third_tab" >/dev/null || fail "new tab has no sidebar"
echo "ok: new tab button spawns tab with sidebar"

cli set-tab-title --tab-id "$third_tab" three
sleep 1
# The press focuses the sidebar of the pressed tab, so motion and release arrive at $2, not $1.
# Separate sends keep the events out of one read chunk (the backend coalesces those) and let the dwell pass.
drag() { # press_pane follow_pane x1 y1 x2 y2
  cli send-text --no-paste --pane-id "$1" "$(printf '\033[<0;%s;%sM' "$3" "$4")"
  sleep 0.4
  cli send-text --no-paste --pane-id "$2" "$(printf '\033[<32;%s;%sM' "$5" "$6")"
  sleep 0.3
  cli send-text --no-paste --pane-id "$2" "$(printf '\033[<0;%s;%sm' "$5" "$6")"
}
sb3=$(sidebar_of "$third_tab")
# Both ends come from the live hit map: card heights are configurable, row 6 was not.
drag "$sb1" "$sb1" 5 "$(row_of "$sb1" one)" 5 "$(row_of "$sb1" three)"
sleep 1.5
[ "$(row_of "$sb1" three)" -lt "$(row_of "$sb1" one)" ] || fail "drag did not reorder tabs"
echo "ok: drag reorders tabs"

window_count() { list | python3 -c 'import json,sys; print(len({p["window_id"] for p in json.load(sys.stdin)}))'; }
drag "$sb1" "$sb3" 5 "$(row_of "$sb1" three)" 28 "$(row_of "$sb1" three)"
for _ in $(seq 1 16); do
  [ "$(window_count)" -eq 2 ] && break
  sleep 0.5
done
[ "$(window_count)" -eq 2 ] || { geometry; fail "drag to edge did not tear tab off into a new window"; }
sleep 1
sidebar_text "$sb1" | grep -c "three" >/dev/null && fail "torn-off tab still listed in the first window"
first_window=$(list | python3 -c 'import json,sys; t='"$first_tab"'; print([p["window_id"] for p in json.load(sys.stdin) if p["tab_id"]==t][0])')
moved_tab=$(list | python3 -c 'import json,sys; w='"$first_window"'; print([p["tab_id"] for p in json.load(sys.stdin) if p["window_id"]!=w][0])')
for _ in $(seq 1 16); do
  sidebar_of "$moved_tab" >/dev/null 2>&1 && break
  sleep 0.5
done
sbm=$(sidebar_of "$moved_tab") || fail "torn-off tab has no sidebar in its new window"
for _ in $(seq 1 16); do
  sidebar_text "$sbm" | grep -c "three" >/dev/null && break
  sleep 0.5
done
sidebar_text "$sbm" | grep -c "three" >/dev/null || { geometry; sidebar_text "$sbm" | head -5; fail "new window sidebar does not list the moved tab"; }
echo "ok: drag to edge moves tab to a new window with its own sidebar"

cli send-text --no-paste --pane-id "$(content_of "$moved_tab")" "exit
"
for _ in $(seq 1 20); do
  [ "$(tab_count)" -eq 1 ] && [ "$(window_count)" -eq 1 ] && break
  sleep 0.5
done
[ "$(tab_count)" -eq 1 ] || { geometry; fail "tab left with only a sidebar was not closed"; }
[ "$(window_count)" -eq 1 ] || fail "empty window did not close"
[ "$(active_title "$sb1")" = "one" ] || fail "orphan cleanup closed the wrong tab"
echo "ok: orphaned sidebar tab closed"

cli spawn --pane-id "$(content_of "$first_tab")" >/dev/null
for _ in $(seq 1 20); do
  [ "$(sidebar_panes | wc -l | tr -d ' ')" -eq 2 ] && break
  sleep 0.5
done
[ "$(sidebar_panes | wc -l | tr -d ' ')" -eq 2 ] || fail "second tab has no sidebar before the restart check"

if [ "$mode" = mux ]; then
  # A reattached client renumbers panes and tabs, so identity can only be counted, not compared.
  panes_of() { list | python3 -c 'import json,sys; t='"$1"'; print(sum(1 for p in json.load(sys.stdin) if p["tab_id"]==t))'; }
  sidebars_of() { list | python3 -c 'import json,sys; t='"$1"'; print(sum(1 for p in json.load(sys.stdin) if p["tab_id"]==t and '"$is_sb"'))'; }
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
  attached=$(date +%s%N)
  [ "$(tab_count)" -eq "$before_count" ] || { geometry; fail "the mux lost tabs across the restart: $before_count -> $(tab_count)"; }
  # A title set after the restart can only appear in a frame the reattached gui painted.
  after_tabs=$(tab_ids)
  for t in $after_tabs; do cli set-tab-title --tab-id "$t" "re$t"; done
  painted=""
  for _ in $(seq 1 200); do
    fresh=1
    for t in $after_tabs; do
      sidebar_text "$(sidebar_of "$t")" 2>/dev/null | grep -q "re$t" || fresh=0
    done
    [ "$fresh" -eq 1 ] && { painted=$(date +%s%N); break; }
    sleep 0.1
  done
  [ -n "$painted" ] || { geometry; fail "no sidebar painted a fresh frame after the restart"; }
  for t in $after_tabs; do
    [ "$(sidebars_of "$t")" -eq 1 ] || { geometry; fail "tab $t has $(sidebars_of "$t") sidebar panes after the restart, want 1"; }
    [ "$(panes_of "$t")" -eq 2 ] || { geometry; fail "tab $t has $(panes_of "$t") panes after the restart, want 2"; }
  done
  ms=$(((painted - attached) / 1000000))
  [ "$ms" -lt 3000 ] || { geometry; fail "sidebars took ${ms}ms to repaint after the restart, want < 3000"; }
  echo "ok: gui restart keeps one sidebar per tab and repaints in ${ms}ms"
fi

# Resize last: WezTerm hands the sidebar half of every new column, so a failure here moves the
# inner edge and would break every gesture that follows.
total_cols() { list | python3 -c 'import json,sys; print(max(p["left_col"]+p["size"]["cols"] for p in json.load(sys.stdin)))'; }
grow_tab=$(tab_ids | cut -d' ' -f2)
other_tab=$(tab_ids | cut -d' ' -f1)
before_cols=$(total_cols)
resize_mark=$(mark)
cli activate-tab --tab-id "$grow_tab"
vtest "$(content_of "$grow_tab")" grow
for _ in $(seq 1 20); do
  [ "$(total_cols)" -ne "$before_cols" ] && break
  sleep 0.25
done
[ "$(total_cols)" -ne "$before_cols" ] || fail "set_inner_size did not resize the window (still $before_cols cols)"
# The observer is registered before the plugin, so it reports the width WezTerm dealt out uncorrected.
drifted=$(since "$resize_mark" | grep -o "e2e: sidebar cols on resize [0-9,]*" | tail -1)
case "$drifted" in
  "") fail "no window-resized event observed" ;;
  *" 28"*) fail "resize did not widen any sidebar; the observer saw '$drifted'" ;;
esac
if ! settle_width "$grow_tab"; then
  # A desired width other than 28 means correct() mistook the resize for a divider drag.
  desired_mark=$(mark)
  vtest "$(content_of "$grow_tab")" probe_desired
  sleep 1.5
  geometry
  fail "the active tab's sidebar stayed $(width_of "$grow_tab") cols after the window grew;\
 $(since "$desired_mark" | grep -o 'e2e: desired width .*' | tail -1)"
fi
echo "ok: window grew $before_cols -> $(total_cols) cols; wezterm dealt the sidebar [$drifted] and correct() put it back to 28"

cli activate-tab --tab-id "$other_tab"
settle_width "$other_tab" || { geometry; fail "the background tab's sidebar stayed $(width_of "$other_tab") cols once it was activated"; }
echo "ok: a background tab's sidebar is corrected to 28 when it is activated"

rc_tab=$(tab_ids | cut -d' ' -f1)
rc_sb=$(sidebar_of "$rc_tab")
rc_content=$(content_of "$rc_tab")
rc_row=$(row_of "$rc_sb" "$(active_title "$rc_sb")")
rc_cols=$(list | python3 -c 'import json,sys; p='"$rc_content"'; print([q["size"]["cols"] for q in json.load(sys.stdin) if q["pane_id"]==p][0])')
# The menu is a GUI tab overlay: the CLI never sees it, so this only asserts nothing is destroyed.
active_pane() {
  m=$(mark)
  vtest "$1" probe_active
  for _ in $(seq 1 25); do
    p=$(since "$m" | sed -n 's/.*e2e: active pane //p' | tail -1)
    [ -n "$p" ] && { echo "$p"; return 0; }
    sleep 0.2
  done
  echo "no-answer"
}
press() { cli send-text --no-paste --pane-id "$1" "$(printf '\033[<%s;%s;%sM' "$4" "$2" "$3")"; }
release() { cli send-text --no-paste --pane-id "$1" "$(printf '\033[<%s;%s;%sm' "$4" "$2" "$3")"; }
[ "$(active_pane "$rc_content")" != no-answer ] || fail "the active-pane probe never answered"
press "$rc_sb" 5 "$rc_row" 2
sleep 1
[ "$(tab_count)" -eq 2 ] || { geometry; fail "the right-button press changed the tab count"; }
[ "$(width_of "$rc_tab")" -eq 28 ] || { geometry; fail "the right-button press resized the sidebar"; }
release "$rc_sb" 5 "$rc_row" 2
sleep 1.5
[ "$(tab_count)" -eq 2 ] || { geometry; fail "the right-button release changed the tab count"; }
[ "$(sidebar_of "$rc_tab")" = "$rc_sb" ] || { geometry; fail "the sidebar pane went away on right click"; }
[ "$(content_of "$rc_tab")" = "$rc_content" ] || { geometry; fail "the content pane went away on right click"; }
[ "$(width_of "$rc_tab")" -eq 28 ] || { geometry; fail "the sidebar changed size while the menu was open"; }
[ "$(cols_of "$rc_content")" -eq "$rc_cols" ] || { geometry; fail "the content pane changed size while the menu was open"; }
[ "$(active_pane "$rc_content")" != no-answer ] || fail "the window stopped answering after the menu opened"
echo "ok: right click keeps the sidebar and content panes alive at their sizes"

list | python3 -c 'import json,sys; [print("  pane", p["pane_id"], p["title"], "left", p["left_col"], "cols", p["size"]["cols"]) for p in json.load(sys.stdin)]'
# The check above ends with a right press+release on a card, which is what opens a popover.
pop_sb="$rc_sb"
pop_content="$rc_content"
pop_content_cols=$(cols_of "$pop_content")
for _ in $(seq 1 24); do
  sidebar_text "$pop_sb" | grep -q "Switch to tab" && break
  sleep 0.25
done
sidebar_text "$pop_sb" | grep -q "Switch to tab" || { sidebar_text "$pop_sb" | head -20; fail "right click did not open the popover in the sidebar"; }
sidebar_text "$pop_sb" | grep -q "Close tab" || fail "the popover is missing its items"
[ "$(cols_of "$pop_content")" -eq "$pop_content_cols" ] || fail "the popover resized the content pane"
cli get-text --pane-id "$pop_content" | grep -q "Switch to tab" && fail "the popover leaked into the content pane"
echo "ok: right click draws the popover inside the sidebar, content untouched"

pop_switch=$(row_of "$pop_sb" "Switch to tab")
click "$pop_sb" 6 "$pop_switch" 0
for _ in $(seq 1 24); do
  sidebar_text "$pop_sb" | grep -q "Switch to tab" || break
  sleep 0.25
done
sidebar_text "$pop_sb" | grep -q "Switch to tab" && fail "the popover stayed open after an item click"
# Title-independent: "Switch to tab" focuses that tab's content pane.
[ "$(active_pane "$pop_content")" = "$pop_content" ] || fail "the item did not switch to its tab (active pane: $(active_pane "$pop_content"))"
echo "ok: clicking a popover item runs it and closes the popover"

echo "all e2e checks passed"
