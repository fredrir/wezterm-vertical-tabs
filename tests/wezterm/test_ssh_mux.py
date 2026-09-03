"""Behavioral coverage for a real WezTerm mux reached through localhost SSH."""

from __future__ import annotations

from typing import TYPE_CHECKING

import pytest
from test_smoke import _transport_states

if TYPE_CHECKING:
    from conftest import E2EOptions, SshMuxContainer, WezTermMuxInstance


@pytest.fixture
def wezterm_options(ssh_mux_container: SshMuxContainer) -> E2EOptions:
    from conftest import E2EOptions

    return E2EOptions(
        {
            "VTABS_E2E_SSH_ADDRESS": f"127.0.0.1:{ssh_mux_container.port}",
            "VTABS_E2E_SSH_USER": "vtabs",
            "VTABS_E2E_SSH_IDENTITY": str(ssh_mux_container.identity_file),
            "VTABS_E2E_REMOTE_WEZTERM": "/usr/bin/wezterm",
            "VTABS_E2E_REMOTE_BIN": "/usr/local/bin/wez-vtabs-e2e",
        },
        capture=ssh_mux_container.capture,
    )


@pytest.mark.ssh_mux_e2e
def test_ssh_mux_tab_runs_content_and_sidebar_on_the_remote_endpoint(
    wezterm_mux: WezTermMuxInstance,
) -> None:
    """An SSH-domain tab executes remotely without changing the standalone Unix domain."""

    local_mux = wezterm_mux.wait_ready_tabs(1)
    local_ids = local_mux.pane_ids
    gui_before = wezterm_mux.wait_ready_tabs(1, endpoint="gui")
    window_id = gui_before.tabs[0].panes[0].window_id

    remote_content_id = wezterm_mux.spawn_domain_tab(window_id, "e2essh")
    gui = wezterm_mux.wait_ready_tabs(2, endpoint="gui", timeout=25, stable_for=0.5)
    remote_tab = next(
        tab for tab in gui.tabs if remote_content_id in {pane.pane_id for pane in tab.content}
    )

    marker = f"ssh-domain-pane-{remote_content_id}"
    wezterm_mux.send_shell(
        remote_content_id,
        f"printf '{marker} '; uname -s",
        endpoint="gui",
    )

    def remote_output() -> str | None:
        text = wezterm_mux.pane_text(remote_content_id, endpoint="gui")
        return text if marker in text and "Linux" in text else None

    output = wezterm_mux.wait_for(
        "the SSH-domain shell to execute on Linux",
        remote_output,
        timeout=15,
        stable_for=0.2,
    )
    local_after = wezterm_mux.wait_topology(
        "the standalone Unix mux to retain only its original panes",
        lambda topology: topology.pane_ids == local_ids,
        stable_for=0.5,
    )

    assert len(remote_tab.sidebars) == 1
    assert len(remote_tab.content) == 1
    assert marker in output and "Linux" in output
    assert local_after.pane_ids == local_ids

    states, _, _ = _transport_states(wezterm_mux, local_ids[0])
    assert states.get(remote_tab.sidebars[0].pane_id) == "off", (
        "a remote host's sidebar must never be offered the inbox"
    )
