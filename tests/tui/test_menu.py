from __future__ import annotations

import pytest

from .adapter import Terminal
from .support import dress_sidebar, tab


async def event_barrier(terminal: Terminal, echo: int) -> int:
    await terminal.send({"t": "ping", "n": echo})
    pong = await terminal.wait_event("pong", where={"echo": echo})
    return pong["n"]


def menu_picks(events: list[dict[str, object]]) -> list[dict[str, object]]:
    return [
        event for event in events if event.get("t") == "intent" and event.get("a") == "menu_pick"
    ]


@pytest.mark.asyncio
async def test_destructive_menu_item_requires_matching_press_and_release(
    terminal: Terminal,
) -> None:
    safe_label = "KEEP"
    destructive_label = "DESTROY"
    await dress_sidebar(terminal, [tab(7, "Workspace")])
    await terminal.publish(
        {
            "t": "menu",
            "open": True,
            "level": "root",
            "anchor": {"row": 3, "col": 14},
            "target": 7,
            "selected": 2,
            "items": [
                {"id": "keep", "label": safe_label},
                {"id": "destroy", "label": destructive_label, "danger": True},
            ],
        }
    )
    await terminal.wait_text(destructive_label)
    destroy = await terminal.locate_text(destructive_label)
    keep = await terminal.locate_text(safe_label)

    start = await terminal.last_event_sequence()
    await terminal.mouse_down(*destroy)
    after_press = await event_barrier(terminal, 81001)
    assert menu_picks(await terminal.events_after(start)) == []

    await terminal.mouse_up(*keep)
    after_wrong_release = await event_barrier(terminal, 81002)
    assert menu_picks(await terminal.events_after(after_press)) == []

    await terminal.mouse_down(*destroy)
    await terminal.mouse_up(*destroy)
    await event_barrier(terminal, 81003)
    picks = menu_picks(await terminal.events_after(after_wrong_release))
    assert len(picks) == 1
    assert picks[0].get("item_id") == "destroy"


@pytest.mark.asyncio
async def test_unplaceable_menu_emits_typed_refusal(terminal_factory) -> None:
    terminal = await terminal_factory(cols=3)
    await dress_sidebar(terminal, [tab(7, "Workspace")])

    await terminal.publish(
        {
            "t": "menu",
            "open": True,
            "level": "root",
            "anchor": {"row": 3, "col": 1},
            "target": 7,
            "items": [{"id": "keep", "label": "KEEP"}],
        }
    )

    await terminal.wait_event(
        "menu_refused",
        where={"why": "width", "id": 7, "level": "root"},
    )
