"""WezTerm CLI clients and owned mux servers on private Unix sockets."""

from __future__ import annotations

import json
import subprocess
from collections.abc import Mapping, Sequence
from pathlib import Path

from tests.support.environment import clean_env
from tests.support.polling import check_process, log_diagnostics, wait_until
from tests.support.process import spawn_logged, stop_process


class WeztermCli:
    def __init__(self, wezterm: Path, config: Path, socket: Path, env: Mapping[str, str]):
        self.socket = socket
        self.command = [str(wezterm), "--config-file", str(config), "cli", "--no-auto-start"]
        self.env = {**env, "WEZTERM_UNIX_SOCKET": str(socket)}

    def __call__(self, *args, env: Mapping[str, str] | None = None) -> str:
        return subprocess.run(
            [*self.command, *map(str, args)],
            env={**self.env, **(env or {})},
            capture_output=True,
            text=True,
            check=True,
            timeout=20,
        ).stdout.strip()

    def panes(self) -> list[dict]:
        return json.loads(self("list", "--format", "json"))

    def adoptable(self, domain: str) -> list[dict]:
        return json.loads(self("adopt-pane", "--domain-name", domain, "--list", "--format", "json"))

    def proxy_domain(self, name: str, **options) -> dict:
        # Stands in for `ssh -T HOST wezterm cli proxy`.
        proxy = ["/usr/bin/env", f"WEZTERM_UNIX_SOCKET={self.socket}", *self.command, "proxy"]
        return {"name": name, "proxy_command": proxy, **options}


class MuxServer:
    def __init__(
        self,
        root: Path,
        binaries: Mapping[str, Path],
        environment: Mapping[str, str],
        clients: Sequence[dict],
        unix_domains: Sequence[dict] = (),
    ):
        self.root, self.binaries = root, binaries
        root.mkdir()
        self.socket = root / "mux.sock"
        self.config = root / "mux.lua"
        own = {"name": "fixture", "socket_path": str(self.socket), "no_serve_automatically": True}
        self.config.write_text(
            "local wezterm = require 'wezterm'\nreturn {"
            "check_for_updates=false,automatically_reload_config=false,"
            "default_prog={'/bin/sh'},initial_cols=120,initial_rows=40,"
            "unix_domains=wezterm.json_parse([=["
            + json.dumps([own, *unix_domains])
            + "]=]),tls_clients=wezterm.json_parse([=["
            + json.dumps(list(clients))
            + "]=])}\n"
        )
        self.env = {**clean_env(environment), "WEZTERM_UNIX_SOCKET": str(self.socket)}
        self.cli = WeztermCli(binaries["wezterm"], self.config, self.socket, self.env)
        self.start()

    def start(self):
        self.process = spawn_logged(
            [self.binaries["wezterm-mux-server"], "--config-file", self.config],
            self.root / "mux.log",
            env=self.env,
        )
        wait_until(
            self.socket.exists,
            timeout=10,
            message="mux socket did not become ready",
            check_live=lambda: check_process(self.process, "mux server"),
            diagnostics=lambda: log_diagnostics(self.root / "mux.log"),
        )

    def restart(self):
        """Stands in for a reboot: local shells die, remote shells outlive their proxies."""
        self.close()
        self.socket.unlink(missing_ok=True)
        self.start()

    def close(self):
        stop_process(self.process)
