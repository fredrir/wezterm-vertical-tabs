"""Build orchestration against Cargo's public artifact stream and real source state."""

import json

import pytest

from tests.tools.conftest import write_file

pytestmark = pytest.mark.rust


def metadata(sandbox):
    return json.loads((sandbox.cache / "build.json").read_text())


def cargo_commands(log, subcommand):
    return [
        arguments
        for line in log.read_text().splitlines()
        if (arguments := json.loads(line))[0] == subcommand
    ]


def test_dev_build_rechecks_cached_binaries_without_running_validation(
    tools_sandbox, local_upstream, recording_cargo
):
    _, revision = local_upstream

    def build():
        before = len(cargo_commands(recording_cargo, "build")) if recording_cargo.exists() else 0
        tools_sandbox.run("--upstream", revision, "--profile", "dev", "build")
        commands = cargo_commands(recording_cargo, "build")[before:]
        assert len(commands) == 2
        packages = [
            command[index + 1]
            for command in commands
            for index, argument in enumerate(command[:-1])
            if argument == "-p"
        ]
        assert set(packages) == {
            "vtabs-store",
            "wezterm-gui",
            "wezterm",
            "wezterm-mux-server",
            "strip-ansi-escapes",
        }

    build()
    # Existing outputs and unchanged helper sources do not establish freshness.
    build()
    assert not cargo_commands(recording_cargo, "test")


def test_build_reuses_validation_for_docs_but_rechecks_changed_tests(
    tools_sandbox, local_upstream, recording_cargo
):
    _, revision = local_upstream
    tools_sandbox.run("--upstream", revision, "build")
    validation = cargo_commands(recording_cargo, "test")
    assert validation
    assert all("external-target" in path for path in metadata(tools_sandbox)["artifacts"].values())

    write_file(tools_sandbox.root, "README.md", "Documentation changed\n")
    tools_sandbox.run("--upstream", revision, "--offline", "build")
    assert cargo_commands(recording_cargo, "test") == validation

    write_file(tools_sandbox.root, "tests/tools/test_added.py", "def test_added(): pass\n")
    tools_sandbox.run("--upstream", revision, "--offline", "build")
    assert len(cargo_commands(recording_cargo, "test")) == len(validation) * 2


def test_build_rejects_source_changes_during_compilation(
    tools_sandbox, local_upstream, recording_cargo
):
    _, revision = local_upstream
    (recording_cargo.parent / "change-source").write_text(str(tools_sandbox.root / "README.md"))

    result = tools_sandbox.run("--upstream", revision, "build", check=False)

    assert result.returncode != 0
    assert "project source changed during build" in result.stderr
    assert not (tools_sandbox.cache / "build.json").exists()
    report = json.loads(next((tools_sandbox.cache / "runs").glob("*/run.json")).read_text())
    assert report["status"] == "failed"
    assert report["metadata"]["upstream"]["revision"] == revision
    assert "rustc" in report["metadata"]["build_configuration"]
    assert "resolved_locks" in report["metadata"]
