"""Isolated real-WezTerm process support for the opt-in smoke test."""

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import uuid
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest


ROOT = Path(__file__).resolve().parents[2]
E2E_CONFIG = ROOT / "plugin" / "tests" / "wezterm-e2e.lua"
BACKEND_OVERRIDE = "WEZ_VTABS_BIN"


def _backend_path() -> Path:
    configured = os.environ.get(BACKEND_OVERRIDE)
    candidate = (
        Path(configured)
        if configured
        else ROOT / "backend" / "target" / "debug" / "wez-vtabs"
    )
    try:
        binary = candidate.expanduser().resolve(strict=True)
    except FileNotFoundError:
        pytest.skip(
            f"source-built backend not found at {candidate}; build it once or set {BACKEND_OVERRIDE}"
        )
    if not binary.is_file() or not os.access(binary, os.X_OK):
        pytest.skip(f"source-built backend is not executable: {binary}")
    return binary


def _wezterm_path() -> Path:
    executable = shutil.which("wezterm")
    if executable is None:
        pytest.skip("wezterm executable is not installed")
    return Path(executable).resolve()


def _require_display() -> None:
    if sys.platform == "linux" and not (
        os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")
    ):
        pytest.skip("real WezTerm smoke test needs DISPLAY or WAYLAND_DISPLAY")
    if sys.platform not in {"darwin", "linux"}:
        pytest.skip("real WezTerm smoke test currently supports macOS and Linux")


