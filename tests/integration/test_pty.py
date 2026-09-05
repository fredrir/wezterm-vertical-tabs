"""Public CLI behavior through tui-test's real pseudoterminal backend."""

import json
import subprocess
import sys
import time

import pytest
from tui_test import TuiTest

from tests.tools.support import create_bundle


@pytest.fixture
def managed_launcher(native_binaries, tools_binary, rust_host, isolated_env, tmp_path):
    if sys.platform != "linux":
        pytest.skip("managed PTY launcher fixture currently targets Linux bundles")
    bundle = create_bundle(
        tmp_path / "bundle", "pty-fixture", tools_binary, rust_host, native_binaries
    )
    install = tmp_path / "installed"
    env = {
        **isolated_env,
        "WEZ_VTABS_INSTALL": str(install),
        "WEZ_VTABS_CACHE": str(tmp_path / "cache"),
    }
    subprocess.run(
        [
            str(tools_binary),
            "install",
            "--bundle",
            str(bundle),
        ],
        env=env,
        capture_output=True,
        text=True,
        timeout=20,
        check=True,
    )
    (install / "update.json").write_text(json.dumps({"last_attempt": int(time.time())}))
    return install / "wez-vtabs", env


@pytest.mark.native
@pytest.mark.pty
@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("arguments", "expected", "code"),
    [
        (["--help"], "Usage:", 0),
        (["--not-a-wezterm-option"], "unexpected argument", 2),
    ],
)
async def test_installed_wez_vtabs_launcher_in_a_pty(
    managed_launcher, tmp_path, arguments, expected, code
):
    launcher, environment = managed_launcher
    async with TuiTest.ephemeral(recording={"mode": "disabled"}) as terminal:
        await terminal.run(
            str(launcher),
            *arguments,
            cols=120,
            rows=40,
            cwd=str(tmp_path),
            env={"DISPLAY": "", "WAYLAND_DISPLAY": "", **environment},
            wait_ready=False,
        )
        await terminal.wait_exit(timeout=10_000)
        await terminal.get_by_text(expected).any().expect()
        assert (await terminal.state()).exited == code
