#!/bin/sh

set -eu
# shellcheck source=scripts/lib.sh
# shellcheck disable=SC1091
. "$(dirname "$0")/lib.sh"

mux_socket=${VTABS_RESTART_MUX_SOCKET:-$data/wezterm/localmux.sock}

usage() {
  cat <<EOF
  just ls [--panes | --tabs | --windows]
EOF
}

mux_cli() {
  WEZTERM_UNIX_SOCKET="$mux_socket" \
    wezterm cli --prefer-mux --no-auto-start "$@"
}

list_panes() {
  mux_cli list
}

pane_row() {
  pane=$1
  list_panes 2>/dev/null | awk -v pane="$pane" 'NR > 1 && $3 == pane { print; found = 1 } END { exit !found }'
}

pane_count() {
  panes=$(list_panes 2>/dev/null) || return 1
  printf '%s\n' "$panes" | awk 'NR > 1 && NF { count++ } END { print count + 0 }'
}

vtab_pane_rows() {
  panes=$(list_panes 2>/dev/null) || return 1
  printf '%s\n' "$panes" | awk '
    NR > 1 && $3 ~ /^[0-9]+$/ &&
      ($6 ~ /^wez-vtabs:[[:xdigit:]]+$/ || $6 ~ /^wez-vtabs-settings:[[:xdigit:]]+$/) { print }
  '
}

list_panes
