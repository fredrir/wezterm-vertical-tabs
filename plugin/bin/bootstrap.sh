#!/bin/sh
# Locates the wez-vtabs backend: explicit path, cached download, GitHub release, or cargo build.
set -u

name="wez-vtabs"
data="${XDG_DATA_HOME:-$HOME/.local/share}/wez-vtabs"
target="${VTABS_TARGET:-}"
version="${VTABS_VERSION:-dev}"

if [ -n "${VTABS_BIN:-}" ] && [ -x "$VTABS_BIN" ]; then
  exec "$VTABS_BIN"
fi

if [ -z "$target" ]; then
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) target=aarch64-apple-darwin ;;
    Darwin-x86_64) target=x86_64-apple-darwin ;;
    Linux-x86_64) target=x86_64-unknown-linux-gnu ;;
    Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-gnu ;;
    *) target=unknown ;;
  esac
fi

bin="$data/bin/$name-$target-$version"
[ -x "$bin" ] && exec "$bin"
mkdir -p "$data/bin"

if [ "$version" != dev ] && [ -n "${VTABS_REPO:-}" ] && command -v curl >/dev/null 2>&1; then
  url="https://github.com/$VTABS_REPO/releases/download/v$version/$name-$target"
  printf 'downloading %s\n' "$url"
  if curl -fsSL -o "$bin.tmp" "$url"; then
    chmod +x "$bin.tmp" && mv "$bin.tmp" "$bin" && exec "$bin"
  fi
  rm -f "$bin.tmp"
  printf 'download failed\n'
fi

if [ "${VTABS_BUILD:-1}" = 1 ] && [ -f "${VTABS_SRC:-}/Cargo.toml" ] && command -v cargo >/dev/null 2>&1; then
  printf 'building backend\n'
  if cargo build --release --manifest-path "$VTABS_SRC/Cargo.toml" --target-dir "$data/target"; then
    cp "$data/target/release/$name" "$bin" && exec "$bin"
  fi
  printf 'build failed\n'
fi

printf 'backend not found\ninstall cargo or set backend.path\n'
while :; do sleep 3600; done
