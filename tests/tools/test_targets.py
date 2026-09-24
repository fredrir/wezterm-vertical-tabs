"""Machines from targets.toml: resolution, reachability and delegation over SSH."""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

import pytest

pytestmark = [
    pytest.mark.rust,
    pytest.mark.skipif(os.name == "nt", reason="POSIX fixture command dispatch"),
]

TARGETS = """
on = "archie"

[targets.macie]
host = "macie"
platform = "macos"

[targets.archie]
host = "archie"
platform = "arch"
role = "mux"

[targets.ntnu]
host = "ntnu"
platform = "ubuntu-26.04"
role = "mux"
"""

SOURCE = ".cache/wez-vtabs/remote/macie/source"


@pytest.fixture
def fleet(tools_sandbox, tmp_path):
    config = Path(tools_sandbox.env["XDG_CONFIG_HOME"]) / "wez-vtabs/targets.toml"
    config.parent.mkdir(parents=True)
    config.write_text(TARGETS)
    tools_sandbox.env["WEZ_VTABS_HOST"] = "macie"
    return tools_sandbox


def fake_commands(sandbox, directory: Path, ssh_status: int = 0) -> Path:
    directory.mkdir()
    log = directory / "calls.jsonl"
    for name, status in (("ssh", ssh_status), ("rsync", 0)):
        command = directory / name
        command.write_text(
            f"#!{sys.executable}\n"
            "import json, sys\n"
            f"with open({str(log)!r}, 'a', encoding='utf-8') as stream:\n"
            f"    stream.write(json.dumps([{name!r}, *sys.argv[1:]]) + '\\n')\n"
            f"raise SystemExit({status})\n",
            encoding="utf-8",
        )
        command.chmod(0o755)
    sandbox.env["PATH"] = str(directory) + os.pathsep + sandbox.env["PATH"]
    return log


def calls(log: Path) -> list[list[str]]:
    return [json.loads(line) for line in log.read_text().splitlines()]


def test_to_selects_the_machine_platform_and_role(fleet):
    data = fleet.json("--explain", "build", "--to", "ntnu", "--on", "local")

    assert data["platform"] == "ubuntu-26.04"
    assert data["role"] == "mux"
    assert data["command"][-6:-2] == ["package", "--role", "mux", "--no-archive"]


def test_unknown_machine_names_the_targets_file(fleet):
    result = fleet.run("status", "--to", "nowhere", check=False)

    assert result.returncode == 1
    assert "unknown machine: nowhere" in result.stderr
    assert "targets.toml" in result.stderr


def test_machines_require_a_targets_file(tools_sandbox):
    result = tools_sandbox.run("status", "--to", "ntnu", check=False)

    assert result.returncode == 1
    assert "targets file missing" in result.stderr


def test_unreachable_target_fails_before_any_work(fleet, tmp_path):
    log = fake_commands(fleet, tmp_path / "bin", ssh_status=255)

    result = fleet.run("deploy", "--to", "ntnu", check=False)

    assert result.returncode == 1
    assert result.stderr == "Error: Unreachable (archie)\n"
    assert [call[0] for call in calls(log)] == ["ssh"]


def test_tests_run_on_the_default_machine_against_the_synced_working_tree(fleet, tmp_path):
    log = fake_commands(fleet, tmp_path / "bin")

    fleet.run("test", "lua", "--", "-k", "plugin")

    rsync = next(call for call in calls(log) if call[0] == "rsync")
    assert rsync[-1] == f"archie:{SOURCE}/"
    assert {"src", "plugin", "Cargo.toml"} <= set(rsync)
    assert "--delete" in rsync
    assert "--exclude=target" in rsync
    script = calls(log)[-1][-1]
    assert script.endswith(
        f'cd {SOURCE} && exec cargo xtask --project-root "$HOME/{SOURCE}" '
        '--cache "$HOME/.cache/wez-vtabs/remote/macie/cache" test lua --on local -- -k plugin'
    )


def test_on_local_overrides_the_default_machine(fleet, tmp_path):
    log = fake_commands(fleet, tmp_path / "bin")

    result = fleet.run("lint", "--on", "local", check=False)

    assert not log.exists()
    assert "Unreachable" not in result.stderr


def test_deploy_on_the_receiving_machine_runs_there(fleet, tmp_path):
    log = fake_commands(fleet, tmp_path / "bin")

    fleet.run("deploy", "--on", "archie", "--to", "archie", check=False)

    script = next(call[-1] for call in calls(log) if "cargo xtask" in call[-1])
    assert script.endswith("deploy --on local --role mux")


def test_this_machine_runs_locally_without_ssh(fleet, tmp_path):
    log = fake_commands(fleet, tmp_path / "bin")

    status = fleet.json("status", "--to", "macie")

    assert status["active"] is None
    assert not log.exists()


def test_remote_rollback_uses_the_deployed_tool(fleet, tmp_path):
    log = fake_commands(fleet, tmp_path / "bin")

    fleet.run("deploy", "--to", "ntnu", "--rollback", "some-version")

    assert calls(log)[-1][-1].endswith(
        'exec "$HOME/.local/bin/wez-vtabs" deploy --rollback some-version'
    )
