"""Owned real-WezTerm processes for black-box mux-domain tests."""

from __future__ import annotations

import base64
import json
import os
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from collections import defaultdict
from collections.abc import Callable, Iterator, Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Literal

import pytest

ROOT = Path(__file__).resolve().parents[2]
BACKEND = ROOT / "backend"
E2E_CONFIG = ROOT / "plugin" / "tests" / "wezterm-e2e.lua"
SSH_IMAGE_DIR = Path(__file__).parent / "ssh"
BACKEND_OVERRIDE = "WEZ_VTABS_BIN"
Endpoint = Literal["gui", "mux"]


@dataclass(frozen=True)
class Toolchain:
    wezterm: Path
    mux_server: Path
    backend: Path
    version: str


@dataclass(frozen=True)
class E2EOptions:
    """Per-module environment and diagnostics added to an owned WezTerm instance."""

    environment: Mapping[str, str]
    capture: Callable[[Path], None] | None = None


@dataclass(frozen=True)
class SshMuxContainer:
    docker: Path
    name: str
    port: int
    identity_file: Path

    def capture(self, log_dir: Path) -> None:
        log_dir.mkdir(parents=True, exist_ok=True)
        container = subprocess.run(
            [self.docker, "logs", self.name],
            capture_output=True,
            text=True,
            timeout=10,
        )
        (log_dir / "ssh-container.log").write_text(container.stdout + container.stderr)
        backend = subprocess.run(
            [
                self.docker,
                "exec",
                self.name,
                "sh",
                "-c",
                "test ! -f /tmp/wez-vtabs-e2e.log || cat /tmp/wez-vtabs-e2e.log",
            ],
            capture_output=True,
            text=True,
            timeout=10,
        )
        (log_dir / "ssh-backend.log").write_text(backend.stdout + backend.stderr)


@dataclass(frozen=True)
class PaneSnapshot:
    """The stable, public subset of one `wezterm cli list --format json` row."""

    window_id: int
    tab_id: int
    pane_id: int
    workspace: str
    title: str
    cwd: str
    cols: int
    rows: int

    @classmethod
    def from_json(cls, value: Mapping[str, Any]) -> PaneSnapshot:
        size = value.get("size")
        if not isinstance(size, Mapping):
            size = {}

        def integer(name: str, source: Mapping[str, Any] = value) -> int:
            item = source.get(name)
            if not isinstance(item, int):
                raise AssertionError(f"pane row has no integer {name!r}: {value!r}")
            return item

        return cls(
            window_id=integer("window_id"),
            tab_id=integer("tab_id"),
            pane_id=integer("pane_id"),
            workspace=str(value.get("workspace", "")),
            title=str(value.get("title", "")),
            cwd=str(value.get("cwd", "")),
            cols=integer("cols", size),
            rows=integer("rows", size),
        )

    @property
    def is_sidebar(self) -> bool:
        return self.title.startswith("wez-vtabs:")

    def as_dict(self) -> dict[str, Any]:
        return {
            "window_id": self.window_id,
            "tab_id": self.tab_id,
            "pane_id": self.pane_id,
            "workspace": self.workspace,
            "title": self.title,
            "cwd": self.cwd,
            "cols": self.cols,
            "rows": self.rows,
        }


@dataclass(frozen=True)
class TabSnapshot:
    tab_id: int
    panes: tuple[PaneSnapshot, ...]

    @property
    def sidebars(self) -> tuple[PaneSnapshot, ...]:
        return tuple(pane for pane in self.panes if pane.is_sidebar)

    @property
    def content(self) -> tuple[PaneSnapshot, ...]:
        return tuple(pane for pane in self.panes if not pane.is_sidebar)


