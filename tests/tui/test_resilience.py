from __future__ import annotations

import pytest

from .adapter import Terminal
from .support import dress_sidebar, sidebar_model, tab


@pytest.mark.asyncio
async def test_malformed_command_does_not_prevent_later_ping(terminal: Terminal) -> None:
    await terminal.write('{"t": definitely-not-json}\n')

    await terminal.send({"t": "ping", "n": 73001})

    pong = await terminal.wait_event("pong", where={"echo": 73001})
    assert pong["echo"] == 73001


@pytest.mark.asyncio
async def test_unframed_json_and_literal_brace_are_keyboard_input(terminal: Terminal) -> None:
    await terminal.write('\x1eVTABS stranger {"t":"quit"}\n')
    await terminal.send({"t": "ping", "n": 73002})
    await terminal.wait_event("pong", where={"echo": 73002})

    await terminal.write('{"t":"quit"}\n')

    key = await terminal.wait_event("key", where={"key": "{"})
    assert key["key"] == "{"
    await terminal.send({"t": "ping", "n": 73003})
    await terminal.wait_event("pong", where={"echo": 73003})


@pytest.mark.asyncio
async def test_oversized_model_is_dropped_and_last_valid_screen_is_retained(
    terminal: Terminal,
) -> None:
    valid = sidebar_model([tab(1, "Last valid tab")])
    await dress_sidebar(terminal, valid)
    await terminal.wait_text("Last valid tab")
    too_many_tabs = [tab(index, f"Rejected {index}", index=index) for index in range(1, 202)]

    await terminal.send(sidebar_model(too_many_tabs, rev=2))

    await terminal.wait_event(
        "dropped", where={"what": "model", "reason": "bounds"}
    )
    await terminal.wait_text("Last valid tab")
    await terminal.wait_text("Rejected 1", absent=True)
