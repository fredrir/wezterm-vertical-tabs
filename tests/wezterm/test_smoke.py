"""Behavioral contracts across a real standalone WezTerm mux domain."""

from __future__ import annotations

from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    from conftest import TabSnapshot, Topology, WezTermMuxInstance


def _only_tab(topology: Topology) -> TabSnapshot:
    assert len(topology.tabs) == 1, f"expected one tab, got {topology.as_list()!r}"
    return topology.tabs[0]


@pytest.mark.smoke
def test_standalone_mux_tab_has_one_rendering_sidebar(
    wezterm_mux: WezTermMuxInstance,
) -> None:
    """The GUI and server agree on one usable sidebar beside the content pane."""

    mux = wezterm_mux.wait_ready_tabs(1)
    server, gui = wezterm_mux.wait_same_topology(mux.pane_ids)

    assert server.shape == gui.shape == ((1, 1),)
    assert wezterm_mux.clients(), "the standalone mux should report its attached GUI client"


@pytest.mark.smoke
def test_gui_reconnect_adopts_the_surviving_sidebar(
    wezterm_mux: WezTermMuxInstance,
) -> None:
    """Disconnecting the GUI must preserve work and reconnect without a duplicate sidebar."""

    before = wezterm_mux.wait_ready_tabs(1, stable_for=0.5)
    before_tab = _only_tab(before)
    before_ids = before.pane_ids
    sidebar_id = before_tab.sidebars[0].pane_id

    wezterm_mux.disconnect_gui()
    disconnected = wezterm_mux.wait_topology(
        "all mux panes to survive without a GUI client",
        lambda topology: topology.pane_ids == before_ids,
        stable_for=0.5,
    )
    assert disconnected.pane_ids == before_ids

    wezterm_mux.start_gui()
    server, reconnected = wezterm_mux.wait_same_topology(before_ids)
    reconnected = wezterm_mux.wait_topology(
        "the reconnected GUI to remain duplicate-free",
        lambda topology: topology.shape == ((1, 1),),
        endpoint="gui",
        stable_for=1.0,
    )
    assert server.pane_ids == before_ids
    assert _only_tab(server).sidebars[0].pane_id == sidebar_id
    assert reconnected.shape == ((1, 1),)


@pytest.mark.smoke
def test_new_mux_tab_gets_one_sidebar_without_disturbing_the_original(
    wezterm_mux: WezTermMuxInstance,
) -> None:
    """A newly spawned tab converges independently while the first tab remains intact."""

    before = wezterm_mux.wait_ready_tabs(1)
    original = _only_tab(before)
    gui_before = wezterm_mux.wait_ready_tabs(1, endpoint="gui")
    new_content_id = wezterm_mux.spawn_tab(_only_tab(gui_before).content[0].pane_id)

    after = wezterm_mux.wait_ready_tabs(2)
    _, gui_after = wezterm_mux.wait_same_topology(after.pane_ids)

    assert original.tab_id in {tab.tab_id for tab in after.tabs}
    assert new_content_id in gui_after.pane_ids
    assert all(len(tab.sidebars) == 1 and tab.content for tab in after.tabs)


def test_hiding_and_restoring_sidebar_preserves_content(
    wezterm_mux: WezTermMuxInstance,
) -> None:
    """Toggling the sidebar never replaces or terminates the user's content pane."""

    before = wezterm_mux.wait_ready_tabs(1)
    tab = _only_tab(before)
    content_ids = frozenset(pane.pane_id for pane in tab.content)
    wezterm_mux.probe(tab.content[0].pane_id, "hide_sidebar")

    hidden = wezterm_mux.wait_topology(
        "the sidebar to disappear while content remains",
        lambda topology: (
            len(topology.tabs) == 1
            and not topology.tabs[0].sidebars
            and frozenset(pane.pane_id for pane in topology.tabs[0].content) == content_ids
        ),
        stable_for=0.3,
    )
    assert frozenset(pane.pane_id for pane in _only_tab(hidden).content) == content_ids

    wezterm_mux.probe(next(iter(content_ids)), "toggle")
    restored = wezterm_mux.wait_ready_tabs(1)
    assert frozenset(pane.pane_id for pane in _only_tab(restored).content) == content_ids