@dataclass(frozen=True)
class Topology:
    panes: tuple[PaneSnapshot, ...]

    @classmethod
    def from_json(cls, value: Any) -> Topology:
        if not isinstance(value, list):
            raise AssertionError(f"wezterm CLI returned a non-list pane model: {value!r}")
        panes = tuple(PaneSnapshot.from_json(row) for row in value if isinstance(row, Mapping))
        if len(panes) != len(value):
            raise AssertionError(f"wezterm CLI returned an invalid pane row: {value!r}")
        return cls(tuple(sorted(panes, key=lambda pane: pane.pane_id)))

    @property
    def pane_ids(self) -> frozenset[int]:
        return frozenset(pane.pane_id for pane in self.panes)

    @property
    def tabs(self) -> tuple[TabSnapshot, ...]:
        grouped: dict[int, list[PaneSnapshot]] = defaultdict(list)
        for pane in self.panes:
            grouped[pane.tab_id].append(pane)
        return tuple(
            TabSnapshot(tab_id, tuple(sorted(panes, key=lambda pane: pane.pane_id)))
            for tab_id, panes in sorted(grouped.items())
        )

    @property
    def shape(self) -> tuple[tuple[int, int], ...]:
        """Per-tab (sidebar, content) counts, comparable across mux proxy ID spaces."""

        return tuple(sorted((len(tab.sidebars), len(tab.content)) for tab in self.tabs))

    def tab(self, tab_id: int) -> TabSnapshot:
        try:
            return next(tab for tab in self.tabs if tab.tab_id == tab_id)
        except StopIteration as error:
            raise AssertionError(f"tab {tab_id} is absent from {self}") from error

    def pane(self, pane_id: int) -> PaneSnapshot:
        try:
            return next(pane for pane in self.panes if pane.pane_id == pane_id)
        except StopIteration as error:
            raise AssertionError(f"pane {pane_id} is absent from {self}") from error

    def as_list(self) -> list[dict[str, Any]]:
        return [pane.as_dict() for pane in self.panes]


def _executable(path: Path, description: str) -> Path:
    try:
        resolved = path.expanduser().resolve(strict=True)
    except FileNotFoundError:
        pytest.fail(f"{description} not found at {path}")
    if not resolved.is_file() or not os.access(resolved, os.X_OK):
        pytest.fail(f"{description} is not executable: {resolved}")
    return resolved


