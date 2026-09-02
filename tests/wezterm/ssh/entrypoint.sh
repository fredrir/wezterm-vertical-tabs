#!/bin/sh
set -eu

test -n "${VTABS_AUTHORIZED_KEY:-}"
install -d -m 0700 -o vtabs -g vtabs /home/vtabs/.ssh
printf '%s\n' "$VTABS_AUTHORIZED_KEY" > /home/vtabs/.ssh/authorized_keys
chown vtabs:vtabs /home/vtabs/.ssh/authorized_keys
chmod 0600 /home/vtabs/.ssh/authorized_keys
ssh-keygen -A

exec /usr/sbin/sshd -D -e
