"""Real SSH mux transport, key rejection, tab lifecycle, and container cleanup."""

import pytest

from tests.native.scenarios import Probe
from tests.support.container_mux import ContainerSshMux


@pytest.mark.container
@pytest.mark.native
def test_container_ssh_mux_domain(
    native_binaries, project_root, isolated_env, headless_display, tmp_path
):
    fixture = ContainerSshMux(tmp_path / "container", native_binaries, project_root, isolated_env)
    probe = None
    try:
        domain = fixture.start()
        fixture.reject_unknown_key()
        probe = Probe(
            tmp_path / "ssh",
            native_binaries["wezterm-gui"],
            native_binaries["wez-vtabs-store"],
            "ssh",
            ssh_config=domain,
            server=native_binaries["wezterm-mux-server"],
            display=headless_display,
        )
        initial = probe.start()
        assert initial["model"]["can_reopen"] is False
        probe.action("new_tab")
        probe.wait(lambda state: len(state["tabs"]) == 2)
        remote = [pane for pane in fixture.panes() if pane["workspace"] == probe.identity]
        assert len(remote) == 2
        probe.action("close")
        probe.wait(lambda state: len(state["tabs"]) == 1 and state["model"]["can_reopen"])
        probe.intent("Reopen")
        probe.wait(lambda state: len(state["tabs"]) == 2 and not state["model"]["can_reopen"])
    finally:
        try:
            if probe is not None:
                probe.close()
        finally:
            fixture.close()
    assert not fixture.started
