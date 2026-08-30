#!/bin/sh
# Behaviour-neutrality baseline. `sh scripts/baseline.sh` records everything a refactor must not
# change; `sh scripts/baseline.sh --check` re-records and diffs against the stored run.
#
# What is recorded, and how it is compared:
#
#   frames/     every render frame `plugin/tests/run.lua` can dump   byte for byte
#   shots/      one pixel hash per screenshot state's sidebar crop   hash equality
#   geometry/   `wezterm cli list` after each e2e step, both modes   text
#   stress/     the groups that ended XFAIL, per mode                text
#
# A refactor step is behaviour-neutral when `--check` reports no differences at all.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
sha=$(cd "$root" && git rev-parse --short HEAD)
base=${VTABS_BASELINE:-$root/.claude/team/baseline}
mode=record
[ "${1:-}" = "--check" ] && mode=check

if [ "$mode" = check ]; then
  ref=${VTABS_BASELINE_REF:-}
  if [ -z "$ref" ]; then
    # The newest recorded run that is not this one; a refactor compares against what came before.
    ref=$(ls -1dt "$base"/*/ 2>/dev/null | grep -v "/$sha/$" | head -1 || true)
    ref=${ref%/}
  fi
  [ -n "$ref" ] && [ -d "$ref" ] || { echo "no baseline to check against under $base"; exit 1; }
  out=$(mktemp -d /tmp/vtbase.XXXXXX)
  echo "checking $sha against $(basename "$ref")"
else
  out=$base/$sha
  rm -rf "$out"
  echo "recording $sha into $out"
fi
mkdir -p "$out/frames" "$out/shots" "$out/geometry" "$out/stress"

bin="${VTABS_BIN:-$root/backend/target/release/wez-vtabs}"
[ -x "$bin" ] || (cd "$root/backend" && cargo build --locked --release)
export VTABS_BIN="$bin"

# --- frames: pure renders, no display needed -------------------------------
echo "  frames"
(cd "$root/plugin" && VTABS_DUMP_FRAMES="$out/frames" lua tests/run.lua >"$out/frames.log" 2>&1) ||
  { echo "  lua suite failed; see $out/frames.log"; exit 1; }
tail -1 "$out/frames.log"

# --- shots: one hash per state, so a diff names the state that moved -------
echo "  shots"
shots=$(mktemp -d /tmp/vtshots.XXXXXX)
VTABS_SHOTS="$shots" xvfb-run -a -s "-screen 0 1600x900x24" sh "$root/scripts/screenshot.sh" \
  >"$out/shots.log" 2>&1 || true
# The sha256 of no bytes is a perfectly good-looking hash, so an unrendered state would compare
# "identical" against another unrendered one. Hash only real pixels and record the roster instead.
empty=$(printf "" | sha256sum | cut -d' ' -f1)
hashed=0
for png in "$shots"/*-sidebar.png; do
  [ -s "$png" ] || continue
  state=$(basename "$png" -sidebar.png)
  hash=$(magick "$png" -depth 8 ppm:- 2>/dev/null | sha256sum | cut -d' ' -f1)
  [ -n "$hash" ] && [ "$hash" != "$empty" ] || { echo "  warn: $state produced no pixels"; continue; }
  echo "$hash" >"$out/shots/$state"
  hashed=$((hashed + 1))
done
# Every state the run was asked for, so a state that stops rendering is a diff, not a silent gap.
sed -n 's/^\([a-z0-9-]*\)|.*/\1/p' "$root/scripts/screenshot.sh" | sort >"$out/shots/roster"
echo "$hashed" >"$out/shots/count"
echo "    $hashed of $(wc -l <"$out/shots/roster" | tr -d ' ') states hashed"
[ "$hashed" -gt 0 ] || { echo "  no screenshot state rendered; refusing to record an empty baseline"; exit 1; }
rm -rf "$shots"

# --- geometry and stress: both e2e modes ----------------------------------
for m in local mux; do
  echo "  e2e $m"
  # `ok` in e2e-lib.sh appends the pane geometry after every step it announces.
  VTABS_E2E_GEOMETRY="$out/geometry/$m.txt" xvfb-run -a -s "-screen 0 1600x900x24" \
    sh "$root/plugin/tests/e2e.sh" "$m" >"$out/geometry/$m.log" 2>&1 || true
  echo "    $(wc -l <"$out/geometry/$m.txt" | tr -d ' ') geometry rows"

  echo "  stress $m"
  VTABS_STRESS_SOFT=1 VTABS_STRESS_FAST=1 xvfb-run -a -s "-screen 0 1600x900x24" \
    sh "$root/plugin/tests/stress.sh" "$m" >"$out/stress/$m.log" 2>&1 || true
  grep -o "^XFAIL: .*" "$out/stress/$m.log" | sort >"$out/stress/$m.txt" || true
  echo "    XFAIL: $(tr '\n' ' ' <"$out/stress/$m.txt")"
done

if [ "$mode" = check ]; then
  echo
  differences=0
  for kind in frames shots geometry stress; do
    if diff -rq "$ref/$kind" "$out/$kind" >"$out/$kind.diff" 2>&1; then
      echo "ok: $kind identical"
    else
      differences=1
      echo "DIFF: $kind"
      sed 's/^/    /' "$out/$kind.diff" | head -20
    fi
  done
  echo
  if [ "$differences" -eq 0 ]; then
    echo "behaviour-neutral: no differences against $(basename "$ref")"
    rm -rf "$out"
  else
    echo "NOT behaviour-neutral; the run is kept at $out"
    exit 1
  fi
else
  echo
  echo "baseline for $sha recorded in $out"
fi
