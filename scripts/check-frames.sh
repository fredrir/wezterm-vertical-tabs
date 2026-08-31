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

# The cross-language gate: the Rust renderer must reproduce every scene's dumps byte-for-byte.
if command -v cargo >/dev/null 2>&1; then
  rust_dir=$(mktemp -d /tmp/vtframes-rust.XXXXXX)
  (cd "$root/backend" && cargo build -q -p wez-vtabs) || { echo "backend build failed"; exit 1; }
  "$root/backend/target/debug/wez-vtabs" dump-frames "$root/plugin/tests/golden/scenes" "$rust_dir"
  rust_fail=0
  for f in "$rust_dir"/*; do
    name=$(basename "$f")
    if ! diff -u "$golden/$name" "$f"; then
      rust_fail=1
    fi
  done
  if [ "$rust_fail" -ne 0 ]; then
    echo "RUST RENDERER MISMATCH against $golden"
    exit 1
  fi
  echo "ok: rust renderer matches $golden ($(ls "$rust_dir" | wc -l) dumps)"
  rm -rf "$rust_dir"
else
  echo "cargo not found; rust renderer pass skipped"
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
