"""Behavioral coverage for rendering with the optional inbox disabled."""

from __future__ import annotations

from typing import TYPE_CHECKING

import pytest
from test_smoke import _only_tab

if TYPE_CHECKING:
    from conftest import E2EOptions, WezTermMuxInstance


@pytest.fixture
def wezterm_options() -> E2EOptions:
    from conftest import E2EOptions

    return E2EOptions({"VTABS_E2E_INBOX": "0"})


def test_sidebar_stays_responsive_with_backend_inbox_disabled(
    wezterm_mux: WezTermMuxInstance,
) -> None:
    """A model update still reaches the visible sidebar through the fallback path."""

    tab = _only_tab(wezterm_mux.wait_ready_tabs(1, stable_for=0.5))
    sidebar_id = tab.sidebars[0].pane_id
    title = f"inbox-disabled-{tab.tab_id}"

    wezterm_mux.set_tab_title(tab.tab_id, title)

    def rendered_title() -> str | None:
        text = wezterm_mux.pane_text(sidebar_id)
        return text if title in text else None

    rendered = wezterm_mux.wait_for(
        "the sidebar to render a title update with its inbox disabled",
        rendered_title,
    )

    assert title in rendered
