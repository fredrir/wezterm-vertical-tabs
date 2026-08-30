#!/bin/sh
# Locates the wez-vtabs backend: explicit path, cached download, verified GitHub release, or cargo build.
set -u

name="wez-vtabs"
data="${XDG_DATA_HOME:-$HOME/.local/share}/wez-vtabs"
target="${VTABS_TARGET:-}"
version="${VTABS_VERSION:-dev}"
PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
export PATH

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

safe() { printf '%s' "$1" | grep -Eq '^[A-Za-z0-9._-]+$'; }
if ! safe "$target" || ! safe "$version"; then
  printf 'invalid VTABS_TARGET or VTABS_VERSION\n'
  exit 1
fi

bin="$data/bin/$name-$target-$version"
[ -x "$bin" ] && exec "$bin"
mkdir -p "$data/bin"

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

download() {
  base="https://github.com/$VTABS_REPO/releases/download/v$version"
  tmp=$(mktemp "$data/bin/.$name.XXXXXX") || return 1
  sums="$tmp.sums"
  printf 'downloading %s\n' "$base/$name-$target"
  if curl -fsSL -o "$tmp" "$base/$name-$target" && curl -fsSL -o "$sums" "$base/SHA256SUMS"; then
    expected=$(grep " $name-$target\$" "$sums" | cut -d' ' -f1)
    actual=$(sha256 "$tmp")
    if [ -n "$expected" ] && [ "$expected" = "$actual" ]; then
      chmod +x "$tmp" && mv "$tmp" "$bin" && rm -f "$sums" && return 0
    fi
    printf 'checksum mismatch\n'
  fi
  rm -f "$tmp" "$sums"
  return 1
}

if [ "$version" != dev ] && [ -n "${VTABS_REPO:-}" ] && command -v curl >/dev/null 2>&1; then
  download && exec "$bin"
  printf 'download failed\n'
fi

if [ "${VTABS_BUILD:-1}" = 1 ] && [ -f "${VTABS_SRC:-}/Cargo.toml" ] && command -v cargo >/dev/null 2>&1; then
  printf 'building backend\n'
  if cargo build --release --manifest-path "$VTABS_SRC/Cargo.toml" --target-dir "$data/target"; then
    cp "$data/target/release/$name" "$bin" && exec "$bin"
  fi
  printf 'build failed\n'
fi

printf 'backend not found: install cargo, publish a release, or set backend.path\n'
exit 1
