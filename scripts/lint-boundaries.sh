#!/bin/sh
# §3 rule 1: no ANSI escape bytes outside vtabs-runtime, vtabs-input's decoder and ratatui.
# plugin/ still hosts the frozen v1 renderer; its files leave this list as the phases delete them.
set -eu
cd "$(dirname "$0")/.."

check() { # dir
  grep -rln $'\x1b\|\\\\x1b\|\\\\033\|\\\\27' "$1" 2>/dev/null || true
}

bad=""
for crate in vtabs-protocol vtabs-core vtabs-theme vtabs-view; do
  [ -d "backend/crates/$crate/src" ] || continue
  hits=$(check "backend/crates/$crate/src")
  [ -z "$hits" ] || bad="$bad $hits"
done

[ -z "$bad" ] || { echo "ANSI escapes outside their home:$bad"; exit 1; }
echo "boundaries ok"
