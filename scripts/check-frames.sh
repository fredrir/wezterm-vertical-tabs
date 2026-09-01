#!/bin/sh
# The oracle: renders every committed scene through the Rust renderer and byte-compares both the
# text and styled dumps against plugin/tests/golden/frames. No display needed.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
golden="$root/plugin/tests/golden/frames"

# The cross-language gate: the Rust renderer must reproduce every scene's dumps byte-for-byte.
command -v cargo >/dev/null 2>&1 || { echo "cargo not found"; exit 1; }
{
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
}
