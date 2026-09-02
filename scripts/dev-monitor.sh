#!/bin/sh
# Warn when the WezTerm sandbox or one of its process trees becomes unhealthy.
set -eu

usage() {
  printf '%s\n' "usage: dev-monitor.sh [--once] SANDBOX_HOME ROLE:PID..." >&2
  exit 2
}

once=0
if [ "${1:-}" = --once ]; then
  once=1
  shift
fi

[ "$#" -ge 2 ] || usage
sandbox_home=$1
shift
root_specs=$*
root_pids=
for spec do
  case "$spec" in
  *:*) ;;
  *) usage ;;
  esac
  role=${spec%%:*}
  pid=${spec#*:}
  case "$role:$pid" in
  :* | *: | *:*[!0-9]*) usage ;;
  *) root_pids="$root_pids $pid" ;;
  esac
done

memory_warn_mb=${VTABS_DEV_MEMORY_WARN_MB:-1024}
cpu_warn_percent=${VTABS_DEV_CPU_WARN_PERCENT:-40}
cpu_grace_seconds=${VTABS_DEV_CPU_GRACE_SECONDS:-3}
process_warn=${VTABS_DEV_PROCESS_WARN:-64}
disk_warn_mb=${VTABS_DEV_DISK_WARN_MB:-256}
sample_seconds=${VTABS_DEV_SAMPLE_SECONDS:-1}

for setting in "$memory_warn_mb" "$cpu_warn_percent" "$cpu_grace_seconds" "$process_warn" "$disk_warn_mb" "$sample_seconds"; do
  case "$setting" in
  '' | *[!0-9]*)
    printf 'error: dev resource thresholds must be non-negative integers\n' >&2
    exit 2
    ;;
  esac
done
[ "$sample_seconds" -gt 0 ] || {
  printf 'error: VTABS_DEV_SAMPLE_SECONDS must be greater than zero\n' >&2
  exit 2
}

memory_warn_kb=$((memory_warn_mb * 1024))
disk_warn_kb=$((disk_warn_mb * 1024))
started_at=$(date +%s)
memory_reported=0
cpu_reported=0
previous_cpu_seconds=
previous_sample_at=
process_reported=0
disk_reported=0
dead_roots=

warn() { printf 'warning: %s\n' "$*" >&2; }
error() { printf 'error: %s\n' "$*" >&2; }

