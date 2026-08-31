#!/bin/sh
# The P1 oracle: re-renders every golden scene (pure Lua, no display, no backend build) and
# byte-compares both the text and styled dumps against plugin/tests/golden/frames.
#
# `sh scripts/check-frames.sh [rendered-dir]` — pass a dir baseline.sh already rendered into to
# skip re-running the suite; with no argument this renders into its own scratch dir.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
golden="$root/plugin/tests/golden/frames"

rendered=${1:-}
cleanup=
if [ -z "$rendered" ]; then
  rendered=$(mktemp -d /tmp/vtframes.XXXXXX)
  cleanup=1
  (cd "$root/plugin" && VTABS_DUMP_FRAMES="$rendered" lua tests/run.lua) || {
    echo "lua suite failed"
    exit 1
  }
fi

diffout=$(mktemp /tmp/vtframes-diff.XXXXXX)
if diff -ru "$golden" "$rendered" >"$diffout" 2>&1; then
  echo "ok: frames match $golden"
  [ -z "$cleanup" ] || rm -rf "$rendered"
  rm -f "$diffout"
else
  echo "FRAMES MISMATCH against $golden"
  echo
  cat "$diffout"
  rm -f "$diffout"
  [ -z "$cleanup" ] || echo "kept re-rendered output at $rendered"
  exit 1
fi
