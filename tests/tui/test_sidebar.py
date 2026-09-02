from __future__ import annotations

import pytest

from .adapter import Terminal
from .support import dress_sidebar, sidebar_model, tab


@pytest.mark.asyncio
@pytest.mark.smoke
async def test_complete_state_renders_visible_tabs(terminal: Terminal) -> None:
    await dress_sidebar(
        terminal,
        sidebar_model([tab(11, "Editor"), tab(22, "Test shell")], active=11),
    )

    await terminal.wait_text("Editor")
    await terminal.wait_text("Test shell")


@pytest.mark.asyncio
async def test_new_model_replaces_the_visible_tab_list(terminal: Terminal) -> None:
    await dress_sidebar(terminal, sidebar_model([tab(11, "Old workspace")]))
    await terminal.wait_text("Old workspace")

    await terminal.send(sidebar_model([tab(22, "New workspace")], rev=2))

    await terminal.wait_text("New workspace")
    await terminal.wait_text("Old workspace", absent=True)


@pytest.mark.asyncio
async def test_clicking_visible_tab_requests_activation_for_that_tab(
    terminal: Terminal,
) -> None:
    await dress_sidebar(
        terminal,
        sidebar_model([tab(11, "Editor"), tab(22, "Dotfiles")], active=11),
    )
    await terminal.wait_text("Dotfiles")
    x, y = await terminal.locate_text("Dotfiles")

    await terminal.mouse_down(x, y)
    await terminal.mouse_up(x, y)

    event = await terminal.wait_event("do", where={"a": "press_card", "id": 22})
    assert event["args"]["part"] == "title"


@pytest.mark.asyncio
async def test_resize_reflows_existing_model_at_the_new_terminal_size(
    terminal: Terminal,
) -> None:
    await dress_sidebar(terminal, sidebar_model([tab(11, "Editor")]))
    await terminal.wait_text("Editor")

    await terminal.resize(18, 12)

    resize = await terminal.wait_event("resize", where={"cols": 18, "rows": 12})
    assert resize["cols"] == 18
    assert resize["rows"] == 12
    await terminal.send({"t": "ping", "n": 74001})
    await terminal.wait_event("pong", where={"echo": 74001})
    await terminal.wait_text("Editor")