def _version(path: Path) -> str:
    try:
        result = subprocess.run(
            [path, "--version"],
            check=True,
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (OSError, subprocess.SubprocessError) as error:
        pytest.fail(f"could not query {path} version: {error}")
    output = (result.stdout or result.stderr).strip()
    if not output:
        pytest.fail(f"{path} --version returned no version")
    return output.split()[-1]


@pytest.fixture(scope="session")
def wezterm_toolchain() -> Toolchain:
    """Resolve and validate the test binaries once for the whole GUI run."""

    if sys.platform not in {"darwin", "linux"}:
        pytest.fail("real mux-domain tests currently support macOS and Linux")
    if sys.platform == "linux" and not (
        os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")
    ):
        pytest.fail("real mux-domain tests require DISPLAY or WAYLAND_DISPLAY")

    wezterm_name = shutil.which("wezterm")
    if wezterm_name is None:
        pytest.fail("wezterm is required for real mux-domain tests")
    wezterm = _executable(Path(wezterm_name), "wezterm")

    sibling = wezterm.with_name("wezterm-mux-server")
    mux_name = shutil.which("wezterm-mux-server")
    mux_server = _executable(
        sibling if sibling.is_file() else Path(mux_name or "wezterm-mux-server"),
        "wezterm-mux-server",
    )

    configured = os.environ.get(BACKEND_OVERRIDE)
    candidate = (
        Path(configured)
        if configured
        else BACKEND / "target" / "debug" / ("wez-vtabs.exe" if os.name == "nt" else "wez-vtabs")
    )
    backend = _executable(candidate, "source-built wez-vtabs backend")

    wezterm_version = _version(wezterm)
    mux_version = _version(mux_server)
    if wezterm_version != mux_version:
        pytest.fail(
            "wezterm and wezterm-mux-server versions differ: "
            f"{wezterm_version!r} != {mux_version!r}"
        )
    return Toolchain(wezterm, mux_server, backend, wezterm_version)


@pytest.fixture(scope="session")
def wezterm_artifact_dir() -> Path:
    configured = os.environ.get("VTABS_TEST_ARTIFACTS")
    return Path(configured).expanduser() if configured else ROOT / ".pytest-artifacts"


@pytest.fixture(scope="session")
def ssh_mux_image() -> tuple[Path, str]:
    """Build the pinned localhost SSH-domain image only for the explicit SSH lane."""

    if os.environ.get("VTABS_SSH_E2E") != "1":
        pytest.skip("set VTABS_SSH_E2E=1 or use `just test-wezterm-ssh-e2e`")
    if sys.platform != "linux":
        pytest.fail("the containerized SSH mux lane requires a Linux host backend")
    docker_name = shutil.which("docker")
    if docker_name is None:
        pytest.fail("docker is required for the SSH mux-domain tests")
    docker = _executable(Path(docker_name), "docker")
    image = "vertical-tabs-wezterm-ssh-e2e:20240203"
    command = [docker, "build", "--tag", image]
    for variable, argument in (
        ("VTABS_SSH_WEZTERM_DEB_URL", "WEZTERM_DEB_URL"),
        ("VTABS_SSH_WEZTERM_DEB_SHA256", "WEZTERM_DEB_SHA256"),
    ):
        value = os.environ.get(variable)
        if value:
            command.extend(("--build-arg", f"{argument}={value}"))
    command.append(SSH_IMAGE_DIR)
    result = subprocess.run(
        command,
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=600,
    )
    if result.returncode != 0:
        pytest.fail(f"could not build SSH mux image:\n{result.stdout}\n{result.stderr}")
    return docker, image


@pytest.fixture
def ssh_mux_container(
    ssh_mux_image: tuple[Path, str],
    wezterm_toolchain: Toolchain,
    tmp_path: Path,
) -> Iterator[SshMuxContainer]:
    """Run one SSH server and one remote WezTerm installation for a single test."""

    docker, image = ssh_mux_image
    ssh_keygen_name = shutil.which("ssh-keygen")
    if ssh_keygen_name is None:
        pytest.fail("ssh-keygen is required for the SSH mux-domain tests")
    ssh_keygen = _executable(Path(ssh_keygen_name), "ssh-keygen")
    identity_file = tmp_path / "id_ed25519"
    subprocess.run(
        [ssh_keygen, "-q", "-t", "ed25519", "-N", "", "-f", identity_file],
        check=True,
        timeout=15,
    )
    public_key = identity_file.with_suffix(".pub").read_text().strip()
    name = f"vtabs-ssh-e2e-{uuid.uuid4().hex}"
    command = [
        docker,
        "run",
        "--detach",
        "--name",
        name,
        "--publish",
        "127.0.0.1::22",
        "--env",
        f"VTABS_AUTHORIZED_KEY={public_key}",
        "--volume",
        f"{wezterm_toolchain.backend}:/opt/vtabs/wez-vtabs:ro",
        image,
    ]
    started = subprocess.run(command, capture_output=True, text=True, timeout=30)
    if started.returncode != 0:
        # `docker run` can leave an exited, named container behind after a partial startup.
        # The UUID makes this exact target ours, so remove it before reporting the setup failure.
        subprocess.run(
            [docker, "rm", "--force", name],
            capture_output=True,
            text=True,
            timeout=15,
        )
        pytest.fail(f"could not start SSH mux container:\n{started.stdout}\n{started.stderr}")

    container: SshMuxContainer | None = None
    try:
        deadline = time.monotonic() + 20
        last_error: Exception | None = None
        while time.monotonic() < deadline:
            try:
                port_result = subprocess.run(
                    [docker, "port", name, "22/tcp"],
                    check=True,
                    capture_output=True,
                    text=True,
                    timeout=3,
                )
                port = int(port_result.stdout.strip().rsplit(":", 1)[-1])
                with socket.create_connection(("127.0.0.1", port), timeout=1) as connection:
                    if not connection.recv(64).startswith(b"SSH-"):
                        raise RuntimeError("container did not send an SSH banner")
                container = SshMuxContainer(docker, name, port, identity_file)
                break
            except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
                last_error = error
                time.sleep(0.1)
        if container is None:
            pytest.fail(f"SSH mux container was not ready: {last_error!r}")

        version = subprocess.run(
            [docker, "exec", name, "wezterm", "--version"],
            check=True,
            capture_output=True,
            text=True,
            timeout=10,
        ).stdout.strip()
        if not version.endswith(wezterm_toolchain.version):
            pytest.fail(
                "host and SSH-container WezTerm versions differ: "
                f"{wezterm_toolchain.version!r} != {version!r}"
            )
        yield container
    finally:
        subprocess.run(
            [docker, "stop", "--time", "5", name],
            capture_output=True,
            text=True,
            timeout=15,
        )
        removed = subprocess.run(
            [docker, "rm", "--force", name],
            capture_output=True,
            text=True,
            timeout=15,
        )
        if removed.returncode != 0 and "No such container" not in removed.stderr:
            raise AssertionError(f"could not remove owned SSH container {name}: {removed.stderr}")


class CliError(RuntimeError):
    pass


class WezTermMuxInstance:
    """A standalone mux server and one reconnectable GUI client, owned by one test."""

    def __init__(
        self,
        root: Path,
        toolchain: Toolchain,
        extra_environment: Mapping[str, str] | None = None,
    ) -> None:
        token = uuid.uuid4().hex
        self.identity = f"vtabs-e2e-{token}"
        self.root = root
        self.toolchain = toolchain
        self.mux_socket = root / "mux.sock"
        self.mux_process: subprocess.Popen[bytes] | None = None
        self.gui_process: subprocess.Popen[bytes] | None = None
        self.gui_socket: Path | None = None
        self._mux_log_handle: Any = None
        self._gui_log_handles: list[Any] = []
        self._gui_generation = 0
        self._commands: list[dict[str, Any]] = []
        self._last_observation: Any = None
        self._captured = False

        self.home = root / "home"
        self.config = root / "config"
        self.cache = root / "cache"
        self.data = root / "data"
        self.state = root / "state"
        self.runtime = root / "runtime"
        self.logs = root / "logs"
        self.gui_socket_dir = self.home / ".local" / "share" / "wezterm"
        for directory in (
            self.home,
            self.config,
            self.cache,
            self.data,
            self.state,
            self.runtime,
            self.logs,
            self.gui_socket_dir,
        ):
            directory.mkdir(parents=True)
        self.runtime.chmod(0o700)

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
                "VTABS_BIN": str(toolchain.backend),
                "VTABS_LOG": str(self.logs / "wez-vtabs.log"),
                "VTABS_E2E_DOMAIN": "CurrentPaneDomain",
                "VTABS_E2E_MUX": str(self.mux_socket),
                # The standalone server passes this to its pane processes.  Server-side helpers
                # must route their own `wezterm cli` back to this fixture, never a user's daemon.
                "WEZTERM_UNIX_SOCKET": str(self.mux_socket),
            }
        )
        if sys.platform == "linux":
            self.env["LIBGL_ALWAYS_SOFTWARE"] = "1"
            self.env["VTABS_E2E_SOFTWARE"] = "1"
        self.env.update(extra_environment or {})
        self._capture_callbacks: list[Callable[[Path], None]] = []

    @property
    def mux_log(self) -> Path:
        return self.logs / "mux-server.log"

    @property
    def gui_logs(self) -> tuple[Path, ...]:
        return tuple(sorted(self.logs.glob("wezterm-gui-*.log")))

    def start(self) -> None:
        self.start_mux()
        self.wait_for(
            "the standalone mux server to answer on its private socket",
            lambda: self.topology("mux") if self.mux_socket.exists() else None,
            timeout=15,
        )
        self.start_gui()
        self.wait_for(
            "the GUI to expose the standalone mux topology",
            self._matching_endpoint_topologies,
            timeout=15,
            stable_for=0.2,
        )

    def start_mux(self) -> None:
        if self.mux_process is not None:
            raise AssertionError("the mux server was already started")
        self._mux_log_handle = self.mux_log.open("wb")
        self.mux_process = subprocess.Popen(
            [str(self.toolchain.mux_server), "--config-file", str(E2E_CONFIG)],
            cwd=ROOT,
            env=self.env,
            stdin=subprocess.DEVNULL,
            stdout=self._mux_log_handle,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )

    def start_gui(self) -> None:
        if self.gui_process is not None:
            raise AssertionError("the GUI is already connected")
        before = set(self.gui_socket_dir.glob("gui-sock-*"))
        self._gui_generation += 1
        log_path = self.logs / f"wezterm-gui-{self._gui_generation}.log"
        log_handle = log_path.open("wb")
        self._gui_log_handles.append(log_handle)
        self.gui_process = subprocess.Popen(
            [
                str(self.toolchain.wezterm),
                "--config-file",
                str(E2E_CONFIG),
                "start",
                "--always-new-process",
                "--no-auto-connect",
                "--class",
                self.identity,
                "--domain",
                "e2emux",
                "--attach",
            ],
            cwd=ROOT,
            env=self.env,
            stdin=subprocess.DEVNULL,
            stdout=log_handle,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        self.gui_socket = self.wait_for(
            "the new GUI's private command socket",
            lambda: self._new_gui_socket(before),
            timeout=15,
        )

    def _new_gui_socket(self, before: set[Path]) -> Path | None:
        candidates = [path for path in self.gui_socket_dir.glob("gui-sock-*") if path not in before]
        if not candidates:
            return None
        process = self.gui_process
        if process is not None:
            exact = [path for path in candidates if path.name.endswith(f"-{process.pid}")]
            if len(exact) == 1:
                return exact[0]
        if len(candidates) == 1:
            return candidates[0]
        raise AssertionError(f"new GUI created ambiguous command sockets: {candidates}")

    def disconnect_gui(self) -> None:
        process = self.gui_process
        self.gui_process = None
        self.gui_socket = None
        if process is not None:
            self._terminate_process_group(process, "GUI")
        for handle in self._gui_log_handles:
            handle.flush()

    def _matching_endpoint_topologies(self) -> tuple[Topology, Topology] | None:
        mux = self.topology("mux")
        gui = self.topology("gui")
        if not mux.panes or gui.shape != mux.shape:
            return None
        return mux, gui

    def _cli(
        self,
        endpoint: Endpoint,
        *args: str | os.PathLike[str],
        check: bool = True,
        timeout: float = 3,
    ) -> subprocess.CompletedProcess[str]:
        command = [str(self.toolchain.wezterm), "cli", "--no-auto-start"]
        cli_env = self.env.copy()
        if endpoint == "mux":
            command.append("--prefer-mux")
            cli_env["WEZTERM_UNIX_SOCKET"] = str(self.mux_socket)
        else:
            if self.gui_socket is None:
                raise CliError("GUI CLI requested while no GUI is connected")
            command.extend(("--class", self.identity))
            cli_env["WEZTERM_UNIX_SOCKET"] = str(self.gui_socket)
        command.extend(os.fspath(arg) for arg in args)
        started = time.monotonic()
        try:
            result = subprocess.run(
                command,
                cwd=ROOT,
                env=cli_env,
                stdin=subprocess.DEVNULL,
                capture_output=True,
                check=False,
                text=True,
                timeout=timeout,
            )
        except subprocess.TimeoutExpired as error:
            self._commands.append(
                {
                    "endpoint": endpoint,
                    "command": command,
                    "elapsed": time.monotonic() - started,
                    "timeout": timeout,
                    "error": repr(error),
                }
            )
            raise
        self._commands.append(
            {
                "endpoint": endpoint,
                "command": command,
                "elapsed": time.monotonic() - started,
                "returncode": result.returncode,
                "stdout": result.stdout,
                "stderr": result.stderr,
            }
        )
        if check and result.returncode != 0:
            raise CliError(
                f"{endpoint} CLI failed with {result.returncode}: {' '.join(command)}"
                f"\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
            )
        return result

    def topology(self, endpoint: Endpoint = "mux") -> Topology:
        result = self._cli(endpoint, "list", "--format", "json")
        return Topology.from_json(json.loads(result.stdout))

    def clients(self) -> Any:
        result = self._cli("mux", "list-clients", "--format", "json")
        return json.loads(result.stdout)

    def pane_text(self, pane_id: int, endpoint: Endpoint = "mux") -> str:
        return self._cli(endpoint, "get-text", "--pane-id", str(pane_id)).stdout

    def spawn_tab(self, pane_id: int) -> int:
        # Drive the user's attached client so the new tab becomes active and the plugin observes it.
        result = self._cli("gui", "spawn", "--pane-id", str(pane_id), "/bin/sh")
        return int(result.stdout.strip())

    def spawn_local_tab(self, window_id: int) -> int:
        return self.spawn_domain_tab(window_id, "local")

    def spawn_domain_tab(self, window_id: int, domain: str) -> int:
        result = self._cli(
            "gui",
            "spawn",
            "--window-id",
            str(window_id),
            "--domain-name",
            domain,
            "/bin/sh",
        )
        return int(result.stdout.strip())

    def probe(self, pane_id: int, name: str) -> None:
        encoded = base64.b64encode(name.encode()).decode()
        command = f"printf '\\033]1337;SetUserVar=vtabs_test={encoded}\\007'\n"
        self._cli("mux", "send-text", "--pane-id", str(pane_id), "--no-paste", command)

    def send_shell(self, pane_id: int, command: str, endpoint: Endpoint = "mux") -> None:
        self._cli(
            endpoint,
            "send-text",
            "--pane-id",
            str(pane_id),
            "--no-paste",
            f"{command}\n",
        )

    def adjust_pane_size(self, pane_id: int, direction: str, amount: int) -> None:
        self._cli(
            "mux",
            "adjust-pane-size",
            "--pane-id",
            str(pane_id),
            "--amount",
            str(amount),
            direction,
        )

    def wait_for(
        self,
        description: str,
        probe: Callable[[], Any],
        *,
        timeout: float = 10,
        stable_for: float = 0,
        poll: float = 0.1,
    ) -> Any:
        """Poll an observable condition until true for a duration independent of poll count."""

        deadline = time.monotonic() + timeout
        stable_since: float | None = None
        last_error: Exception | None = None
        last_value: Any = None
        while time.monotonic() < deadline:
            self._assert_owned_processes()
            try:
                last_value = probe()
                self._last_observation = last_value
                if last_value:
                    now = time.monotonic()
                    stable_since = stable_since or now
                    if now - stable_since >= stable_for:
                        return last_value
                else:
                    stable_since = None
            except (CliError, json.JSONDecodeError, OSError, subprocess.SubprocessError) as error:
                last_error = error
                stable_since = None
            time.sleep(poll)
        error_detail = f"\nlast transient error: {last_error}" if last_error else ""
        raise AssertionError(
            f"timed out after {timeout:.1f}s waiting for {description}"
            f"{error_detail}\nlast observation: {last_value!r}\n{self.diagnostic_summary()}"
        )

    def wait_topology(
        self,
        description: str,
        predicate: Callable[[Topology], bool],
        *,
        endpoint: Endpoint = "mux",
        timeout: float = 10,
        stable_for: float = 0.2,
    ) -> Topology:
        def observe() -> Topology | None:
            topology = self.topology(endpoint)
            return topology if predicate(topology) else None

        return self.wait_for(description, observe, timeout=timeout, stable_for=stable_for)

    def wait_ready_tabs(
        self,
        expected_tabs: int,
        *,
        endpoint: Endpoint = "mux",
        timeout: float = 12,
        stable_for: float = 0.3,
    ) -> Topology:
        def ready() -> Topology | None:
            topology = self.topology(endpoint)
            if len(topology.tabs) != expected_tabs:
                return None
            for tab in topology.tabs:
                if len(tab.sidebars) != 1 or not tab.content:
                    return None
                if not self.pane_text(tab.sidebars[0].pane_id, endpoint).strip():
                    return None
            return topology

        return self.wait_for(
            f"{expected_tabs} tab(s) with content and exactly one rendering sidebar",
            ready,
            timeout=timeout,
            stable_for=stable_for,
        )

    def wait_same_topology(
        self,
        expected_mux_ids: frozenset[int] | None = None,
    ) -> tuple[Topology, Topology]:
        def matching() -> tuple[Topology, Topology] | None:
            mux = self.topology("mux")
            gui = self.topology("gui")
            wanted = expected_mux_ids if expected_mux_ids is not None else mux.pane_ids
            if mux.pane_ids == wanted and gui.shape == mux.shape:
                return mux, gui
            return None

        return self.wait_for(
            "the GUI and standalone mux to expose the same tab shape",
            matching,
            stable_for=0.2,
        )

    def _assert_owned_processes(self) -> None:
        mux = self.mux_process
        if mux is not None and mux.poll() is not None:
            raise AssertionError(
                f"standalone mux server exited with {mux.returncode}\n{self._tail(self.mux_log)}"
            )
        gui = self.gui_process
        if gui is not None and gui.poll() is not None:
            latest = self.gui_logs[-1] if self.gui_logs else self.logs / "missing-gui.log"
            raise AssertionError(f"WezTerm GUI exited with {gui.returncode}\n{self._tail(latest)}")

    @staticmethod
    def _tail(path: Path, lines: int = 80) -> str:
        try:
            return "\n".join(path.read_text(errors="replace").splitlines()[-lines:])
        except OSError as error:
            return f"unable to read {path}: {error}"

    def diagnostic_summary(self) -> str:
        for handle in [self._mux_log_handle, *self._gui_log_handles]:
            if handle is not None:
                handle.flush()
        sections = [f"wezterm version: {self.toolchain.version}"]
        for endpoint in ("mux", "gui"):
            if endpoint == "gui" and self.gui_socket is None:
                sections.append("gui topology: disconnected")
                continue
            try:
                sections.append(f"{endpoint} topology: {self.topology(endpoint).as_list()!r}")
            except Exception as error:  # diagnostics must not hide the original failure
                sections.append(f"{endpoint} topology error: {error!r}")
        try:
            sections.append(f"mux clients: {self.clients()!r}")
        except Exception as error:  # diagnostics must not hide the original failure
            sections.append(f"mux clients error: {error!r}")
        sections.append(f"mux log tail:\n{self._tail(self.mux_log)}")
        for path in self.gui_logs:
            sections.append(f"{path.name} tail:\n{self._tail(path)}")
        backend_log = self.logs / "wez-vtabs.log"
        sections.append(f"backend log tail:\n{self._tail(backend_log)}")
        return "\n\n".join(sections)

    def capture_failure(self, artifact_root: Path, nodeid: str) -> Path:
        if self._captured:
            matches = sorted((artifact_root / "wezterm").glob(f"{_safe_name(nodeid)}-*"))
            return matches[-1] if matches else artifact_root / "wezterm"
        self._captured = True
        destination = artifact_root / "wezterm" / f"{_safe_name(nodeid)}-{self.identity[-8:]}"
        destination.mkdir(parents=True, exist_ok=True)
        for callback in self._capture_callbacks:
            try:
                callback(self.logs)
            except Exception as error:  # diagnostics must not hide the original failure
                with (self.logs / "capture-errors.log").open("a") as errors:
                    errors.write(f"{error!r}\n")
        for handle in [self._mux_log_handle, *self._gui_log_handles]:
            if handle is not None:
                handle.flush()
        for path in self.logs.glob("*"):
            if path.is_file():
                shutil.copy2(path, destination / path.name)
        (destination / "diagnostics.txt").write_text(self.diagnostic_summary())
        (destination / "cli-history.json").write_text(
            json.dumps(self._commands, indent=2, ensure_ascii=False, default=repr)
        )
        return destination

    def close(self) -> None:
        cleanup_errors: list[str] = []
        try:
            self.disconnect_gui()
        except Exception as error:  # finish cleaning the independently owned server
            cleanup_errors.append(f"GUI cleanup: {error!r}")
        if self.mux_process is not None and self.mux_process.poll() is None:
            try:
                for pane in self.topology("mux").panes:
                    self._cli("mux", "kill-pane", "--pane-id", str(pane.pane_id), check=False)
            except Exception as error:
                cleanup_errors.append(f"pane cleanup: {error!r}")
            try:
                self._terminate_process_group(self.mux_process, "mux server")
            except Exception as error:
                cleanup_errors.append(f"mux cleanup: {error!r}")
        self.mux_process = None
        for handle in [self._mux_log_handle, *self._gui_log_handles]:
            if handle is not None:
                handle.close()
        self._mux_log_handle = None
        self._gui_log_handles.clear()
        if cleanup_errors:
            raise AssertionError("; ".join(cleanup_errors))

    @staticmethod
    def _terminate_process_group(process: subprocess.Popen[bytes], description: str) -> None:
        if process.poll() is not None:
            process.wait()
            return
        try:
            if os.getpgid(process.pid) == process.pid:
                os.killpg(process.pid, signal.SIGTERM)
            else:
                process.terminate()
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=5)
            return
        except subprocess.TimeoutExpired:
            pass
        try:
            if os.getpgid(process.pid) == process.pid:
                os.killpg(process.pid, signal.SIGKILL)
            else:
                process.kill()
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired as error:
            raise AssertionError(f"owned {description} process group did not stop") from error


