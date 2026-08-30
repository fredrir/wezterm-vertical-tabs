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
# `--only <kinds>` narrows a run to a comma-separated subset of those four. `--only frames` needs
# no display and no backend, so it is the seconds-long gate to run between edits; the full run is
# the gate a step lands on.
#
# A refactor step is behaviour-neutral when `--check` reports no differences at all.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
sha=$(cd "$root" && git rev-parse --short HEAD)
# Every worktree shares one baseline: the sticky reference is worthless if each worktree keeps a
# private copy of it, and the refactor steps are all done in worktrees.
main=$(cd "$root" && cd "$(git rev-parse --git-common-dir)/.." && pwd)
base=${VTABS_BASELINE:-$main/.claude/team/baseline}

ALL="frames shots geometry stress"
mode=record
pin=
only=

usage() {
  echo "usage: baseline.sh [--check] [--pin] [--only $(echo "$ALL" | tr ' ' ',')]"
  exit 1
}
while [ $# -gt 0 ]; do
  case "$1" in
    --check) mode=check ;;
    # Moves the sticky reference to this run; without it the first recording stays the reference.
    --pin) pin=1 ;;
    --only) shift; [ $# -gt 0 ] || usage; only=$(echo "$1" | tr ',' ' ') ;;
    --only=*) only=$(echo "${1#--only=}" | tr ',' ' ') ;;
    *) usage ;;
  esac
  shift
done
for kind in $only; do
  case " $ALL " in
    *" $kind "*) ;;
    *) echo "unknown kind: $kind"; usage ;;
  esac
done
kinds=${only:-$ALL}
wants() {
  case " $kinds " in
    *" $1 "*) return 0 ;;
  esac
  return 1
}

