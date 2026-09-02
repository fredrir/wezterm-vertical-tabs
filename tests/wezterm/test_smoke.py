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

    def probe(self, pane_id: int, name: str) -> None: ...

    def adjust_pane_size(self, pane_id: int, direction: str, amount: int) -> None: ...

    def log_has(self, needle: str) -> bool: ...

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


def _split_of(wezterm: WezTerm) -> tuple[dict[str, Any], dict[str, Any]] | None:
    """The one tab's sidebar and content pane, once both exist and the sidebar paints."""
    tabs = _tabs_with_rendering_sidebars(wezterm, 1)
    if not tabs:
        return None
    panes = next(iter(tabs.values()))
    sidebar = next(p for p in panes if str(p.get("title", "")).startswith("wez-vtabs:"))
    content = next(p for p in panes if p is not sidebar)
    return sidebar, content


def _cols(wezterm: WezTerm, pane_id: int) -> int | None:
    for pane in wezterm.panes():
        if pane.get("pane_id") == pane_id:
            size = pane.get("size") or {}
            cols = size.get("cols")
            return cols if isinstance(cols, int) else None
    return None


def test_holds_the_sidebar_width_through_a_window_resize(wezterm_instance: WezTerm) -> None:
    """A window resize deals the sidebar half of every column; it must end where it started."""

    sidebar, content = wezterm_instance.wait_for(
        "one content pane beside one rendering sidebar",
        lambda: _split_of(wezterm_instance),
        consecutive=3,
    )
    sidebar_id, content_id = sidebar["pane_id"], content["pane_id"]
    width = _cols(wezterm_instance, sidebar_id)
    content_width = _cols(wezterm_instance, content_id)
    assert width and content_width

    # Ten frames of growth 100 ms apart, the way `window-resized` arrives during a drag.
    wezterm_instance.probe(content_id, "drag_grow")
    wezterm_instance.wait_for(
        "the content pane to have received the new columns",
        lambda: (_cols(wezterm_instance, content_id) or 0) > content_width,
    )
    wezterm_instance.wait_for(
        "the sidebar width to be held through the resize",
        lambda: _cols(wezterm_instance, sidebar_id) == width,
        consecutive=8,
    )
    grown = _cols(wezterm_instance, content_id)
    assert grown and grown > content_width, "every added column went to the content pane"
    assert wezterm_instance.log_has("vtabs: geometry: tab"), "the frames were corrected as they landed"

    wezterm_instance.probe(content_id, "drag_shrink")
    wezterm_instance.wait_for(
        "the content pane to have given the columns back",
        lambda: (_cols(wezterm_instance, content_id) or 0) < grown,
    )
    wezterm_instance.wait_for(
        "the sidebar width to be held through the shrink",
        lambda: _cols(wezterm_instance, sidebar_id) == width,
        consecutive=8,
    )


def test_adopts_a_divider_move_as_the_new_width(wezterm_instance: WezTerm) -> None:
    """The split moved by hand, with nothing else changed, is the width from then on."""

    sidebar, content = wezterm_instance.wait_for(
        "one content pane beside one rendering sidebar",
        lambda: _split_of(wezterm_instance),
        consecutive=3,
    )
    sidebar_id, content_id = sidebar["pane_id"], content["pane_id"]
    width = _cols(wezterm_instance, sidebar_id)
    assert width
    # A new window's late fit-to-display resizes read as resize frames for 100 ms each; a divider
    # moved inside one is corrected, not adopted, so the window must have come to rest first.
    wezterm_instance.wait_for(
        "the new window to have stopped resizing",
        lambda: _cols(wezterm_instance, sidebar_id) == width,
        consecutive=15,
    )

    # `adjust-pane-size` moves the root split from the content pane, exactly as a divider drag does.
    wezterm_instance.adjust_pane_size(content_id, "Right", 6)
    wezterm_instance.wait_for(
        "the divider move to be adopted rather than corrected",
        lambda: _cols(wezterm_instance, sidebar_id) == width + 6,
        consecutive=10,
    )
    wezterm_instance.probe(content_id, "probe_desired")
    wezterm_instance.wait_for(
        "the plugin to report the moved width as the desired one",
        lambda: wezterm_instance.log_has(f"e2e: desired width {width + 6}"),
    )
