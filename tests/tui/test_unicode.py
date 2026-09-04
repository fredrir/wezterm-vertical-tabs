from __future__ import annotations

import pytest

from .adapter import Terminal
from .support import dress_sidebar, tab


@pytest.mark.asyncio
async def test_unicode_title_cannot_inject_terminal_controls(terminal: Terminal) -> None:
    original_terminal_title = await terminal.title()
    unsafe_title = "Café東京\x1b]0;hijacked\x07\u202e"

    await dress_sidebar(terminal, [tab(7, unsafe_title)])
    await terminal.send({"t": "ping", "n": 82001})
    await terminal.wait_event("pong", where={"echo": 82001})

    screen = await terminal.text()
    assert "Café東京" in screen
    assert await terminal.title() == original_terminal_title
    assert "\x1b" not in screen
    assert "\x07" not in screen
    assert "\u202e" not in screen
