#!/bin/sh
# Applies a build to your WezTerm: working tree (default), a real plugin-dir install, or the published release.
set -eu
# shellcheck source=scripts/lib.sh
. "$(dirname "$0")/lib.sh"

mode=dev
while [ $# -gt 0 ]; do
  case "$1" in
  --from-dev | dev) mode=dev ;;
  --from-prd | prd) mode=prd ;;
  --from-release | release) mode=release ;;
  -h | --help)
    say "usage: just deploy [--from-dev|--from-prd|--from-release]"
    exit 0
    ;;
  *) die "unknown flag: $1" ;;
  esac
  shift
done

# git narrates annotated-tag clones and detached HEAD; only surface it on failure.
clone_quiet() {
  src=$1
  dst=$2
  shift 2
  if ! err=$(git -c advice.detachedHead=false clone --quiet "$@" "$src" "$dst" 2>&1); then
    printf '%s\n' "$err" >&2
    return 1
  fi
}

wipe_checkout() {
  case "$checkout" in
  "$plugins"/?*) rm -rf "$checkout" ;;
  *) die "refusing to remove $checkout" ;;
  esac
}

install_cached() {
  mkdir -p "$cache"
  cp "$1" "$cache/$name-$triple-$version"
  chmod 755 "$cache/$name-$triple-$version"
  cached=$cache/$name-$triple-$version
}

case "$mode" in
dev)
  build release || die "build failed"
  bin=$(bin_for release)
  ok "deployed $dim$bin$off"
  restart_sidebars "$bin"
  say "${dim}your config already points here (plugins/vtabs.lua backend.path)${off}"
  ;;

prd)
  if [ "$triple" = unknown ]; then die "unsupported host: $(uname -sm)"; fi
  git -C "$root" diff --quiet || warn "working tree is dirty; installing committed HEAD only"
  build release || die "build failed"

  wipe_checkout
  mkdir -p "$plugins"
  clone_quiet "$root" "$checkout"
  git -C "$checkout" remote set-url origin "$url"
  install_cached "$(bin_for release)"

  ok "installed $dim$checkout$off"
  say "  binary  $dim$cached$off"
  say "  commit  $dim$(git -C "$checkout" rev-parse --short HEAD)$off"
  say ""
  say "Point your config at the real install to exercise it:"
  say "  ${dim}local vtabs = wezterm.plugin.require \"$url\"${off}"
  restart_sidebars "$cached"
  ;;

release)
  if [ "$triple" = unknown ]; then die "unsupported host: $(uname -sm)"; fi
  command -v curl >/dev/null 2>&1 || die "curl not found"
  tmp=$(mktemp -d /tmp/vtabs-rel.XXXXXX)
  trap 'rm -rf "$tmp"' EXIT

  # Resolve GitHub's latest *published* release. The checked-out branch may be
  # ahead of or behind it, so plugin/vtabs/version.lua is not authoritative here.
  latest="$url/releases/latest/download/SHA256SUMS"
  resolved=$(curl -fsSI "$latest" | sed -n 's/^[Ll]ocation:[[:space:]]*//p' | tr -d '\r' | head -1)
  tag=$(printf '%s\n' "$resolved" | sed -n 's|.*/releases/download/\(v[^/]*\)/SHA256SUMS.*|\1|p')
  printf '%s\n' "$tag" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$' \
    || die "could not resolve a semantic release tag from $resolved"
  version=${tag#v}
  base="$url/releases/download/$tag"

  say "${dim}fetching $base/$name-$triple${off}"
  curl -fsSL -o "$tmp/SHA256SUMS" "$resolved" || die "SHA256SUMS download failed"
  curl -fsSL -o "$tmp/$name-$triple" "$base/$name-$triple" || die "download failed (is v$version published?)"
  expected=$(grep " $name-$triple\$" "$tmp/SHA256SUMS" | cut -d' ' -f1)
  actual=$(shasum -a 256 "$tmp/$name-$triple" | cut -d' ' -f1)
  [ -n "$expected" ] && [ "$expected" = "$actual" ] || die "checksum mismatch"
  ok "checksum verified $dim$(printf %s "$actual" | cut -c1-12)…$off"

  chmod 755 "$tmp/$name-$triple"
  install_cached "$tmp/$name-$triple"

  wipe_checkout
  mkdir -p "$plugins"
  clone_quiet "$url" "$checkout" --branch "v$version" || die "clone of v$version failed"

  ok "installed published v$version $dim$checkout$off"
  say "  binary  $dim$cached$off"
  restart_sidebars "$cached"
  ;;
esac