def _safe_name(nodeid: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.-]+", "-", nodeid).strip("-")


@pytest.fixture
def wezterm_options() -> E2EOptions:
    return E2EOptions({})


@pytest.hookimpl(hookwrapper=True)
def pytest_runtest_makereport(item: pytest.Item, call: pytest.CallInfo[Any]):
    outcome = yield
    report = outcome.get_result()
    setattr(item, f"rep_{report.when}", report)


@pytest.fixture
def wezterm_mux(
    wezterm_toolchain: Toolchain,
    wezterm_artifact_dir: Path,
    wezterm_options: E2EOptions,
    request: pytest.FixtureRequest,
) -> Iterator[WezTermMuxInstance]:
    # Unix-domain socket paths have a small platform limit; keep the whole root deliberately short.
    root = Path(tempfile.mkdtemp(prefix="vt-", dir="/tmp"))
    instance = WezTermMuxInstance(root, wezterm_toolchain, wezterm_options.environment)
    if wezterm_options.capture is not None:
        instance._capture_callbacks.append(wezterm_options.capture)
    setup_complete = False
    try:
        instance.start()
        setup_complete = True
        yield instance
    except BaseException:
        destination = instance.capture_failure(wezterm_artifact_dir, request.node.nodeid)
        if setup_complete:
            report = getattr(request.node, "rep_call", None)
            if report is not None:
                report.sections.append(("wezterm e2e artifacts", str(destination)))
        raise
    finally:
        report = getattr(request.node, "rep_call", None)
        if report is not None and report.failed and not instance._captured:
            destination = instance.capture_failure(wezterm_artifact_dir, request.node.nodeid)
            report.sections.append(("wezterm e2e artifacts", str(destination)))
        try:
            instance.close()
        except BaseException:
            if not instance._captured:
                instance.capture_failure(wezterm_artifact_dir, request.node.nodeid)
            raise
        finally:
            shutil.rmtree(root, ignore_errors=True)


def pytest_collection_modifyitems(items: Sequence[pytest.Item]) -> None:
    for item in items:
        if Path(str(item.path)).resolve().is_relative_to(Path(__file__).parent):
            item.add_marker("wezterm_e2e")
