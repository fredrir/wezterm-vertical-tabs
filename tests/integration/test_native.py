"""Production native startup, rendering, and shutdown on owned displays."""

import json
import subprocess
from contextlib import ExitStack, suppress

import pytest

from tests.native.scenarios import Probe
from tests.native.tls_fixture import LocalTlsMux
from tests.native.ui_scenarios import GuiInput, scenarios


@pytest.mark.native
@pytest.mark.parametrize("domain", ["local", "unix"])
def test_native_start_render_and_shutdown(native_binaries, headless_display, tmp_path, domain):
    probe = Probe(
        tmp_path / domain,
        native_binaries["wezterm-gui"],
        native_binaries["wez-vtabs-store"],
        domain,
        server=native_binaries["wezterm-mux-server"],
        chrome=True,
        effects=True,
        display=headless_display,
    )
    try:
        state = probe.start()
        assert state["visible"]
        assert state["content"]["width"] > 0
        assert state["paint"]["samples"] > 0
        assert state["model"]["can_reopen"] is False
        probe.intent({"SetSetting": {"key": "width", "value": 300}})
        probe.wait(lambda current: current["model"]["settings"]["width"] == 300)
        probe.action("quit")
        probe.gui_process.wait(timeout=10)
        assert probe.gui_process.returncode == 0
        if domain == "unix":
            assert probe.processes[0].poll() is None, "quitting the GUI stopped the remote mux"
    finally:
        probe.close()
    assert not any(process.poll() is None for process in probe.processes)
    assert headless_display.owned
    (tmp_path / "result.json").write_text(json.dumps({"domain": domain, "shutdown": "clean"}))


@pytest.mark.native
def test_native_keyboard_pointer_and_clipboard(native_binaries, headless_display, tmp_path):
    probe = Probe(
        tmp_path / "native",
        native_binaries["wezterm-gui"],
        native_binaries["wez-vtabs-store"],
        "local",
        effects=True,
        initial_size={"cols": 110, "rows": 40},
        display=headless_display,
    )
    gui = GuiInput(probe, headless_display, tmp_path)
    try:
        report = scenarios(probe, gui)
        (tmp_path / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        assert report["errors"] == []
    except Exception as error:
        (tmp_path / "report.json").write_text(
            json.dumps(
                {"passed": False, "error": str(error), "screenshots": gui.captures}, indent=2
            )
            + "\n"
        )
        if gui.window is not None:
            with suppress(OSError, subprocess.SubprocessError, AssertionError):
                gui.capture("failure", pause=0)
        raise
    finally:
        probe.close()


@pytest.mark.native
def test_native_mutual_tls_tab_lifecycle(native_binaries, headless_display, tmp_path):
    with ExitStack() as cleanup:
        server = LocalTlsMux(tmp_path / "server", native_binaries["wezterm-mux-server"])
        cleanup.callback(server.close)
        domain = server.start()
        assert server.protocol in {"TLSv1.2", "TLSv1.3"}
        probe = Probe(
            tmp_path / "client",
            native_binaries["wezterm-gui"],
            native_binaries["wez-vtabs-store"],
            "tls",
            tls_config=domain,
            display=headless_display,
        )
        cleanup.callback(probe.close)
        initial = probe.start()
        assert initial["model"]["can_reopen"] is False
        probe.action("new_tab")
        probe.wait(lambda state: len(state["tabs"]) == 2)
        probe.action("close")
        probe.wait(lambda state: len(state["tabs"]) == 1 and state["model"]["can_reopen"])
        probe.intent("Reopen")
        probe.wait(lambda state: len(state["tabs"]) == 2 and not state["model"]["can_reopen"])
    assert server.process.poll() is not None
    assert list(server.certificates.glob("*.key")) == []


@pytest.mark.native
def test_unix_attach_and_new_tab_agree_without_a_window_resize(
    native_binaries, headless_display, tmp_path
):
    probe = Probe(
        tmp_path / "unix",
        native_binaries["wezterm-gui"],
        native_binaries["wez-vtabs-store"],
        "unix",
        server=native_binaries["wezterm-mux-server"],
        chrome=True,
        initial_size={"cols": 100, "rows": 32},
        display=headless_display,
    )
    try:
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
    finally:
        probe.close()
