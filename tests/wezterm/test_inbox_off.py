"""The inbox transport is a knob: off, every mux sidebar stays on stdin."""

from __future__ import annotations

from typing import TYPE_CHECKING

import pytest
from test_smoke import _only_tab, _transport_states

if TYPE_CHECKING:
    from conftest import E2EOptions, WezTermMuxInstance


@pytest.fixture
def wezterm_options() -> E2EOptions:
    from conftest import E2EOptions

    return E2EOptions({"VTABS_E2E_INBOX": "0"})


def test_backend_inbox_off_keeps_every_sidebar_on_stdin(
    wezterm_mux: WezTermMuxInstance,
) -> None:
    mux_content_id = _only_tab(wezterm_mux.wait_ready_tabs(1, stable_for=0.5)).content[0].pane_id
    gui = wezterm_mux.wait_ready_tabs(1, endpoint="gui")
    states, writes, _ = _transport_states(wezterm_mux, mux_content_id)
    assert states == {_only_tab(gui).sidebars[0].pane_id: "off"}
    assert writes == 0
