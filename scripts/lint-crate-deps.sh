#!/bin/sh
set -eu
cd "$(dirname "$0")/../backend"

# crate | forbidden dependency names, comma-separated
RULES="vtabs-protocol|crossterm,ratatui,tachyonfx,vtabs-input,vtabs-runtime,vtabs-zen
vtabs-input|crossterm,ratatui,tachyonfx,vtabs-runtime,vtabs-zen
vtabs-core|ratatui,crossterm,tachyonfx
vtabs-theme|ratatui,crossterm,tachyonfx,vtabs-input,vtabs-runtime,serde_json
vtabs-view|crossterm"

fail=0
echo "$RULES" | while IFS='|' read -r crate forbidden; do
  cargo tree -p "$crate" -e normal --prefix none >/tmp/vtabs-tree.$$ 2>/dev/null || continue
  for dep in $(echo "$forbidden" | tr ',' ' '); do
    if grep -q "^$dep " /tmp/vtabs-tree.$$; then
      echo "FORBIDDEN: $crate -> $dep"
      fail=1
    fi
  done
  rm -f /tmp/vtabs-tree.$$
  [ "$fail" -eq 0 ] || exit 1
done

# vtabs-zen: no dependency at all
deps=$(cargo tree -p vtabs-zen -e normal --prefix none | grep -cv '^vtabs-zen ' || true)
[ "$deps" -eq 0 ] || {
  echo "FORBIDDEN: vtabs-zen has dependencies"
  cargo tree -p vtabs-zen -e normal
  exit 1
}
echo "crate deps ok"
