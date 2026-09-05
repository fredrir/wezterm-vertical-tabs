#!/bin/sh
set -eu
mkdir -p /run/sshd /home/mux/.ssh /tmp/mux
cp /fixture-key.pub /home/mux/.ssh/authorized_keys
chmod 700 /home/mux/.ssh /tmp/mux
chmod 600 /home/mux/.ssh/authorized_keys
chown -R mux:mux /home/mux/.ssh /tmp/mux
ssh-keygen -A
cat > /tmp/mux/mux.lua <<'LUA'
return {
  check_for_updates = false,
  default_prog = {'/bin/sh'},
  unix_domains = {{name = 'container-mux', socket_path = '/tmp/mux/mux.sock', no_serve_automatically = true}},
}
LUA
runuser -u mux -- /opt/wezterm/wezterm-mux-server --config-file /tmp/mux/mux.lua > /tmp/mux.log 2>&1 &
cat > /tmp/sshd_config <<'SSH'
Port 2222
ListenAddress 0.0.0.0
HostKey /etc/ssh/ssh_host_ed25519_key
PidFile /tmp/sshd.pid
AuthorizedKeysFile .ssh/authorized_keys
PasswordAuthentication no
KbdInteractiveAuthentication no
AuthenticationMethods publickey
PubkeyAuthentication yes
UsePAM no
AllowUsers mux
AllowAgentForwarding no
AllowTcpForwarding no
X11Forwarding no
PermitRootLogin no
PrintMotd no
LogLevel VERBOSE
ForceCommand env WEZTERM_UNIX_SOCKET=/tmp/mux/mux.sock /opt/wezterm/wezterm --config-file /tmp/mux/mux.lua cli --no-auto-start proxy
SSH
exec /usr/bin/sshd -D -e -f /tmp/sshd_config
