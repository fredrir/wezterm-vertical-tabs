"""Public CLI behavior through tui-test's real pseudoterminal backend."""

import json
import shutil
import subprocess
import sys
import time

import pytest
from tui_test import TuiTest


@pytest.mark.pty
@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("arguments", "expected", "code"),
    [
        (["--help"], "prepare,build,dev,check,package,install,update,launch,doctor", 0),
        (["unknown-command"], "invalid choice", 2),
        (["doctor", "--unexpected"], "unexpected arguments", 2),
    ],
)
async def test_public_management_cli_in_a_pty(
    project_root, isolated_env, tmp_path, arguments, expected, code
):
    async with TuiTest.ephemeral(recording={"mode": "disabled"}) as terminal:
        await terminal.run(
            sys.executable,
            str(project_root / "scripts/native.py"),
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


@pytest.fixture
def managed_launcher(native_binaries, project_root, isolated_env, tmp_path):
    if sys.platform != "linux":
        pytest.skip("managed PTY launcher fixture currently targets Linux bundles")
    bundle = tmp_path / "bundle"
    binaries = bundle / "bin"
    binaries.mkdir(parents=True)
    source = bundle / "source"
    (source / "scripts").mkdir(parents=True)
    shutil.copy2(project_root / "Cargo.toml", source / "Cargo.toml")
    shutil.copy2(project_root / "scripts/native.py", source / "scripts/native.py")
    for name in ("wezterm-gui", "wez-vtabs-store"):
        shutil.copy2(native_binaries[name], binaries / name)
    (bundle / "build.json").write_text(json.dumps({"id": "pty-fixture", "capability": 1}))
    install = tmp_path / "installed"
    env = {
        **isolated_env,
        "WEZ_VTABS_INSTALL": str(install),
        "WEZ_VTABS_CACHE": str(tmp_path / "cache"),
    }
    subprocess.run(
        [
            sys.executable,
            str(project_root / "scripts/native.py"),
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