while :; do
  [ ! -e "$sandbox_home/.stopping" ] || exit 0
  live_roots=0
  for spec in $root_specs; do
    role=${spec%%:*}
    pid=${spec#*:}
    if kill -0 "$pid" 2>/dev/null; then
      live_roots=$((live_roots + 1))
    else
      [ ! -e "$sandbox_home/.stopping" ] || exit 0
      case " $dead_roots " in
      *" $pid "*) ;;
      *)
        error "development $role exited unexpectedly (PID $pid); stopping and restarting just dev may be required"
        dead_roots="$dead_roots $pid"
        ;;
      esac
    fi
  done

  snapshot=$(LC_ALL=C ps -axo pid=,ppid=,rss=,%cpu=,time=,comm= 2>/dev/null) || {
    warn "resource monitor could not read the process table"
    exit 1
  }
  stats=$(printf '%s\n' "$snapshot" | awk -v roots="$root_pids" '
    function cpu_seconds(value, parts, count, days) {
      days = 0
      count = split(value, parts, "-")
      if (count == 2) {
        days = parts[1]
        value = parts[2]
      }
      count = split(value, parts, ":")
      if (count == 2) return days * 86400 + parts[1] * 60 + parts[2]
      if (count == 3) return days * 86400 + parts[1] * 3600 + parts[2] * 60 + parts[3]
      return 0
    }
    BEGIN {
      count = split(roots, root, " ")
      for (i = 1; i <= count; i++)
        if (root[i] != "") owned[root[i]] = 1
    }
    {
      rows++
      pid[rows] = $1
      ppid[rows] = $2
      rss[rows] = $3
      cpu[rows] = $4
      cpu_time[rows] = cpu_seconds($5)
    }
    END {
      do {
        changed = 0
        for (i = 1; i <= rows; i++)
          if (!owned[pid[i]] && owned[ppid[i]]) {
            owned[pid[i]] = 1
            changed = 1
          }
      } while (changed)

      for (i = 1; i <= rows; i++)
        if (owned[pid[i]]) {
          processes++
          total_rss += rss[i]
          total_cpu += cpu[i]
          total_cpu_time += cpu_time[i]
          if (rss[i] > top_rss) {
            top_rss = rss[i]
            top_rss_pid = pid[i]
          }
          if (cpu[i] > top_cpu) {
            top_cpu = cpu[i]
            top_cpu_pid = pid[i]
          }
        }
      printf "%.0f %.0f %d %d %.0f %d %.0f %.2f\n", total_rss, total_cpu, processes,
        top_rss_pid, top_rss, top_cpu_pid, top_cpu, total_cpu_time
    }
  ')
  # shellcheck disable=SC2086 # stats is exactly eight whitespace-delimited numeric fields
  set -- $stats
  total_rss_kb=${1:-0}
  total_cpu=${2:-0}
  process_count=${3:-0}
  top_rss_pid=${4:-0}
  top_rss_kb=${5:-0}
  top_cpu_pid=${6:-0}
  top_recent_cpu=${7:-0}
  total_cpu_seconds=${8:-0}
  total_rss_mb=$((total_rss_kb / 1024))
  top_rss_mb=$((top_rss_kb / 1024))
  top_rss_name=$(LC_ALL=C ps -p "$top_rss_pid" -o comm= 2>/dev/null | awk 'NR == 1 { sub(/^.*\//, ""); print; exit }')
  [ -n "$top_rss_name" ] || top_rss_name="PID $top_rss_pid"
  top_cpu_name=$(LC_ALL=C ps -p "$top_cpu_pid" -o comm= 2>/dev/null | awk 'NR == 1 { sub(/^.*\//, ""); print; exit }')
  [ -n "$top_cpu_name" ] || top_cpu_name="PID $top_cpu_pid"

  sampled_at=$(date +%s)
  sampled_cpu=
  if [ -n "$previous_cpu_seconds" ] && [ "$sampled_at" -gt "$previous_sample_at" ]; then
    sampled_cpu=$(awk -v current="$total_cpu_seconds" -v previous="$previous_cpu_seconds" \
      -v elapsed="$((sampled_at - previous_sample_at))" 'BEGIN {
        delta = current - previous
        if (delta < 0) delta = 0
        printf "%.0f", delta * 100 / elapsed
      }')
  elif [ "$once" -eq 1 ]; then
    # One-shot mode exists for diagnostics/tests and has no earlier CPU-time baseline.
    sampled_cpu=$total_cpu
  fi
  previous_cpu_seconds=$total_cpu_seconds
  previous_sample_at=$sampled_at

  if [ "$memory_warn_kb" -gt 0 ] && [ "$total_rss_kb" -ge "$memory_warn_kb" ]; then
    if [ "$memory_reported" -eq 0 ]; then
      warn "high dev memory: ${total_rss_mb} MiB across $process_count processes (limit ${memory_warn_mb} MiB; top: $top_rss_name PID $top_rss_pid at ${top_rss_mb} MiB)"
      memory_reported=1
    fi
  elif [ "$memory_reported" -eq 1 ] && [ "$total_rss_kb" -lt $((memory_warn_kb * 4 / 5)) ]; then
    memory_reported=0
  fi

  cpu_age=$((sampled_at - started_at))
  if [ -n "$sampled_cpu" ] && [ "$cpu_age" -ge "$cpu_grace_seconds" ] && \
    [ "$cpu_warn_percent" -gt 0 ] && [ "$sampled_cpu" -ge "$cpu_warn_percent" ]; then
    if [ "$cpu_reported" -eq 0 ]; then
      warn "high dev CPU: ${sampled_cpu}% across $process_count processes (limit ${cpu_warn_percent}%; top recent average: $top_cpu_name PID $top_cpu_pid at ${top_recent_cpu}%; 100% is one fully used core)"
      cpu_reported=1
    fi
  elif [ -n "$sampled_cpu" ] && [ "$cpu_age" -ge "$cpu_grace_seconds" ]; then
    if [ "$sampled_cpu" -lt $((cpu_warn_percent * 3 / 4)) ]; then
      cpu_reported=0
    fi
  fi

  if [ "$process_warn" -gt 0 ] && [ "$process_count" -ge "$process_warn" ]; then
    if [ "$process_reported" -eq 0 ]; then
      warn "large dev process tree: $process_count processes (limit $process_warn)"
      process_reported=1
    fi
  elif [ "$process_reported" -eq 1 ] && [ "$process_count" -lt $((process_warn * 4 / 5)) ]; then
    process_reported=0
  fi

  disk_kb=$(du -sk "$sandbox_home" 2>/dev/null | awk 'NR == 1 { print $1 }')
  disk_kb=${disk_kb:-0}
  if [ "$disk_warn_kb" -gt 0 ] && [ "$disk_kb" -ge "$disk_warn_kb" ]; then
    if [ "$disk_reported" -eq 0 ]; then
      warn "large dev sandbox: $((disk_kb / 1024)) MiB on disk (limit ${disk_warn_mb} MiB)"
      disk_reported=1
    fi
  elif [ "$disk_reported" -eq 1 ] && [ "$disk_kb" -lt $((disk_warn_kb * 4 / 5)) ]; then
    disk_reported=0
  fi

  [ "$once" -eq 1 ] && exit 0
  [ "$live_roots" -gt 0 ] || exit 1
  sleep "$sample_seconds"
done