if [ "$mode" = check ]; then
  ref=${VTABS_BASELINE_REF:-}
  if [ -z "$ref" ] && [ -f "$base/REF" ]; then
    # The sticky pre-refactor reference: every step measures against it, so drift cannot accumulate
    # one behaviour-neutral step at a time. `--pin` moves it; VTABS_BASELINE_REF overrides for one run.
    ref=$base/$(cat "$base/REF")
  fi
  if [ -z "$ref" ]; then
    ref=$(ls -1dt "$base"/*/ 2>/dev/null | grep -v "/$sha/$" | head -1 || true)
    ref=${ref%/}
  fi
  [ -n "$ref" ] && [ -d "$ref" ] || { echo "no baseline to check against under $base"; exit 1; }
  # A kind the reference never recorded has nothing to differ from, so it is reported as skipped
  # rather than counted as a difference. Old references predate the file and hold all four.
  held=$ALL
  [ -f "$ref/KINDS" ] && held=$(tr '\n' ' ' <"$ref/KINDS")
  skipped=
  wanted=
  for kind in $kinds; do
    case " $held " in
      *" $kind "*) wanted="$wanted $kind" ;;
      *) skipped="$skipped $kind" ;;
    esac
  done
  kinds=${wanted# }
  out=$(mktemp -d /tmp/vtbase.XXXXXX)
  echo "checking $sha against $(basename "$ref") [${kinds:-nothing}]"
  [ -z "$skipped" ] || echo "  skipping${skipped}: not recorded in $(basename "$ref")"
else
  out=$base/$sha
  # A narrowed record refreshes its own kinds and leaves the rest of the run alone.
  if [ -z "$only" ]; then
    rm -rf "$out"
  else
    for kind in $kinds; do rm -rf "${out:?}/$kind" "${out:?}/$kind.diff"; done
  fi
  echo "recording $sha into $out [$kinds]"
fi
mkdir -p "$out"
for kind in $kinds; do mkdir -p "$out/$kind"; done

# Only the display-bound kinds need the backend; `--only frames` must stay a seconds-long gate.
if wants shots || wants geometry || wants stress; then
  bin="${VTABS_BIN:-$root/backend/target/release/wez-vtabs}"
  [ -x "$bin" ] || (cd "$root/backend" && cargo build --locked --release)
  export VTABS_BIN="$bin"
fi

# --- frames: pure renders, no display needed -------------------------------
if wants frames; then
  echo "  frames"
  (cd "$root/plugin" && VTABS_DUMP_FRAMES="$out/frames" lua tests/run.lua >"$out/frames.log" 2>&1) ||
    { echo "  lua suite failed; see $out/frames.log"; exit 1; }
  tail -1 "$out/frames.log"
fi

# --- shots: one hash per state, so a diff names the state that moved -------
if wants shots; then
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
  sed 's/^STATES="//' "$root/scripts/screenshot.sh" |
    sed -n 's/^\([a-z0-9-]*\)|.*/\1/p' | sort >"$out/shots/roster"
  echo "$hashed" >"$out/shots/count"
  echo "    $hashed of $(wc -l <"$out/shots/roster" | tr -d ' ') states hashed"
  [ "$hashed" -gt 0 ] || { echo "  no screenshot state rendered; refusing to record an empty baseline"; exit 1; }
  rm -rf "$shots"
fi

# --- geometry and stress: both e2e modes ----------------------------------
for m in local mux; do
  if wants geometry; then
    echo "  e2e $m"
    # `ok` in e2e-lib.sh appends the pane geometry after every step it announces.
    VTABS_E2E_GEOMETRY="$out/geometry/$m.txt" xvfb-run -a -s "-screen 0 1600x900x24" \
      sh "$root/plugin/tests/e2e.sh" "$m" >"$out/geometry/$m.log" 2>&1 || true
    echo "    $(wc -l <"$out/geometry/$m.txt" | tr -d ' ') geometry rows"
  fi

  if wants stress; then
    echo "  stress $m"
    VTABS_STRESS_SOFT=1 VTABS_STRESS_FAST=1 xvfb-run -a -s "-screen 0 1600x900x24" \
      sh "$root/plugin/tests/stress.sh" "$m" >"$out/stress/$m.log" 2>&1 || true
    grep -o "^XFAIL: .*" "$out/stress/$m.log" | sort >"$out/stress/$m.txt" || true
    echo "    XFAIL: $(tr '\n' ' ' <"$out/stress/$m.txt")"
  fi
done

if [ "$mode" = check ]; then
  echo
  differences=0
  for kind in $kinds; do
    if diff -rq "$ref/$kind" "$out/$kind" >"$out/$kind.diff" 2>&1; then
      echo "ok: $kind identical"
    else
      differences=1
      echo "DIFF: $kind"
      sed 's/^/    /' "$out/$kind.diff" | head -20
    fi
  done
  echo
  if [ -z "$kinds" ]; then
    echo "nothing to compare: $(basename "$ref") holds none of the requested kinds"
    rm -rf "$out"
    exit 1
  elif [ "$differences" -eq 0 ]; then
    echo "behaviour-neutral: no differences against $(basename "$ref") [$kinds]"
    rm -rf "$out"
  else
    echo "NOT behaviour-neutral; the run is kept at $out"
    exit 1
  fi
else
  # What this run holds, so a later `--check` can tell an unrecorded kind from a changed one.
  held=
  [ -f "$out/KINDS" ] && held=$(tr '\n' ' ' <"$out/KINDS")
  for kind in $ALL; do
    case " $held $kinds " in
      *" $kind "*) echo "$kind" ;;
    esac
  done >"$out/KINDS"
  full=$(tr '\n' ' ' <"$out/KINDS")
  complete=
  [ "$(wc -w <"$out/KINDS" | tr -d ' ')" -eq 4 ] && complete=1
  mkdir -p "$base"
  # Only a complete run may become the sticky reference: a partial one would make every later
  # check silently skip the kinds it never held — the half-covered reference the roster guards against.
  if [ -z "$complete" ]; then
    [ -z "$pin" ] || { echo; echo "refusing to pin $sha: it holds only [${full% }], not all four kinds"; exit 1; }
  elif [ ! -f "$base/REF" ] || [ -n "$pin" ]; then
    printf '%s' "$sha" >"$base/REF"
    echo
    echo "sticky reference is now $sha"
  fi
  echo
  echo "baseline for $sha recorded in $out [${full% }]"
  if [ -f "$base/REF" ]; then
    echo "checks compare against $(cat "$base/REF") unless VTABS_BASELINE_REF says otherwise"
  else
    echo "no sticky reference yet: record all four kinds to set one"
  fi
fi
