"""Small public-behavior smoke test against a real, isolated WezTerm GUI."""

from __future__ import annotations

import os
from collections import defaultdict
from collections.abc import Callable
from typing import Any, Protocol

import pytest


OPT_IN = "VTABS_WEZTERM_E2E"
pytestmark = [
    pytest.mark.wezterm_e2e,
    pytest.mark.skipif(
        os.environ.get(OPT_IN) != "1",
        reason=f"real WezTerm smoke test is opt-in; set {OPT_IN}=1",
    ),
]


class WezTerm(Protocol):
    def panes(self) -> list[dict[str, Any]]: ...

    def text(self, pane_id: int) -> str: ...

    def spawn_tab(self, pane_id: int) -> None: ...

    def wait_for(
        self,
        description: str,
        predicate: Callable[[], Any],
        *,
        timeout: float = 10,
        consecutive: int = 1,
    ) -> Any: ...


def _tabs_with_rendering_sidebars(
    wezterm: WezTerm, expected_tabs: int
) -> dict[int, list[dict[str, Any]]] | None:
    by_tab: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for pane in wezterm.panes():
        tab_id = pane.get("tab_id")
        if isinstance(tab_id, int):
            by_tab[tab_id].append(pane)

    if len(by_tab) != expected_tabs:
        return None

    for panes in by_tab.values():
        sidebars = [
            pane
            for pane in panes
            if str(pane.get("title", "")).startswith("wez-vtabs:")
        ]
        content = [pane for pane in panes if pane not in sidebars]
        if len(sidebars) != 1 or not content:
            return None
        pane_id = sidebars[0].get("pane_id")
        if not isinstance(pane_id, int) or not wezterm.text(pane_id).strip():
            return None
    return dict(by_tab)


def test_attaches_a_rendering_sidebar_to_every_tab(wezterm_instance: WezTerm) -> None:
    """A new tab gets one usable sidebar without disturbing the existing tab."""

    first = wezterm_instance.wait_for(
        "one content pane with one rendering sidebar",
        lambda: _tabs_with_rendering_sidebars(wezterm_instance, 1),
        consecutive=3,
    )
    first_content = next(
        pane
        for pane in next(iter(first.values()))
        if not str(pane.get("title", "")).startswith("wez-vtabs:")
    )

    first_content_id = first_content.get("pane_id")
    assert isinstance(first_content_id, int), "the content pane should have a CLI pane id"
    wezterm_instance.spawn_tab(first_content_id)

    tabs = wezterm_instance.wait_for(
        "each of two tabs to have content and one rendering sidebar",
        lambda: _tabs_with_rendering_sidebars(wezterm_instance, 2),
        consecutive=3,
    )
    assert len(tabs) == 2, "the original and newly spawned tab should both remain available"
