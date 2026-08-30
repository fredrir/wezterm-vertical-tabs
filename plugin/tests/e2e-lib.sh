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
# A group of checks that pins a bug still open upstream. Fatal, so nobody forgets it; set
# `VTABS_STRESS_SOFT=1` to print XFAIL instead and let the rest of the run continue.
soft() {
  if [ -n "${VTABS_STRESS_SOFT:-}" ]; then
    ("$@") || echo "XFAIL: $1"
  else
    "$@"
  fi
}
cli() { wezterm cli --no-auto-start "$@"; }
mark() { wc -l <"$log" | tr -d ' '; }
since() { tail -n "+$(($1 + 1))" "$log"; }
# Runs a probe defined in wezterm-e2e.lua by making the pane print an OSC 1337 SetUserVar.
vtest() { cli send-text --no-paste --pane-id "$1" "printf '\\033]1337;SetUserVar=vtabs_test=$(printf %s "$2" | base64)\\a'
"; }
# A gui busy with a resize can answer an empty body; every helper below parses this, so retry here.
list() {
  for _ in 1 2 3 4 5; do
    body=$(cli list --format json 2>/dev/null || true)
    case "$body" in
      \[*) printf '%s' "$body"; return 0 ;;
    esac
    sleep 0.3
  done
  fail "wezterm cli list stopped answering"
}
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
press() { cli send-text --no-paste --pane-id "$1" "$(printf '\033[<%s;%s;%sM' "$4" "$2" "$3")"; }
release() { cli send-text --no-paste --pane-id "$1" "$(printf '\033[<%s;%s;%sm' "$4" "$2" "$3")"; }
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
# Tabs holding more panes than a sidebar plus the content; catches a second sidebar before its
# title lands. `max_panes` rises while a check deliberately splits the content.
max_panes=2
fat_tabs() {
  list | python3 -c '
import json,sys,collections
n=collections.Counter(p["tab_id"] for p in json.load(sys.stdin))
print(" ".join("%d:%d"%(t,c) for t,c in sorted(n.items()) if c>'"$max_panes"'))'
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

# Sidebar panes on a tab whose title the backend has claimed.
marked_of() { list | python3 -c 'import json,sys; t='"$1"'; print(sum(1 for p in json.load(sys.stdin) if p["tab_id"]==t and '"$is_marked"'))'; }
window_of() { list | python3 -c 'import json,sys; t='"$1"'; print([p["window_id"] for p in json.load(sys.stdin) if p["tab_id"]==t][0])'; }
# Tabs of the window holding the most of them; collapse state is per window, so width checks that
# span two tabs have to stay inside one.
busiest_window_tabs() {
  list | python3 -c '
import json,sys,collections
by=collections.defaultdict(set)
for p in json.load(sys.stdin): by[p["window_id"]].add(p["tab_id"])
print(" ".join(str(t) for t in sorted(max(by.values(), key=len))))'
}
# Waits for a tab's lazy attach, asserting the invariant on every look.
wait_attached() { # tab_id [seconds]
  n=$((${2:-8} * 4))
  while [ "$n" -gt 0 ]; do
    [ "$(marked_of "$1")" -ge 1 ] && break
    no_dupes "waiting for tab $1 to attach"
    n=$((n - 1)); sleep 0.25
  done
  [ "$(marked_of "$1")" -eq 1 ] || { geometry; fail "tab $1 has $(marked_of "$1") sidebars after its lazy attach"; }
}

# --- log assertions --------------------------------------------------------
# Plugin-side warnings and errors since a `mark`; an empty answer means the step was clean.
vtabs_warnings() { since "$1" | grep -E "(WARN|ERROR).*(lua: )?vtabs: " | grep -v "some glyphs are not one cell wide" || true; }
no_warnings() { # mark label
  w=$(vtabs_warnings "$1")
  [ -z "$w" ] || { echo "$w"; fail "the plugin warned during $2"; }
}

# --- frozen-frame detection ------------------------------------------------
# A pane that stops repainting keeps its last frame; the text alone cannot say which, so callers
# name the string the sidebar must show once it repaints.
frame_shows() { # sidebar_pane needle [seconds]
  n=$((${3:-8} * 4))
  while [ "$n" -gt 0 ]; do
    sidebar_text "$1" 2>/dev/null | grep -qF "$2" && return 0
    n=$((n - 1)); sleep 0.25
  done
  return 1
}
# A pane whose render threw keeps whatever it last painted, or nothing at all if it never painted.
renders() { # sidebar_pane label
  text=$(sidebar_text "$1" 2>/dev/null | tr -d ' \n')
  [ -n "$text" ] || { geometry; fail "sidebar $1 painted nothing during $2"; }
}
not_frozen() { # sidebar_pane tab_id label — retitles the tab and waits for the frame to follow
  needle="live$(date +%N | tail -c 5)"
  cli set-tab-title --tab-id "$2" "$needle"
  frame_shows "$1" "$needle" 8 || { sidebar_text "$1" | head -12; fail "sidebar $1 froze during $3"; }
}

# --- width traces (item 9) -------------------------------------------------
# One line per tab: sidebar cols, content cols and the tab's own width.
widths() {
  list | python3 -c '
import json,sys,collections
panes=json.load(sys.stdin)
by=collections.defaultdict(list)
for p in panes: by[p["tab_id"]].append(p)
if not panes: print("    (no panes)")
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