def test_window_resize_preserves_sidebar_width(wezterm_mux: WezTermMuxInstance) -> None:
    """Window growth and shrinkage are absorbed by content instead of the sidebar."""

    before = wezterm_mux.wait_ready_tabs(1, stable_for=0.5)
    tab = _only_tab(before)
    sidebar = tab.sidebars[0]
    content = tab.content[0]

    wezterm_mux.probe(content.pane_id, "grow")
    grown = wezterm_mux.wait_topology(
        "window growth to reach content while preserving the sidebar width",
        lambda topology: (
            topology.pane(sidebar.pane_id).cols == sidebar.cols
            and topology.pane(content.pane_id).cols > content.cols
        ),
        stable_for=0.5,
    )
    grown_content_cols = grown.pane(content.pane_id).cols

    wezterm_mux.probe(content.pane_id, "shrink")
    shrunk = wezterm_mux.wait_topology(
        "window shrinkage to leave content while preserving the sidebar width",
        lambda topology: (
            topology.pane(sidebar.pane_id).cols == sidebar.cols
            and topology.pane(content.pane_id).cols < grown_content_cols
        ),
        stable_for=0.5,
    )
    assert shrunk.pane(sidebar.pane_id).cols == sidebar.cols


def test_manual_divider_width_survives_the_next_resize(
    wezterm_mux: WezTermMuxInstance,
) -> None:
    """A user-selected width becomes the observable width retained on later resize."""

    before = wezterm_mux.wait_ready_tabs(1, stable_for=1.0)
    tab = _only_tab(before)
    sidebar = tab.sidebars[0]
    content = tab.content[0]
    adopted_width = sidebar.cols + 6

    wezterm_mux.adjust_pane_size(content.pane_id, "Right", 6)
    moved = wezterm_mux.wait_topology(
        "the divider move to settle at the requested width",
        lambda topology: topology.pane(sidebar.pane_id).cols == adopted_width,
        stable_for=0.8,
    )
    moved_content_cols = moved.pane(content.pane_id).cols

    wezterm_mux.probe(content.pane_id, "grow")
    resized = wezterm_mux.wait_topology(
        "a later resize to preserve the adopted divider width",
        lambda topology: (
            topology.pane(sidebar.pane_id).cols == adopted_width
            and topology.pane(content.pane_id).cols > moved_content_cols
        ),
        stable_for=0.5,
    )
    assert resized.pane(sidebar.pane_id).cols == adopted_width


def test_split_started_from_sidebar_is_rescued_into_usable_content(
    wezterm_mux: WezTermMuxInstance,
) -> None:
    """A shell accidentally split from the sidebar is moved out of its narrow band."""

    before = wezterm_mux.wait_ready_tabs(1)
    tab = _only_tab(before)
    original_ids = before.pane_ids
    wezterm_mux.probe(tab.content[0].pane_id, "split_sidebar_h")

    rescued = wezterm_mux.wait_topology(
        "the new shell to be rescued beside the one full-height sidebar",
        lambda topology: (
            len(topology.tabs) == 1
            and len(topology.tabs[0].sidebars) == 1
            and len(topology.tabs[0].content) == 2
            and all(
                pane.cols > topology.tabs[0].sidebars[0].cols for pane in topology.tabs[0].content
            )
        ),
        timeout=15,
        stable_for=0.5,
    )
    new_content = next(
        pane for pane in _only_tab(rescued).content if pane.pane_id not in original_ids
    )
    marker = f"rescued-pane-{new_content.pane_id}"
    wezterm_mux.send_shell(new_content.pane_id, f"printf '{marker}\\n'")

    def text_with_marker() -> str | None:
        text = wezterm_mux.pane_text(new_content.pane_id)
        return text if marker in text else None

    text = wezterm_mux.wait_for("the rescued shell to execute input", text_with_marker)
    assert marker in text


