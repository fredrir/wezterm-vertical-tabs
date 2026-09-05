"""The temporary Python handoff keeps already installed updaters compatible."""

import json
import shutil
import subprocess
import sys

import pytest

pytestmark = pytest.mark.rust


def test_legacy_updater_entry_forwards_arguments_and_exit_status_to_rust(
    project_root, tools_sandbox, failing_cargo, tmp_path
):
    legacy = tools_sandbox.root / "scripts/native.py"
    legacy.parent.mkdir()
    shutil.copy2(project_root / "scripts/native.py", legacy)
    output = tmp_path / "output with spaces and $symbols"
    result = subprocess.run(
        [sys.executable, str(legacy), "update", "--stage-only", "--output", str(output)],
        env={**tools_sandbox.env, "WEZ_VTABS_UPDATE_SOURCE_SYNCED": "1"},
        text=True,
        capture_output=True,
        timeout=20,
        check=False,
    )

    assert result.returncode == 61
    commands = [json.loads(line) for line in failing_cargo.read_text().splitlines()]
    assert len(commands) == 1
    command = commands[0]
    assert command[:2] == ["run", "--quiet"]
    assert command[command.index("--manifest-path") + 1] == str(
        tools_sandbox.root / "tools/Cargo.toml"
    )
    assert command[command.index("--") + 1 :] == [
        "--project-root",
        str(tools_sandbox.root),
        "update",
        "--stage-only",
        "--output",
        str(output),
    ]
