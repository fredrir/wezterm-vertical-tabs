"""Disposable SSH mux servers in containers bound only to host loopback."""

from __future__ import annotations

import hashlib
import json
import os
import platform
import shutil
import subprocess
import time
import uuid
from pathlib import Path

_ENGINE_XDG = {
    key: os.environ.get(key)
    for key in (
        "XDG_RUNTIME_DIR",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
    )
}


class ContainerSshMux:
    def __init__(self, root: Path, binaries: dict[str, Path], project: Path, env: dict[str, str]):
        self.root, self.binaries, self.project = root, binaries, project
        self.env = env
        # The engine owns its storage and namespace files. Keep those out of pytest's
        # temporary tree; only the explicitly named container and bind mounts are ours.
        self.engine_env = env.copy()
        for key, value in _ENGINE_XDG.items():
            if value is not None:
                self.engine_env[key] = value
            else:
                self.engine_env.pop(key, None)
        self.runtime = (
            os.environ.get("VTABS_TEST_CONTAINER_RUNTIME")
            or shutil.which("podman")
            or shutil.which("docker")
        )
        self.name = "wez-vtabs-ssh-" + uuid.uuid4().hex
        self.started = False
        self.run_attempted = False

    def command(self, *args, check=True, timeout=30):
        result = subprocess.run(
            [str(self.runtime), *map(str, args)],
            env=self.engine_env,
            capture_output=True,
            text=True,
            check=False,
            timeout=timeout,
        )
        if check and result.returncode:
            raise RuntimeError(
                f"container command failed ({result.returncode}): {' '.join(map(str, args))}\n{result.stdout}\n{result.stderr}"
            )
        return result

    def start(self) -> Path:
        if not self.runtime:
            raise RuntimeError("container SSH tests require Podman or Docker")
        self.command("info", timeout=15)
        self.root.mkdir(mode=0o700, parents=True)
        staged = self.root / "bin"
        staged.mkdir()
        for name in ("wezterm", "wezterm-mux-server"):
            shutil.copy2(self.binaries[name], staged / name)
        self.key = self.root / "client_key"
        subprocess.run(
            ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(self.key)],
            env=self.env,
            check=True,
            capture_output=True,
            timeout=10,
        )
        image = self.image()
        self.run_attempted = True
        self.command(
            "run",
            "--detach",
            "--rm",
            "--name",
            self.name,
            "--publish",
            "127.0.0.1::2222",
            "--mount",
            f"type=bind,src={staged},dst=/opt/wezterm,ro",
            "--mount",
            f"type=bind,src={self.key}.pub,dst=/fixture-key.pub,ro",
            image,
        )
        self.started = True
        address = self.command("port", self.name, "2222/tcp").stdout.strip()
        self.port = int(address.rsplit(":", 1)[1])
        deadline = time.monotonic() + 20
        while True:
            ready = self.command("exec", self.name, "test", "-S", "/tmp/mux/mux.sock", check=False)
            if ready.returncode == 0:
                break
            if time.monotonic() >= deadline:
                log = self.command("exec", self.name, "cat", "/tmp/mux.log", check=False)
                raise RuntimeError("container mux failed to start: " + log.stdout + log.stderr)
            time.sleep(0.1)
        host_key = self.command(
            "exec", self.name, "cat", "/etc/ssh/ssh_host_ed25519_key.pub"
        ).stdout
        self.known_hosts = self.root / "known_hosts"
        self.known_hosts.write_text(f"[127.0.0.1]:{self.port} {host_key}", encoding="utf-8")
        domain = self.root / "ssh.json"
        domain.write_text(
            json.dumps(
                {
                    "name": "scenario-ssh",
                    "remote_address": f"127.0.0.1:{self.port}",
                    "username": "mux",
                    "multiplexing": "WezTerm",
                    "no_agent_auth": True,
                    "remote_wezterm_path": "/opt/wezterm/wezterm",
                    "override_proxy_command": "/opt/wezterm/wezterm --config-file /tmp/mux/mux.lua cli --no-auto-start proxy",
                    "ssh_option": {
                        "identityfile": str(self.key),
                        "identitiesonly": "yes",
                        "userknownhostsfile": str(self.known_hosts),
                        "globalknownhostsfile": "/dev/null",
                        "stricthostkeychecking": "yes",
                        "proxycommand": "none",
                    },
                }
            ),
            encoding="utf-8",
        )
        return domain

    def image(self) -> str:
        image = os.environ.get("VTABS_TEST_SSH_IMAGE")
        if image is not None:
            self.command("image", "inspect", image)
            return image
        context = self.project / "tests/containers/ssh"
        release = platform.freedesktop_os_release()
        base = os.environ.get("VTABS_TEST_SSH_BASE")
        if base is None:
            distribution = release.get("ID")
            if distribution == "arch":
                base = "docker.io/library/archlinux:base"
            elif distribution in {"ubuntu", "debian"} and release.get("VERSION_ID"):
                base = f"docker.io/library/{distribution}:{release['VERSION_ID']}"
            else:
                raise RuntimeError(
                    "set VTABS_TEST_SSH_BASE to an Arch, Debian, or Ubuntu image compatible with the native binaries"
                )
        digest = hashlib.sha256(
            base.encode()
            + b"".join((context / name).read_bytes() for name in ("Dockerfile", "entrypoint.sh"))
        ).hexdigest()[:16]
        image = "localhost/wez-vtabs-ssh:" + digest
        if self.command("image", "inspect", image, check=False).returncode:
            result = self.command(
                "build", "--build-arg", f"BASE_IMAGE={base}", "--tag", image, context, timeout=300
            )
            (self.root / "build.log").write_text(result.stdout + result.stderr, encoding="utf-8")
        return image

    def panes(self) -> list[dict]:
        result = self.command(
            "exec",
            "--user",
            "mux",
            "--env",
            "WEZTERM_UNIX_SOCKET=/tmp/mux/mux.sock",
            self.name,
            "/opt/wezterm/wezterm",
            "--config-file",
            "/tmp/mux/mux.lua",
            "cli",
            "--no-auto-start",
            "list",
            "--format",
            "json",
        )
        return json.loads(result.stdout)

    def reject_unknown_key(self) -> None:
        key = self.root / "rejected_key"
        subprocess.run(
            ["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)],
            env=self.env,
            check=True,
            capture_output=True,
            timeout=10,
        )
        result = subprocess.run(
            [
                "ssh",
                "-F",
                "/dev/null",
                "-i",
                str(key),
                "-p",
                str(self.port),
                "-o",
                "BatchMode=yes",
                "-o",
                "IdentitiesOnly=yes",
                "-o",
                "StrictHostKeyChecking=yes",
                "-o",
                f"UserKnownHostsFile={self.known_hosts}",
                "-o",
                "GlobalKnownHostsFile=/dev/null",
                "mux@127.0.0.1",
                "true",
            ],
            env=self.env,
            capture_output=True,
            text=True,
            timeout=10,
        )
        assert result.returncode == 255, result.stderr
        assert "Permission denied" in result.stderr, result.stderr

    def close(self) -> None:
        try:
            if self.run_attempted:
                try:
                    result = self.command("logs", self.name, check=False)
                    (self.root / "container.log").write_text(
                        result.stdout + result.stderr, encoding="utf-8"
                    )
                except (OSError, subprocess.SubprocessError):
                    pass
                finally:
                    self.command("rm", "--force", self.name, check=False)
                remaining = self.command(
                    "ps", "--all", "--filter", f"name={self.name}", "--format", "{{.Names}}"
                )
                if self.name in remaining.stdout.splitlines():
                    raise RuntimeError(f"container cleanup failed: {self.name}")
                self.started = False
                self.run_attempted = False
        finally:
            for name in ("client_key", "rejected_key"):
                (self.root / name).unlink(missing_ok=True)
