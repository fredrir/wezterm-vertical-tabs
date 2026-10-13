"""Production startup, attachment, detachment and shutdown on owned displays."""

import json
from contextlib import ExitStack

import pytest

from tests.scenarios.tls_fixture import LocalTlsMux
from tests.support.gui import GuiInput
from tests.support.wezterm import WeztermCli


@pytest.mark.gui
@pytest.mark.parametrize("domain", ["local", "unix"])
def test_start_render_and_shutdown(probe_factory, headless_display, tmp_path, domain):
    probe = probe_factory(domain, domain, chrome=True, effects=True)
    state = probe.start()
    assert state["visible"]
    assert state["content"]["width"] > 0
    probe.setting("width", 300)
    probe.action("quit")
    probe.gui_process.wait(timeout=10)
    assert probe.gui_process.returncode == 0
    if domain == "unix":
        assert probe.processes[0].poll() is None, "quitting the GUI stopped the remote mux"
    probe.close()
    assert not any(process.poll() is None for process in probe.processes)
    assert headless_display.owned
    (tmp_path / "result.json").write_text(json.dumps({"domain": domain, "shutdown": "clean"}))


@pytest.mark.gui
def test_mutual_tls_tab_lifecycle(wezterm_binaries, probe_factory, tmp_path):
    with ExitStack() as cleanup:
        server = LocalTlsMux(tmp_path / "server", wezterm_binaries["wezterm-mux-server"])
        cleanup.callback(server.close)
        domain = server.start()
        assert server.protocol in {"TLSv1.2", "TLSv1.3"}
        probe = probe_factory("client", "tls", tls_config=domain)
        cleanup.callback(probe.close)
        probe.start()
        probe.action("new_tab")
        probe.wait(lambda state: len(state["tabs"]) == 2)
        probe.action("close")
        probe.wait(lambda state: len(state["tabs"]) == 1)
    assert server.process.poll() is not None
    assert list(server.certificates.glob("*.key")) == []


@pytest.mark.gui
@pytest.mark.parametrize("hidden", [[], ["__backing:elsewhere"]])
def test_detaching_the_last_pane_quits_and_reconnecting_starts_fresh(
    wezterm_binaries, probe_factory, hidden
):
    probe = probe_factory("unix", "unix")
    cli = WeztermCli(wezterm_binaries["wezterm"], probe.config, probe.root / "mux.sock", probe.env)

    def workspaces():
        return sorted(pane["workspace"] for pane in cli.panes())

    workspace = probe.start()["workspace"]
    # The mux server's own startup pane would give the GUI a workspace to fall back to.
    for pane in cli.panes():
        if pane["workspace"] != workspace:
            cli("kill-pane", "--pane-id", pane["pane_id"])
    # Tabs backing another host's panes must not keep the GUI open or receive it.
    for name in hidden:
        cli("spawn", "--new-window", "--workspace", name)
    probe.sample_for(0.5)
    probe.send("detach")
    probe.gui_process.wait(timeout=10)
    assert probe.gui_process.returncode == 0
    assert probe.processes[0].poll() is None, "quitting the GUI stopped the mux"
    assert workspaces() == sorted(["__detached", *hidden])

    (probe.root / "command.json").unlink()
    probe.latest.clear()
    probe.gui_process = probe.start_process(
        [
            probe.gui,
            "--config-file",
            probe.config,
            "connect",
            "scenario-unix",
            "--class",
            probe.identity,
        ],
        "reconnect.log",
    )
    window = probe.wait(lambda state: state.get("tabs"), timeout=25, any_window=True)["window"]
    # The connection UI's tab closes once the domain attaches.
    probe.sample_for(0.5)
    state = probe.latest[window]
    assert state["workspace"] not in ["__detached", *hidden]
    assert len(state["tabs"]) == 1
    assert workspaces() == sorted(["__detached", "default", *hidden])


@pytest.mark.gui
def test_unix_attach_and_new_tab_agree_without_a_window_resize(probe_factory):
    probe = probe_factory("unix", "unix", chrome=True, initial_size={"cols": 100, "rows": 32})
    initial = probe.start()
    dimensions = initial["dimensions"]
    initial_size = initial["tabs"][0]["size"]
    probe.action("new_tab")
    created = probe.wait(lambda state: len(state.get("tabs", [])) == 2)
    samples = [created, *probe.sample_for(0.25)]
    for state in samples:
        if state["window"] != probe.window:
            continue
        assert state["dimensions"] == dimensions, "attaching or spawning resized the window"
        assert all(tab["size"] == initial_size for tab in state["tabs"]), (
            "remote attachment kept provisional geometry until the next physical resize"
        )


@pytest.mark.gui
def test_tab_search_lists_and_adopts_unadopted_remote_panes(
    wezterm_binaries, probe_factory, isolated_env, tmp_path
):
    def adoptable(state):
        return {
            hit["id"]
            for hit in state.get("model", {}).get("hits", [])
            if hit["id"].startswith('Menu("adopt/')
        }

    with ExitStack() as cleanup:
        remote = LocalTlsMux(tmp_path / "remote", wezterm_binaries["wezterm-mux-server"])
        cleanup.callback(remote.close)
        remote.start()
        remote_cli = WeztermCli(
            wezterm_binaries["wezterm"], remote.config, remote.root / "mux.sock", isolated_env
        )
        peer = remote_cli.proxy_domain("peer", local_pane_layout=True)
        probe = probe_factory(
            "gui",
            "unix",
            extra_lua=f"table.insert(cfg.unix_domains, wezterm.json_parse([=[{json.dumps(peer)}]=]))",
        )
        cleanup.callback(probe.close)
        probe.config.write_text(
            probe.config.read_text().replace(
                "  plugin.apply_to_config(cfg,options)",
                "  options.hooks={adoptable=function() return 'peer' end}\n"
                "  plugin.apply_to_config(cfg,options)",
            )
        )
        gui = GuiInput(probe, tmp_path)
        (pane,) = [p["pane_id"] for p in remote_cli.panes()]
        initial = probe.start()
        gui.attach()
        probe.action("navigator")
        entry = f'Menu("adopt/peer/{pane}")'
        probe.wait(lambda state: adoptable(state) == {entry})
        gui.click(entry)
        probe.wait(lambda state: len(state["tabs"]) == len(initial["tabs"]) + 1)
        assert remote_cli.panes()[0]["workspace"].startswith("__backing:")

        # A second remote pane proves the next search finished without the adopted one.
        second = int(remote_cli("spawn", "--new-window"))
        probe.action("navigator")
        probe.wait(lambda state: adoptable(state) == {f'Menu("adopt/peer/{second}")'})
