"""Fresh TLS shells retain the caller's local split tree."""

import concurrent.futures
import json
import os
import signal
import subprocess
import tempfile
import time
from pathlib import Path

import pytest

from tests.scenarios.tls_fixture import LocalTlsMux

pytestmark = pytest.mark.gui


class MuxServer:
    def __init__(self, root, binaries, environment, clients):
        self.root, self.binaries = root, binaries
        root.mkdir()
        self.socket = root / "mux.sock"
        self.config = root / "mux.lua"
        self.config.write_text(
            "local wezterm = require 'wezterm'\nreturn {"
            "check_for_updates=false,automatically_reload_config=false,"
            "default_prog={'/bin/sh'},initial_cols=120,initial_rows=40,"
            "unix_domains={{name='fixture',socket_path="
            + json.dumps(str(self.socket))
            + ",no_serve_automatically=true}},tls_clients=wezterm.json_parse([=["
            + json.dumps(clients)
            + "]=])}\n"
        )
        self.env = {
            key: value
            for key, value in environment.items()
            if not key.startswith("TMUX")
            and key not in {"ZDOTDIR", "ENV", "BASH_ENV", "HWIRE_SESSION"}
        }
        self.env["WEZTERM_UNIX_SOCKET"] = str(self.socket)
        self.log = (root / "mux.log").open("w")
        self.process = subprocess.Popen(
            [str(binaries["wezterm-mux-server"]), "--config-file", str(self.config)],
            env=self.env,
            stdin=subprocess.DEVNULL,
            stdout=self.log,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        wait_for(lambda: self.socket.exists())

    def cli(self, *args):
        return subprocess.run(
            [
                str(self.binaries["wezterm"]),
                "--config-file",
                str(self.config),
                "cli",
                "--no-auto-start",
                *map(str, args),
            ],
            env=self.env,
            capture_output=True,
            text=True,
            check=True,
            timeout=20,
        ).stdout.strip()

    def panes(self):
        return json.loads(self.cli("list", "--format", "json"))

    def close(self):
        try:
            os.killpg(self.process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            self.process.wait(timeout=5)
        finally:
            try:
                os.killpg(self.process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            self.log.close()


def wait_for(condition):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        value = condition()
        if value:
            return value
        time.sleep(0.05)
    raise AssertionError("mux state did not converge")


@pytest.fixture
def mux_pair(wezterm_binaries, isolated_env):
    # Keep Unix sockets below macOS's path-length limit.
    with tempfile.TemporaryDirectory(prefix="vt-attach-", dir="/tmp") as directory:
        root = Path(directory)
        remote = LocalTlsMux(root / "remote", wezterm_binaries["wezterm-mux-server"])
        local = None
        try:
            remote.start()
            clients = []
            for name, local_layout in (("peer", True), ("peer-alt", True), ("shared", False)):
                clients.append(
                    {
                        **json.loads(remote.domain.read_text()),
                        "name": name,
                        "local_pane_layout": local_layout,
                    }
                )
            clients.append({**clients[0], "name": "offline", "remote_address": "127.0.0.1:1"})
            local = MuxServer(root / "local", wezterm_binaries, isolated_env, clients)

            def remote_cli(*args):
                return subprocess.run(
                    [
                        str(wezterm_binaries["wezterm"]),
                        "--config-file",
                        str(remote.config),
                        "cli",
                        "--no-auto-start",
                        *map(str, args),
                    ],
                    env={**isolated_env, "WEZTERM_UNIX_SOCKET": str(remote.root / "mux.sock")},
                    check=True,
                    capture_output=True,
                    text=True,
                    timeout=20,
                ).stdout.strip()

            for pane in local.panes():
                local.cli("kill-pane", "--pane-id", pane["pane_id"])
            for pane in json.loads(remote_cli("list", "--format", "json")):
                remote_cli("kill-pane", "--pane-id", pane["pane_id"])
            wait_for(lambda: not local.panes())
            wait_for(lambda: not json.loads(remote_cli("list", "--format", "json")))
            yield local, remote_cli
        finally:
            if local is not None:
                local.close()
            remote.close()


def geometry(pane):
    return pane["left_col"], pane["top_row"], pane["size"]["cols"], pane["size"]["rows"]


def test_tls_replacement_preserves_siblings_and_closes_independently(mux_pair):
    local, remote = mux_pair
    existing = int(remote("spawn", "--new-window"))
    left = int(local.cli("spawn", "--new-window"))
    right = int(local.cli("split-pane", "--pane-id", left, "--right"))
    before = {p["pane_id"]: p for p in local.panes()}
    original_tab = before[left]["tab_id"]
    original_window = before[left]["window_id"]
    fresh = int(local.cli("split-pane", "--pane-id", right, "--domain-name", "peer"))
    local.cli("kill-pane", "--pane-id", right)
    panes = wait_for(lambda: len(p := local.panes()) == 2 and p)
    assert {p["tab_id"] for p in panes} == {original_tab}
    assert {p["window_id"] for p in panes} == {original_window}
    assert geometry(next(p for p in panes if p["pane_id"] == left)) == geometry(before[left])
    assert geometry(next(p for p in panes if p["pane_id"] == fresh)) == geometry(before[right])
    assert len(json.loads(remote("list", "--format", "json"))) == 2

    # A second route to the same server must not import another copy of its panes.
    second = int(local.cli("split-pane", "--pane-id", left, "--domain-name", "peer-alt"))
    local.cli("kill-pane", "--pane-id", left)
    sibling = int(local.cli("split-pane", "--pane-id", fresh, "--right"))
    local.cli("adjust-pane-size", "--pane-id", sibling, "--amount", 3, "Left")
    panes = wait_for(lambda: len(p := local.panes()) == 3 and p)
    assert {p["pane_id"] for p in panes} == {fresh, second, sibling}
    assert {p["tab_id"] for p in panes} == {original_tab}
    local.cli("kill-pane", "--pane-id", fresh)
    panes = wait_for(lambda: len(p := local.panes()) == 2 and p)
    assert {p["pane_id"] for p in panes} == {second, sibling}
    remote_panes = wait_for(
        lambda: len(p := json.loads(remote("list", "--format", "json"))) == 3 and p
    )
    assert existing in {p["pane_id"] for p in remote_panes}

    # Returning to this computer creates a local shell in the same slot.
    before_return = next(p for p in panes if p["pane_id"] == second)
    returned = int(local.cli("split-pane", "--pane-id", second, "--domain-name", "local"))
    local.cli("kill-pane", "--pane-id", second)
    panes = wait_for(lambda: len(p := local.panes()) == 2 and p)
    assert {p["pane_id"] for p in panes} == {returned, sibling}
    assert geometry(next(p for p in panes if p["pane_id"] == returned)) == geometry(before_return)


def test_shared_mux_concurrent_spawns_do_not_duplicate_proxies(mux_pair):
    local, remote = mux_pair
    source = int(local.cli("spawn", "--new-window"))
    # Attach before concurrent requests so this exercises spawn/resync ordering.
    spawned = [int(local.cli("spawn", "--pane-id", source, "--domain-name", "shared"))]
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        spawned += list(
            pool.map(
                lambda _: int(local.cli("spawn", "--pane-id", source, "--domain-name", "shared")),
                range(16),
            )
        )
    panes = local.panes()
    assert len(panes) == len(spawned) + 1
    assert {p["pane_id"] for p in panes} == {source, *spawned}
    assert len(json.loads(remote("list", "--format", "json"))) == len(spawned)
    local.cli("kill-pane", "--pane-id", spawned[0])
    panes = wait_for(lambda: len(p := local.panes()) == len(spawned) and p)
    assert {p["pane_id"] for p in panes} == {source, *spawned[1:]}


def test_shared_mux_split_moves_translate_pane_ids(mux_pair):
    local, remote = mux_pair
    source = int(local.cli("spawn", "--new-window"))
    first = int(local.cli("spawn", "--pane-id", source, "--domain-name", "shared"))
    second = int(local.cli("spawn", "--pane-id", source, "--domain-name", "shared"))
    local.cli("split-pane", "--pane-id", first, "--move-pane-id", second, "--right")
    panes = wait_for(lambda: len(p := local.panes()) == 3 and p)
    tabs = {p["pane_id"]: p["tab_id"] for p in panes}
    assert tabs[first] == tabs[second] != tabs[source]
    assert len(json.loads(remote("list", "--format", "json"))) == 2
    local.cli("kill-pane", "--pane-id", first)
    panes = wait_for(lambda: len(p := local.panes()) == 2 and p)
    assert {p["pane_id"] for p in panes} == {source, second}


def test_failed_remote_connection_preserves_the_source(mux_pair):
    local, remote = mux_pair
    source = int(local.cli("spawn", "--new-window"))
    before = local.panes()
    with pytest.raises(subprocess.CalledProcessError):
        local.cli("split-pane", "--pane-id", source, "--domain-name", "offline")
    after = local.panes()
    assert source in {p["pane_id"] for p in after}
    assert geometry(next(p for p in after if p["pane_id"] == source)) == geometry(before[0])
    assert json.loads(remote("list", "--format", "json")) == []


def test_shared_client_removal_does_not_resize_an_unaffected_sibling(mux_pair):
    local, remote = mux_pair
    source = int(remote("spawn", "--new-window"))
    sibling = int(remote("split-pane", "--pane-id", source, "--right"))
    expected = geometry(
        next(p for p in json.loads(remote("list", "--format", "json")) if p["pane_id"] == sibling)
    )
    local.cli("spawn", "--new-window", "--domain-name", "shared")
    wait_for(lambda: len(local.panes()) == 3)

    # Remote notifications can reach a shared client before its replacement
    # tree. Pruning the old proxy must not send its interim expansion upstream.
    for _ in range(6):
        prior = {p["pane_id"] for p in local.panes()}
        replacement = int(remote("split-pane", "--pane-id", source))
        remote("kill-pane", "--pane-id", source)
        wait_for(lambda prior=prior: len(p := local.panes()) == 3 and {v["pane_id"] for v in p} != prior)
        source = replacement
        panes = json.loads(remote("list", "--format", "json"))
        assert geometry(next(p for p in panes if p["pane_id"] == sibling)) == expected
