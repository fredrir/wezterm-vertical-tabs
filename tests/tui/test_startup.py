from __future__ import annotations

import pytest

from .adapter import COLS, ROWS, Terminal


@pytest.mark.asyncio
async def test_startup_announces_sidebar_role(terminal: Terminal) -> None:
    assert await terminal.wait_user_var("vtabs_role") == "sidebar"


@pytest.mark.asyncio
@pytest.mark.smoke
async def test_startup_reports_terminal_size_and_paint_capability(terminal: Terminal) -> None:
    ready = await terminal.wait_event("ready")

    assert ready["v"] == 3
    assert ready["cols"] == COLS
    assert ready["rows"] == ROWS
    assert ready["paints"] is True
