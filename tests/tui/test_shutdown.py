from __future__ import annotations

import pytest


@pytest.mark.asyncio
@pytest.mark.smoke
async def test_quit_restores_terminal_and_exits_successfully(terminal_factory) -> None:
    sentinel = "terminal-restoration"
    terminal = await terminal_factory(post_exit_sentinel=sentinel)

    await terminal.quit()

    assert await terminal.exit_status() == 0
    await terminal.wait_text(f"{sentinel}-before")
    await terminal.wait_text(f"{sentinel}-after")
    await terminal.wait_text(f"{sentinel}-tty-restored")
