#!/bin/sh
# Shared harness for the e2e scripts: launches a throwaway WezTerm and wraps `wezterm cli`.
# Sourced, never run. Callers set `mode` (local|mux) first and read `$pid` after `e2e_launch`.
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

mode="${mode:-local}"
home=$(mktemp -d /tmp/vte2e.XXXXXX)
if [ "$mode" = mux ]; then
  export VTABS_E2E_MUX="$home/mux.sock"
  launch="connect e2emux"
else
  launch="start --always-new-process"
fi
echo "mode: $mode"
# Isolated HOME keeps sockets and the mux pid file away from the real wezterm.
mkdir -p "$home/run"

e2e_launch() {
  # shellcheck disable=SC2086
  HOME="$home" XDG_RUNTIME_DIR="$home/run" VTABS_ROOT="$root" VTABS_BIN="$bin" \
    VTABS_E2E_COLLAPSED="${VTABS_E2E_COLLAPSED:-hidden}" WEZTERM_LOG=info \
    wezterm --config-file "$root/plugin/tests/wezterm-e2e.lua" \
    $launch --class vtabs-e2e >"$log" 2>&1 &
  pid=$!
  export WEZTERM_UNIX_SOCKET="$home/run/wezterm/gui-sock-$pid"
  for _ in $(seq 1 50); do
    [ -S "$WEZTERM_UNIX_SOCKET" ] && wezterm cli --no-auto-start list >/dev/null 2>&1 && break
    sleep 0.2
  done
  sleep 2
}

e2e_cleanup() {
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

fail() { echo "FAIL: $*"; exit 1; }
cli() { wezterm cli --no-auto-start "$@"; }
mark() { wc -l <"$log" | tr -d ' '; }
since() { tail -n "+$(($1 + 1))" "$log"; }
# Runs a probe defined in wezterm-e2e.lua by making the pane print an OSC 1337 SetUserVar.
vtest() { cli send-text --no-paste --pane-id "$1" "printf '\\033]1337;SetUserVar=vtabs_test=$(printf %s "$2" | base64)\\a'
"; }
list() { cli list --format json; }
is_sb='(p["title"].startswith("wez-vtabs") or (p["left_col"]==0 and p["size"]["cols"]==28))'
# Marker title only: a pane the backend already claimed. Never guesses from geometry.
is_marked='p["title"].startswith("wez-vtabs")'
sidebar_panes() { list | python3 -c 'import json,sys; print("\n".join(str(p["pane_id"]) for p in json.load(sys.stdin) if '"$is_sb"'))'; }
sidebar_of() { list | python3 -c 'import json,sys; t='"$1"'; print([p["pane_id"] for p in json.load(sys.stdin) if p["tab_id"]==t and '"$is_sb"'][0])'; }
content_of() { list | python3 -c 'import json,sys; t='"$1"'; print([p["pane_id"] for p in json.load(sys.stdin) if p["tab_id"]==t and not '"$is_sb"'][0])'; }
tab_ids() { list | python3 -c 'import json,sys; print(" ".join(str(t) for t in sorted({p["tab_id"] for p in json.load(sys.stdin)})))'; }
tab_count() { tab_ids | wc -w | tr -d ' '; }
sidebar_text() { cli get-text --pane-id "$1"; }
tab_of_pane() { list | python3 -c 'import json,sys; s='"$1"'; print([p["tab_id"] for p in json.load(sys.stdin) if p["pane_id"]==s][0])'; }
width_of() { list | python3 -c 'import json,sys; t='"$1"'; print([p["size"]["cols"] for p in json.load(sys.stdin) if p["tab_id"]==t and '"$is_sb"'][0])'; }
cols_of() { list | python3 -c 'import json,sys; p='"$1"'; print([q["size"]["cols"] for q in json.load(sys.stdin) if q["pane_id"]==p][0])'; }
total_cols() { list | python3 -c 'import json,sys; print(max(p["left_col"]+p["size"]["cols"] for p in json.load(sys.stdin)))'; }
window_count() { list | python3 -c 'import json,sys; print(len({p["window_id"] for p in json.load(sys.stdin)}))'; }
geometry() { list | python3 -c 'import json,sys; [print("  win", p["window_id"], "tab", p["tab_id"], "pane", p["pane_id"], p["title"], "left", p["left_col"], "cols", p["size"]["cols"]) for p in json.load(sys.stdin)]'; }
click() { cli send-text --no-paste --pane-id "$1" "$(printf '\033[<%s;%s;%sM\033[<%s;%s;%sm' "$4" "$2" "$3" "$4" "$2" "$3")"; }
row_of() { sidebar_text "$1" | python3 -c 'import sys; rows=sys.stdin.read().split("\n"); print(next(i+1 for i,l in enumerate(rows) if "'"$2"'" in l))'; }
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
active_title() { probe_line "$(content_of "$(tab_of_pane "$1")")" probe_active_title "active title"; }

# --- duplicate-sidebar assertions (item 5) ---------------------------------
# Tabs holding more than one pane whose title the backend claimed, as "tab:count".
dupe_tabs() {
  list | python3 -c '
import json,sys,collections
n=collections.Counter()
for p in json.load(sys.stdin):
    if '"$is_marked"': n[p["tab_id"]]+=1
print(" ".join("%d:%d"%(t,c) for t,c in sorted(n.items()) if c>1))'
}
# Tabs holding more than two panes at all; catches a second sidebar before its title lands.
fat_tabs() {
  list | python3 -c '
import json,sys,collections
n=collections.Counter(p["tab_id"] for p in json.load(sys.stdin))
print(" ".join("%d:%d"%(t,c) for t,c in sorted(n.items()) if c>2))'
}
# Asserts the invariant after one step. $1 labels the step in the failure text.
no_dupes() {
  d=$(dupe_tabs)
  [ -z "$d" ] || { geometry; fail "duplicate sidebars after $1 (tab:panes $d)"; }
  f=$(fat_tabs)
  [ -z "$f" ] || { geometry; fail "extra panes after $1 (tab:panes $f)"; }
}
# Same assertion, retried: a duplicate that heals within $2 seconds is still reported.
no_dupes_settled() {
  n=$((${2:-3} * 4))
  while [ "$n" -gt 0 ]; do
    [ -z "$(dupe_tabs)$(fat_tabs)" ] && break
    n=$((n - 1)); sleep 0.25
  done
  no_dupes "$1"
}

# --- width traces (item 9) -------------------------------------------------
# One line per tab: sidebar cols, content cols and the tab's own width.
widths() {
  list | python3 -c '
import json,sys,collections
panes=json.load(sys.stdin)
by=collections.defaultdict(list)
for p in panes: by[p["tab_id"]].append(p)
for t in sorted(by):
    ps=by[t]
    sb=[p for p in ps if '"$is_sb"']
    ct=[p for p in ps if not '"$is_sb"']
    print("    tab %d sidebar %s content %s tab_cols %d" % (
        t,
        ",".join(str(p["size"]["cols"]) for p in sb) or "-",
        ",".join(str(p["size"]["cols"]) for p in ct) or "-",
        max(p["left_col"]+p["size"]["cols"] for p in ps)))'
}
trace() { echo "  trace $*"; widths; }
