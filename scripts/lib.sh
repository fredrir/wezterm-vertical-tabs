# Shared paths and helpers for the dev scripts.
# shellcheck shell=sh disable=SC2034  # consumed by the scripts that source this
root="${VTABS_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
name=wez-vtabs
url=https://github.com/fredrir/wezterm-vertical-tabs
data="${XDG_DATA_HOME:-$HOME/.local/share}"
plugins="$data/wezterm/plugins"
cache="$data/$name/bin"
devlogs="${XDG_STATE_HOME:-$HOME/.local/state}/$name/dev-logs"

# WezTerm mangles the repo URL into the plugin dir name: / -> sZs, : -> sCs, . -> sDs.
component=$(printf '%s' "$url" | sed -e 's|/|sZs|g' -e 's|:|sCs|g' -e 's|\.|sDs|g')
checkout="$plugins/$component"

version=$(sed -n 's/^return "\(.*\)"$/\1/p' "$root/plugin/vtabs/version.lua")

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) triple=aarch64-apple-darwin ;;
  Darwin-x86_64) triple=x86_64-apple-darwin ;;
  Linux-x86_64) triple=x86_64-unknown-linux-gnu ;;
  Linux-aarch64 | Linux-arm64) triple=aarch64-unknown-linux-gnu ;;
  *) triple=unknown ;;
esac

if [ -t 1 ]; then
  dim=$(printf '\033[2m'); red=$(printf '\033[31m'); grn=$(printf '\033[32m')
  ylw=$(printf '\033[33m'); off=$(printf '\033[0m')
else
  dim=; red=; grn=; ylw=; off=
fi

say() { printf '%s\n' "$*"; }
ok() { printf '%s%s%s\n' "$grn" "$*" "$off"; }
warn() { printf '%s%s%s\n' "$ylw" "$*" "$off"; }
die() { printf '%s%s%s\n' "$red" "$*" "$off" >&2; exit 1; }

build() {
  profile=$1
  start=$(date +%s)
  status=0
  if [ "$profile" = release ]; then
    cargo build --quiet --release --locked --manifest-path "$root/backend/Cargo.toml" \
      >/dev/null 2>"$root/.dev-build.log" || status=$?
  else
    cargo build --quiet --locked --manifest-path "$root/backend/Cargo.toml" \
      >/dev/null 2>"$root/.dev-build.log" || status=$?
  fi
  elapsed=$(( $(date +%s) - start ))
  if [ $status -ne 0 ]; then
    printf '%sbuild failed%s (%ss)\n' "$red" "$off" "$elapsed"
    tail -20 "$root/.dev-build.log"
    return 1
  fi
  # With --quiet, a successful Cargo build writes only actionable compiler diagnostics.
  [ ! -s "$root/.dev-build.log" ] || cat "$root/.dev-build.log" >&2
  [ "${VTABS_QUIET_SUCCESS:-0}" = 1 ] || \
    printf '%sbuild ok%s %s(%ss, %s)%s\n' "$grn" "$off" "$dim" "$elapsed" "$profile" "$off"
}

bin_for() {
  [ "$1" = release ] && printf '%s' "$root/backend/target/release/$name" \
    || printf '%s' "$root/backend/target/debug/$name"
}

# Sidebars respawn on the next poll, so killing the backend is a hot-swap.
restart_sidebars() {
  n=$(pgrep -xf "$1" 2>/dev/null | wc -l | tr -d ' ')
  if [ "$n" -gt 0 ]; then
    pkill -xf "$1" 2>/dev/null || true
    [ "${VTABS_QUIET_SUCCESS:-0}" = 1 ] || \
      printf '%s  swapped %s sidebar(s)%s\n' "$dim" "$n" "$off"
  else
    [ "${VTABS_QUIET_SUCCESS:-0}" = 1 ] || \
      printf '%s  no running sidebars%s\n' "$dim" "$off"
  fi
}