class WezTermInstance:
    """One GUI and its private CLI identity, directories, panes, and process group."""

    def __init__(self, root: Path, wezterm: Path, backend: Path) -> None:
        token = uuid.uuid4().hex
        self.identity = f"vtabs-e2e-{token}"
        self.root = root
        self.wezterm = wezterm
        self.backend = backend
        self.process: subprocess.Popen[bytes] | None = None
        self._log_handle: Any = None

        self.home = root / "home"
        self.config = root / "config"
        self.cache = root / "cache"
        self.data = root / "data"
        self.state = root / "state"
        self.runtime = root / "runtime"
        self.logs = root / "logs"
        for directory in (
            self.home,
            self.config,
            self.cache,
            self.data,
            self.state,
            self.runtime,
            self.logs,
            self.home / ".local" / "share" / "wezterm",
        ):
            directory.mkdir(parents=True)
        self.runtime.chmod(0o700)

        # In particular, don't inherit WEZTERM_PANE or WEZTERM_UNIX_SOCKET from a
        # developer running pytest inside WezTerm. The private runtime dir and
        # unique class make it impossible for our CLI calls to select their GUI.
        self.env = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith("WEZTERM_") and not key.startswith("VTABS_")
        }
        self.env.update(
            {
                "HOME": str(self.home),
                "XDG_CONFIG_HOME": str(self.config),
                "XDG_CACHE_HOME": str(self.cache),
                "XDG_DATA_HOME": str(self.data),
                "XDG_STATE_HOME": str(self.state),
                "XDG_RUNTIME_DIR": str(self.runtime),
                "WEZTERM_CONFIG_DIR": str(self.config / "wezterm"),
                "WEZTERM_LOG": "info",
                "VTABS_ROOT": str(ROOT),
                "VTABS_BIN": str(self.backend),
                "VTABS_LOG": str(self.logs / "wez-vtabs.log"),
                "VTABS_E2E_DOMAIN": "CurrentPaneDomain",
            }
        )

    @property
    def gui_log(self) -> Path:
        return self.logs / "wezterm.log"

    def start(self) -> None:
        self._log_handle = self.gui_log.open("wb")
        command = [
            str(self.wezterm),
            "--config-file",
            str(E2E_CONFIG),
            "start",
            "--always-new-process",
            "--no-auto-connect",
            "--class",
            self.identity,
            "--workspace",
            self.identity,
        ]
        self.process = subprocess.Popen(
            command,
            cwd=ROOT,
            env=self.env,
            stdin=subprocess.DEVNULL,
            stdout=self._log_handle,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )

    def _cli(self, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
        command = [
            str(self.wezterm),
            "cli",
            "--no-auto-start",
            "--class",
            self.identity,
            *args,
        ]
        cli_env = self.env.copy()
        gui_sockets = sorted(
            (self.home / ".local" / "share" / "wezterm").glob("gui-sock-*")
        )
        if len(gui_sockets) > 1:
            raise AssertionError(f"isolated WezTerm created multiple GUI sockets: {gui_sockets}")
        if gui_sockets:
            # Selecting the socket explicitly is both safer and more portable
            # than relying on platform-specific class-to-socket discovery.
            cli_env["WEZTERM_UNIX_SOCKET"] = str(gui_sockets[0])
        return subprocess.run(
            command,
            cwd=ROOT,
            env=cli_env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            check=check,
            text=True,
            timeout=2,
        )

    def panes(self) -> list[dict[str, Any]]:
        result = self._cli("list", "--format", "json")
        parsed = json.loads(result.stdout)
        if not isinstance(parsed, list):
            raise AssertionError(f"wezterm cli returned a non-list pane model: {parsed!r}")
        return parsed

    def text(self, pane_id: int) -> str:
        return self._cli("get-text", "--pane-id", str(pane_id)).stdout

    def spawn_tab(self, pane_id: int) -> None:
        self._cli("spawn", "--pane-id", str(pane_id), "/bin/sh")

    def wait_for(
        self,
        description: str,
        predicate: Callable[[], Any],
        *,
        timeout: float = 10,
        consecutive: int = 1,
    ) -> Any:
        deadline = time.monotonic() + timeout
        last_error: Exception | None = None
        successes = 0
        while time.monotonic() < deadline:
            if self.process is not None and self.process.poll() is not None:
                raise AssertionError(
                    f"WezTerm exited with {self.process.returncode} while waiting for {description}\n"
                    f"{self.log_tail()}"
                )
            try:
                value = predicate()
                if value:
                    successes += 1
                    if successes >= consecutive:
                        return value
                else:
                    successes = 0
            except (
                json.JSONDecodeError,
                OSError,
                subprocess.SubprocessError,
            ) as error:
                last_error = error
                successes = 0
            time.sleep(0.1)
        detail = ""
        if isinstance(last_error, subprocess.CalledProcessError):
            stderr = (last_error.stderr or "").strip()
            detail = f"; last CLI error: {stderr or last_error}"
        elif last_error is not None:
            detail = f"; last transient error: {last_error}"
        raise AssertionError(f"timed out waiting for {description}{detail}\n{self.log_tail()}")

    def wait_for_panes(self) -> list[dict[str, Any]]:
        return self.wait_for("the isolated WezTerm CLI", self.panes, timeout=15)

    def log_tail(self, line_count: int = 80) -> str:
        if self._log_handle is not None:
            self._log_handle.flush()
        try:
            lines = self.gui_log.read_text(errors="replace").splitlines()
        except OSError as error:
            return f"unable to read {self.gui_log}: {error}"
        return "\n".join(lines[-line_count:])

    def close(self) -> None:
        process = self.process
        if process is None:
            if self._log_handle is not None:
                self._log_handle.close()
                self._log_handle = None
            return

        # Ask only this instance's private mux to close its own panes. This lets
        # shells and backend processes exit normally before the GUI is stopped.
        if process.poll() is None:
            try:
                for pane in self.panes():
                    pane_id = pane.get("pane_id")
                    if isinstance(pane_id, int):
                        self._cli("kill-pane", "--pane-id", str(pane_id), check=False)
            except (OSError, ValueError, subprocess.SubprocessError):
                pass

        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            # start_new_session makes this process-group id ours. Never use a
            # name-based kill: another WezTerm process may belong to the user.
            try:
                if os.getpgid(process.pid) == process.pid:
                    os.killpg(process.pid, signal.SIGTERM)
                else:
                    process.terminate()
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                try:
                    if os.getpgid(process.pid) == process.pid:
                        os.killpg(process.pid, signal.SIGKILL)
                    else:
                        process.kill()
                except ProcessLookupError:
                    pass
                process.wait(timeout=3)
        finally:
            if self._log_handle is not None:
                self._log_handle.close()
                self._log_handle = None


@pytest.fixture
def wezterm_instance() -> WezTermInstance:
    _require_display()
    # WezTerm's Unix-domain socket path has a small platform limit. pytest's
    # nested tmp_path can exceed it before WezTerm adds `.local/share/wezterm`.
    with tempfile.TemporaryDirectory(prefix="vt-", dir="/tmp") as directory:
        instance = WezTermInstance(Path(directory), _wezterm_path(), _backend_path())
        try:
            instance.start()
            instance.wait_for_panes()
            yield instance
        finally:
            instance.close()
