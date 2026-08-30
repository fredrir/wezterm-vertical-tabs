#!/bin/sh
# Reports which backend is actually running and whether the installs agree.
set -eu
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"

row() { printf '  %-22s %s\n' "$1" "$2"; }
stamp() { [ -e "$1" ] && printf '%s  %s' "$(wc -c <"$1" | tr -d ' ')B" "$(date -r "$1" '+%Y-%m-%d %H:%M')" || printf '%s' "-"; }

say "${dim}source${off}"
row "root" "$root"
row "branch" "$(git -C "$root" rev-parse --abbrev-ref HEAD) $(git -C "$root" diff --quiet 2>/dev/null && echo clean || echo dirty)"
row "version.lua" "$version"
row "Cargo.toml" "$(sed -n 's/^version *= *"\(.*\)"$/\1/p' "$root/backend/Cargo.toml" | head -1)"
row "latest tag" "$(git -C "$root" describe --tags --abbrev=0 2>/dev/null || echo none)"

say ""
say "${dim}builds${off}"
row "target/debug" "$(stamp "$root/backend/target/debug/$name")"
row "target/release" "$(stamp "$root/backend/target/release/$name")"

say ""
say "${dim}installs${off}"
if [ -d "$checkout" ]; then
  row "plugin dir" "$checkout"
  row "  version.lua" "$(sed -n 's/^return "\(.*\)"$/\1/p' "$checkout/plugin/vtabs/version.lua" 2>/dev/null || echo '-')"
else
  row "plugin dir" "not installed ${dim}(just deploy --from-prd)${off}"
fi
if [ -d "$cache" ] && [ -n "$(ls -A "$cache" 2>/dev/null)" ]; then
  for f in "$cache"/*; do row "  cached" "$(basename "$f")  $(stamp "$f")"; done
else
  row "bootstrap cache" "empty"
fi

say ""
say "${dim}your wezterm config${off}"
cfg="${WEZTERM_CONFIG_DIR:-$HOME/.config/wezterm}"
# Resolve symlinks: BSD grep -r will not descend into a symlinked directory.
cfg=$(cd "$cfg" 2>/dev/null && pwd -P) || cfg=""
if [ -n "$cfg" ] && grep -rl "vtabs" "$cfg" >/dev/null 2>&1; then
  grep -rn --exclude-dir=lua_modules --exclude-dir=.luarocks --exclude-dir=types \
    -e "wez-vertical-tabs" -e "vtabs.apply_to_config" "$cfg" 2>/dev/null |
    sed "s|$cfg|<config>|g; s|$HOME|~|g" | head -8 |
    while IFS= read -r l; do printf '  %s\n' "$l"; done
else
  row "wiring" "no vtabs reference found in $cfg"
fi

say ""
say "${dim}running backends${off}"
found=0
for pid in $(pgrep -f "$name" 2>/dev/null || true); do
  cmd=$(ps -p "$pid" -o args= 2>/dev/null || true)
  case "$cmd" in
    *watchexec* | *doctor.sh* | *dev.sh*) continue ;;
  esac
  [ -n "$cmd" ] || continue
  found=$((found + 1))
  row "pid $pid" "$(printf '%s' "$cmd" | sed "s|$HOME|~|g")"
done
[ "$found" -eq 0 ] && row "" "none running"
exit 0
