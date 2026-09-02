#!/bin/sh
set -eu

export VTABS_LOG=/tmp/wez-vtabs-e2e.log
exec /opt/vtabs/wez-vtabs "$@"
