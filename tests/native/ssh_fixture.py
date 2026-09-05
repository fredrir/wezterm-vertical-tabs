#!/usr/bin/env python3
"""Run SSH-mux scenarios through an isolated, unprivileged localhost sshd."""

from __future__ import annotations

import argparse
import json
import os
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path


class LocalSshMux:
    def __init__(self, root: Path, cli: Path, server: Path):
        self.root = root
        self.cli, self.server = cli, server
        self.processes, self.logs = [], []
        self.domain = root / "ssh.json"

    def launch(self, args, name, env=None):
        log = (self.root / name).open("wb")
        self.logs.append(log)
        process = subprocess.Popen(
            args,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=log,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        self.processes.append(process)
        return process

    def start(self):
        if os.name != "posix":
            raise RuntimeError("localhost sshd fixture requires a POSIX host")
        import pwd

        user = pwd.getpwuid(os.getuid()).pw_name
        sshd = shutil.which("sshd") or "/usr/sbin/sshd"
        if not Path(sshd).is_file():
            raise RuntimeError("sshd is unavailable; provide an existing disposable SSH fixture")
        self.root.mkdir(parents=True, exist_ok=True)
        self.root.chmod(0o700)
        for name in ("config", "cache", "data", "state", "runtime", "bin"):
            (self.root / name).mkdir(exist_ok=True)
        (self.root / "runtime").chmod(0o700)
        for source in (self.cli, self.server):
            shutil.copy2(source, self.root / "bin" / source.name)
        cli = self.root / "bin" / self.cli.name
        server = self.root / "bin" / self.server.name
        for name in ("host_key", "client_key"):
            subprocess.run(
                [
                    "ssh-keygen",
                    "-q",
                    "-t",
                    "ed25519",
                    "-N",
                    "",
                    "-f",
                    str(self.root / name),
                ],
                check=True,
            )
        (self.root / "authorized_keys").write_bytes((self.root / "client_key.pub").read_bytes())
        (self.root / "authorized_keys").chmod(0o600)
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        (self.root / "known_hosts").write_text(
            f"[127.0.0.1]:{port} " + (self.root / "host_key.pub").read_text()
        )
        settings = {
            "Port": port,
            "ListenAddress": "127.0.0.1",
            "HostKey": self.root / "host_key",
            "PidFile": self.root / "sshd.pid",
            # The owned fixture is mode0700; permit a sticky /tmp ancestor on macOS.
            "AuthorizedKeysFile": self.root / "authorized_keys",
            "StrictModes": "no",
            "PubkeyAuthentication": "yes",
            "AuthenticationMethods": "publickey",
            "PasswordAuthentication": "no",
            "KbdInteractiveAuthentication": "no",
            "UsePAM": "no",
            "AllowUsers": user,
            "AllowAgentForwarding": "no",
            "AllowTcpForwarding": "no",
            "X11Forwarding": "no",
            "PrintMotd": "no",
            "UseDNS": "no",
            "LogLevel": "VERBOSE",
        }
        (self.root / "sshd_config").write_text("".join(f"{k} {v}\n" for k, v in settings.items()))
        daemon = self.launch([sshd, "-D", "-e", "-f", str(self.root / "sshd_config")], "sshd.log")
        self.ssh = [
            "ssh",
            "-F",
            "/dev/null",
            "-i",
            str(self.root / "client_key"),
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "BatchMode=yes",
            "-o",
            f"UserKnownHostsFile={self.root}/known_hosts",
            "-o",
            "GlobalKnownHostsFile=/dev/null",
            "-o",
            "StrictHostKeyChecking=yes",
            "-p",
            str(port),
            f"{user}@127.0.0.1",
        ]
        deadline = time.monotonic() + 5
        while True:
            if daemon.poll() is not None:
                raise RuntimeError((self.root / "sshd.log").read_text())
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                    break
            except OSError as error:
                if time.monotonic() > deadline:
                    raise RuntimeError("isolated sshd did not begin listening") from error
                time.sleep(0.05)
        subprocess.run(
            self.ssh + ["printf VTABS_SSH_OK"],
            check=True,
            timeout=10,
            capture_output=True,
        )
        socket_path = self.root / "mux.sock"
        config = self.root / "mux.lua"
        config.write_text(
            "return { unix_domains={{name='ssh-fixture',socket_path="
            + json.dumps(str(socket_path))
            + ",no_serve_automatically=true}},default_prog={'/bin/sh'},check_for_updates=false }\n"
        )
        remote = {
            f"XDG_{name}_HOME": str(self.root / name.lower())
            for name in ("CONFIG", "CACHE", "DATA", "STATE")
        }
        remote.update(
            {
                "XDG_RUNTIME_DIR": str(self.root / "runtime"),
                "WEZTERM_UNIX_SOCKET": str(socket_path),
                "PATH": str(self.root / "bin") + ":/usr/bin:/bin:/usr/sbin:/sbin",
            }
        )
        env = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith(("WEZTERM_", "WEZ_VTABS_"))
        }
        env.update(remote)
        mux = self.launch([str(server), "--config-file", str(config)], "mux.log", env)
        deadline = time.monotonic() + 10
        while not socket_path.exists():
            if mux.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError(
                    "isolated mux startup failed: " + (self.root / "mux.log").read_text()
                )
            time.sleep(0.05)
        command = ["env"] + [f"{key}={value}" for key, value in remote.items()]
        command += [
            str(cli),
            "--config-file",
            str(config),
            "cli",
            "--prefer-mux",
            "--no-auto-start",
        ]
        result = subprocess.run(
            self.ssh + [shlex.join(command + ["list", "--format", "json"])],
            check=True,
            timeout=10,
            capture_output=True,
            text=True,
        )
        json.loads(result.stdout)
        self.domain.write_text(
            json.dumps(
                {
                    "name": "scenario-ssh",
                    "remote_address": f"127.0.0.1:{port}",
                    "username": user,
                    "no_agent_auth": True,
                    "multiplexing": "WezTerm",
                    "remote_wezterm_path": str(cli),
                    "override_proxy_command": shlex.join(command + ["proxy"]),
                    "ssh_option": {
                        "identityfile": str(self.root / "client_key"),
                        "identitiesonly": "yes",
                        "userknownhostsfile": str(self.root / "known_hosts"),
                        "globalknownhostsfile": "/dev/null",
                        "stricthostkeychecking": "yes",
                        "proxycommand": "none",
                    },
                },
                indent=2,
            )
        )
        return self.domain

    def close(self):
        try:
            for process in reversed(self.processes):
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    pass
                finally:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    process.wait(timeout=5)
        finally:
            for log in self.logs:
                log.close()
            for name in ("host_key", "client_key"):
                (self.root / name).unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--wezterm", required=True, type=Path, help="CLI binary from the tested build"
    )
    parser.add_argument("--mux-server", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument(
        "scenario",
        nargs=argparse.REMAINDER,
        help="scenarios.py arguments after --; omit to probe SSH-mux only",
    )
    args = parser.parse_args()
    root = (args.output or Path(tempfile.mkdtemp(prefix="vtabs-localhost-ssh-"))).resolve()
    fixture = LocalSshMux(
        root,
        args.wezterm.resolve(),
        (args.mux_server or args.wezterm.with_name("wezterm-mux-server")).resolve(),
    )
    try:
        config = fixture.start()
        print(f"SSH-mux authenticated; fixture: {root}", flush=True)
        scenario = args.scenario[1:] if args.scenario[:1] == ["--"] else args.scenario
        if scenario:
            subprocess.run(
                [
                    sys.executable,
                    str(Path(__file__).with_name("scenarios.py")),
                    "--domain",
                    "ssh",
                    "--ssh-config",
                    str(config),
                ]
                + scenario,
                check=True,
            )
    finally:
        fixture.close()


if __name__ == "__main__":
    main()
