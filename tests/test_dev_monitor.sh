#!/bin/sh
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d /tmp/vtabs-monitor-test.XXXXXX)
burn_pid=
cleanup() {
  [ -z "$burn_pid" ] || kill "$burn_pid" 2>/dev/null || true
  [ -z "$burn_pid" ] || wait "$burn_pid" 2>/dev/null || true
  rm -rf "$tmp"
}
trap cleanup EXIT INT TERM

quiet=$(
  VTABS_DEV_MEMORY_WARN_MB=1048576 \
    VTABS_DEV_CPU_WARN_PERCENT=0 \
    VTABS_DEV_PROCESS_WARN=0 \
    VTABS_DEV_DISK_WARN_MB=0 \
    sh "$root/scripts/dev-monitor.sh" --once "$tmp" "test:$$" 2>&1
)
[ -z "$quiet" ] || {
  printf 'healthy monitor was noisy: %s\n' "$quiet" >&2
  exit 1
}

warning=$(
  VTABS_DEV_MEMORY_WARN_MB=1 \
    VTABS_DEV_CPU_WARN_PERCENT=0 \
    VTABS_DEV_PROCESS_WARN=0 \
    VTABS_DEV_DISK_WARN_MB=0 \
    sh "$root/scripts/dev-monitor.sh" --once "$tmp" "test:$$" 2>&1
)
case "$warning" in
*"warning: high dev memory:"*"top:"*) ;;
*)
  printf 'memory warning missing context: %s\n' "$warning" >&2
  exit 1
  ;;
esac

dead=$(
  VTABS_DEV_MEMORY_WARN_MB=0 \
    VTABS_DEV_CPU_WARN_PERCENT=0 \
    VTABS_DEV_PROCESS_WARN=0 \
    VTABS_DEV_DISK_WARN_MB=0 \
    sh "$root/scripts/dev-monitor.sh" --once "$tmp" "gui:99999999" 2>&1
)
case "$dead" in
*"error: development gui exited unexpectedly"*) ;;
*)
  printf 'missing process-exit diagnostic: %s\n' "$dead" >&2
  exit 1
  ;;
esac

sh -c 'while :; do :; done' &
burn_pid=$!
sleep 1
cpu_warning=$(
  VTABS_DEV_MEMORY_WARN_MB=0 \
    VTABS_DEV_CPU_GRACE_SECONDS=0 \
    VTABS_DEV_PROCESS_WARN=0 \
    VTABS_DEV_DISK_WARN_MB=0 \
    sh "$root/scripts/dev-monitor.sh" --once "$tmp" "burn:$burn_pid" 2>&1
)
kill "$burn_pid" 2>/dev/null || true
wait "$burn_pid" 2>/dev/null || true
burn_pid=
case "$cpu_warning" in
*"warning: high dev CPU:"*"100% is one fully used core"*) ;;
*)
  printf 'CPU warning missing context: %s\n' "$cpu_warning" >&2
  exit 1
  ;;
esac

printf 'dev monitor tests passed\n'
