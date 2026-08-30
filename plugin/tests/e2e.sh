#!/bin/sh
# Drives a throwaway WezTerm instance and exercises the sidebar through the CLI.
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
bin="${VTABS_BIN:-$root/backend/target/release/wez-vtabs}"
[ -x "$bin" ] || { echo "backend binary not found: $bin"; exit 1; }
log=$(mktemp -t vtabs-e2e)

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
HOME="$home" VTABS_ROOT="$root" VTABS_BIN="$bin" WEZTERM_LOG=info wezterm --config-file "$root/plugin/tests/wezterm-e2e.lua" \
  $launch --class vtabs-e2e >"$log" 2>&1 &
pid=$!
cleanup() {
  set +e
  kill $pid 2>/dev/null; wait $pid 2>/dev/null
  if [ "$mode" = mux ]; then
    sleep 1
    for p in $(pgrep -x wezterm-mux-server); do
      if ps -p "$p" -E -o command= | grep -c "$home" >/dev/null; then
        kill "$p" || true
      fi
    done
  fi
  rm -rf "$home"
  grep -i "vtabs: \(user-var\|update-status\|window-\)\|WARN\|ERROR" "$log" | grep -v "Broken pipe" | tail -20
  rm -f "$log"
}
trap cleanup EXIT

export WEZTERM_UNIX_SOCKET="$home/.local/share/wezterm/gui-sock-$pid"
for _ in $(seq 1 50); do
  [ -S "$WEZTERM_UNIX_SOCKET" ] && wezterm cli --no-auto-start list >/dev/null 2>&1 && break
  sleep 0.2
done
sleep 2

fail() { echo "FAIL: $*"; exit 1; }
cli() { wezterm cli --no-auto-start "$@"; }
list() { cli list --format json; }
is_sb='(p["title"].startswith("wez-vtabs") or (p["left_col"]==0 and p["size"]["cols"]==28))'
sidebar_panes() { list | python3 -c 'import json,sys; print("\n".join(str(p["pane_id"]) for p in json.load(sys.stdin) if '"$is_sb"'))'; }
sidebar_of() { list | python3 -c 'import json,sys; t='"$1"'; print([p["pane_id"] for p in json.load(sys.stdin) if p["tab_id"]==t and '"$is_sb"'][0])'; }
content_of() { list | python3 -c 'import json,sys; t='"$1"'; print([p["pane_id"] for p in json.load(sys.stdin) if p["tab_id"]==t and not '"$is_sb"'][0])'; }
tab_ids() { list | python3 -c 'import json,sys; print(" ".join(str(t) for t in sorted({p["tab_id"] for p in json.load(sys.stdin)})))'; }
tab_count() { tab_ids | wc -w | tr -d ' '; }
sidebar_text() { cli get-text --pane-id "$1"; }
active_title() { sidebar_text "$1" | python3 -c 'import sys; rows=[l for l in sys.stdin.read().split("\n") if "▎" in l]; print(rows[0].split("▎",1)[1].split()[0] if rows else "")'; }
row_of() { sidebar_text "$1" | python3 -c 'import sys; rows=sys.stdin.read().split("\n"); print(next(i+1 for i,l in enumerate(rows) if "'"$2"'" in l))'; }
click() { cli send-text --no-paste --pane-id "$1" "$(printf '\033[<%s;%s;%sM\033[<%s;%s;%sm' "$4" "$2" "$3" "$4" "$2" "$3")"; }

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

click "$sb1" 5 "$(row_of "$sb1" "New Tab")" 0
sleep 1.5
[ "$(tab_count)" -eq 2 ] || fail "click on New Tab row did not spawn a tab"
third_tab=$(tab_ids | cut -d' ' -f2)
sidebar_of "$third_tab" >/dev/null || fail "new tab has no sidebar"
echo "ok: new tab button spawns tab with sidebar"

cli set-tab-title --tab-id "$third_tab" three
sleep 1
drag() { cli send-text --no-paste --pane-id "$1" "$(printf '\033[<0;%s;%sM\033[<32;%s;%sM\033[<32;%s;%sM\033[<0;%s;%sm' "$2" "$3" "$2" "$3" "$4" "$5" "$4" "$5")"; }
drag "$sb1" 5 "$(row_of "$sb1" three)" 5 "$(row_of "$sb1" one)"
sleep 1.5
[ "$(row_of "$sb1" three)" -lt "$(row_of "$sb1" one)" ] || fail "drag did not reorder tabs"
echo "ok: drag reorders tabs"

window_count() { list | python3 -c 'import json,sys; print(len({p["window_id"] for p in json.load(sys.stdin)}))'; }
drag "$sb1" 5 "$(row_of "$sb1" three)" 28 "$(row_of "$sb1" three)"
sleep 2
[ "$(window_count)" -eq 2 ] || fail "drag to edge did not tear tab off into a new window"
sidebar_text "$sb1" | grep -c "three" >/dev/null && fail "torn-off tab still listed in the first window"
first_window=$(list | python3 -c 'import json,sys; t='"$first_tab"'; print([p["window_id"] for p in json.load(sys.stdin) if p["tab_id"]==t][0])')
moved_tab=$(list | python3 -c 'import json,sys; w='"$first_window"'; print([p["tab_id"] for p in json.load(sys.stdin) if p["window_id"]!=w][0])')
sbm=$(sidebar_of "$moved_tab") || fail "torn-off tab has no sidebar in its new window"
sleep 2
geometry() { list | python3 -c 'import json,sys; [print("  win", p["window_id"], "tab", p["tab_id"], "pane", p["pane_id"], p["title"], "left", p["left_col"], "cols", p["size"]["cols"]) for p in json.load(sys.stdin)]'; }
sidebar_text "$sbm" | grep -c "three" >/dev/null || { geometry; sidebar_text "$sbm" | head -5; fail "new window sidebar does not list the moved tab"; }
echo "ok: drag to edge moves tab to a new window with its own sidebar"

cli send-text --no-paste --pane-id "$(content_of "$moved_tab")" "exit
"
sleep 1.5
[ "$(tab_count)" -eq 1 ] || fail "tab left with only a sidebar was not closed"
[ "$(window_count)" -eq 1 ] || fail "empty window did not close"
[ "$(active_title "$sb1")" = "one" ] || fail "orphan cleanup closed the wrong tab"
echo "ok: orphaned sidebar tab closed"

list | python3 -c 'import json,sys; [print("  pane", p["pane_id"], p["title"], "left", p["left_col"], "cols", p["size"]["cols"]) for p in json.load(sys.stdin)]'
echo "all e2e checks passed"
