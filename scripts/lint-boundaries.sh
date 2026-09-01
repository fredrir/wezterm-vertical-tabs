#!/bin/sh
# §3 rule 1: no ANSI escape bytes outside vtabs-runtime and vtabs-input's decoder — in Rust or Lua.
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

# plugin/ builds no frames any more; the two patterns that classify forwarded key bytes are the
# only escapes it may name.
lua_hits=$(check plugin/vtabs plugin/init.lua | grep -v '^plugin/vtabs/input.lua$' || true)
[ -z "$lua_hits" ] || bad="$bad $lua_hits"
input_escapes=$(grep -cF '\27' plugin/vtabs/input.lua || true)
[ "$input_escapes" -le 2 ] || bad="$bad plugin/vtabs/input.lua(+$input_escapes)"

[ -z "$bad" ] || { echo "ANSI escapes outside their home:$bad"; exit 1; }
echo "boundaries ok"
