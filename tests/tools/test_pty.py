"""Management CLI contracts through tui-test's real pseudoterminal backend."""

import pytest
from tui_test import TuiTest


@pytest.mark.rust
@pytest.mark.pty
@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("arguments", "expected", "code"),
    [
        (["--help"], "prepare", 0),
        (["unknown-command"], "unrecognized subcommand", 2),
        (["doctor", "--unexpected"], "unexpected argument", 2),
    ],
)
async def test_public_management_cli_in_a_pty(
    tools_binary, isolated_env, tmp_path, arguments, expected, code
):
    async with TuiTest.ephemeral(recording={"mode": "disabled"}) as terminal:
        await terminal.run(
            str(tools_binary),
            *arguments,
            cols=140,
            rows=40,
            cwd=str(tmp_path),
            env={"DISPLAY": "", "WAYLAND_DISPLAY": "", **isolated_env},
            wait_ready=False,
        )
        await terminal.wait_exit(timeout=10_000)
        await terminal.get_by_text(expected).any().expect()
        assert (await terminal.state()).exited == code
