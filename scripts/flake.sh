#!/bin/sh
# Runs an e2e suite N times and reports how often each distinct failure appears, so a flake can be
# given a rate instead of an anecdote. `sh scripts/flake.sh [runs] [local|mux] [e2e|stress]`.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
runs=${1:-5}
mode=${2:-local}
suite=${3:-e2e}
out=${VTABS_FLAKE:-$root/.claude/team/flake}/$(cd "$root" && git rev-parse --short HEAD)-$suite-$mode
rm -rf "$out"
mkdir -p "$out"

bin="${VTABS_BIN:-$root/backend/target/release/wez-vtabs}"
[ -x "$bin" ] || (cd "$root/backend" && cargo build --locked --release)
export VTABS_BIN="$bin"

case "$suite" in
  e2e) script=$root/plugin/tests/e2e.sh ;;
  stress) script=$root/plugin/tests/stress.sh ;;
  *) echo "suite must be e2e or stress"; exit 1 ;;
esac

echo "$runs x $suite ($mode) at $(cd "$root" && git rev-parse --short HEAD)"
passed=0
n=0
while [ "$n" -lt "$runs" ]; do
  n=$((n + 1))
  VTABS_STRESS_SOFT=1 VTABS_STRESS_FAST=1 timeout 2700 \
    xvfb-run -a -s "-screen 0 1600x900x24" sh "$script" "$mode" >"$out/run$n.log" 2>&1 || true
  # `VTABS_STRESS_SOFT=1` lets a stress run finish with XFAILs, so those are counted separately.
  grep -h "^XFAIL:" "$out/run$n.log" >>"$out/xfails.txt" 2>/dev/null || true
  if grep -q "^all .* checks passed" "$out/run$n.log"; then
    passed=$((passed + 1))
    echo "  run $n: pass ($(grep -c "^XFAIL:" "$out/run$n.log" || true) xfail)"
  else
    # The failure text minus the ids and counts that differ between runs, so equal causes group.
    grep -h "^FAIL:" "$out/run$n.log" |
      sed 's/[0-9][0-9]*/N/g' >>"$out/failures.txt" || true
    echo "  run $n: $(grep -m1 "^FAIL:" "$out/run$n.log" || echo "no FAIL line; see $out/run$n.log")"
  fi
done

echo
echo "passed $passed/$runs"
if [ -s "$out/failures.txt" ]; then
  echo "distinct failures by rate:"
  sort "$out/failures.txt" | uniq -c | sort -rn |
    awk -v r="$runs" '{c=$1; $1=""; printf "  %d/%s  %s\n", c, r, substr($0,2)}'
else
  echo "no failures"
fi
if [ -s "$out/xfails.txt" ]; then
  echo "xfail groups by rate:"
  sort "$out/xfails.txt" | uniq -c | sort -rn |
    awk -v r="$runs" '{c=$1; $1=""; printf "  %d/%s  %s\n", c, r, substr($0,2)}'
fi
echo "logs in $out"
