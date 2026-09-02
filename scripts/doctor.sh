#!/bin/sh
# Reports which backend is actually running and whether the installs agree.
set -eu
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"

row() { printf '  %-22s %s\n' "$1" "$2"; }
stamp() { [ -e "$1" ] && printf '%s  %s' "$(wc -c <"$1" | tr -d ' ')B" "$(date -r "$1" '+%Y-%m-%d %H:%M')" || printf '%s' "-"; }

say "${dim}source${off}"
row "root" "$root"
state=clean
git -C "$root" diff --quiet 2>/dev/null && git -C "$root" diff --cached --quiet 2>/dev/null || state=dirty
row "branch" "$(git -C "$root" rev-parse --abbrev-ref HEAD) $state"
row "version.lua" "$version"
row "Cargo.toml" "$(sed -n 's/^version *= *"\(.*\)"$/\1/p' "$root/backend/Cargo.toml" | head -1)"
latest_tag=$(git -C "$root" tag --list 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname | head -1)
row "latest tag" "${latest_tag:-none}"

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
say "${dim}crashes${off}"
last=$(ls -1dt "$devlogs"/*/ 2>/dev/null | head -1)
if [ -n "$last" ]; then
  row "last dev logs" "$(printf '%s' "$last" | sed "s|$HOME|~|")"
  grep -h 'panic at' "$last"* 2>/dev/null | head -3 | while IFS= read -r l; do printf '  %s\n' "$l"; done
else
  row "last dev logs" "none ${dim}(just dev)${off}"
fi
reports="$HOME/Library/Logs/DiagnosticReports"
if [ -d "$reports" ]; then
  latest=$(ls -1t "$reports"/wezterm-gui-*.ips 2>/dev/null | head -1 | sed "s|$HOME|~|")
  row "crash report" "${latest:-none}"
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