def test_local_and_standalone_mux_tabs_stay_on_their_own_endpoints(
    wezterm_mux: WezTermMuxInstance,
) -> None:
    """Adding a local tab must not migrate or duplicate the standalone mux panes."""

    mux_before = wezterm_mux.wait_ready_tabs(1)
    mux_ids = mux_before.pane_ids
    gui_before = wezterm_mux.wait_ready_tabs(1, endpoint="gui")
    window_id = _only_tab(gui_before).panes[0].window_id
    local_content_id = wezterm_mux.spawn_local_tab(window_id)

    gui = wezterm_mux.wait_ready_tabs(2, endpoint="gui")
    mux_after = wezterm_mux.wait_topology(
        "the standalone mux to retain only its original panes",
        lambda topology: topology.pane_ids == mux_ids,
        stable_for=0.5,
    )
    local_tab = next(
        tab for tab in gui.tabs if local_content_id in {pane.pane_id for pane in tab.panes}
    )

    assert mux_after.pane_ids == mux_ids
    assert len(local_tab.sidebars) == 1


def test_rapid_native_tab_switching_leaves_every_tab_intact(
    wezterm_mux: WezTermMuxInstance,
) -> None:
    """A held tab-switch key must neither pile up splits and adjusts nor destabilize the GUI."""

    before = wezterm_mux.wait_ready_tabs(1)
    gui_before = wezterm_mux.wait_ready_tabs(1, endpoint="gui")
    gui_content_id = _only_tab(gui_before).content[0].pane_id
    for count in range(2, 5):
        wezterm_mux.spawn_tab(gui_content_id)
        wezterm_mux.wait_ready_tabs(count)
    settled = wezterm_mux.wait_ready_tabs(4, stable_for=0.5)
    pane_ids = settled.pane_ids

    # Three bursts of a held key, each let settle before the next: the plugin serves the tab the
    # hand stops on and must have split nothing into the ones it passed through.
    for _ in range(3):
        for _ in range(12):
            wezterm_mux.activate_tab_relative(gui_content_id, 1)
        wezterm_mux.wait_topology(
            "the burst to settle on four intact tabs",
            lambda topology: topology.pane_ids == pane_ids and topology.shape == ((1, 1),) * 4,
            stable_for=0.4,
        )

    after = wezterm_mux.wait_topology(
        "every tab to still hold its own content and exactly one sidebar",
        lambda topology: topology.pane_ids == pane_ids and topology.shape == ((1, 1),) * 4,
        stable_for=1.0,
    )
    assert after.pane_ids == pane_ids, "nothing was split into or closed out of a passing tab"
    assert wezterm_mux.clients(), "the GUI is still attached"
    assert before.tabs[0].tab_id in {tab.tab_id for tab in after.tabs}


def test_settings_page_closes_as_a_whole_tab(wezterm_mux: WezTermMuxInstance) -> None:
    """Closing the settings page never leaves a tab holding only its sidebar."""

    before = wezterm_mux.wait_ready_tabs(1)
    content_id = _only_tab(before).content[0].pane_id
    wezterm_mux.probe(content_id, "settings")
    opened = wezterm_mux.wait_topology(
        "the settings page to open beside its own sidebar",
        lambda topology: len(topology.tabs) == 2 and topology.shape == ((1, 1), (1, 1)),
        stable_for=0.5,
    )
    page_tab = next(tab for tab in opened.tabs if tab.tab_id != _only_tab(before).tab_id)
    page_ids = frozenset(pane.pane_id for pane in page_tab.panes)

    wezterm_mux.probe(content_id, "close_settings")
    closed = wezterm_mux.wait_topology(
        "the settings tab to be gone with both of its panes",
        lambda topology: len(topology.tabs) == 1 and not (topology.pane_ids & page_ids),
        stable_for=0.5,
    )
    assert closed.pane_ids == before.pane_ids
    assert closed.shape == ((1, 1),)
