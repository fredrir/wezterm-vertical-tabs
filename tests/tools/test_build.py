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


def test_build_reuses_validation_for_docs_but_rechecks_test_and_toolchain_changes(
    tools_sandbox, local_upstream, recording_cargo
):
    _, revision = local_upstream
    tools_sandbox.run("--upstream", revision, "build")
    first = metadata(tools_sandbox)
    assert len(cargo_commands(recording_cargo, "test")) == 3
    assert all("external-target" in path for path in first["artifacts"].values())

    write_file(tools_sandbox.root, "README.md", "Documentation changed\n")
    tools_sandbox.run("--upstream", revision, "--offline", "build")
    docs = metadata(tools_sandbox)
    assert docs["compile_digest"] == first["compile_digest"]
    assert docs["validation_digest"] == first["validation_digest"]
    assert docs["id"] != first["id"]
    assert len(cargo_commands(recording_cargo, "test")) == 3
    assert len(cargo_commands(recording_cargo, "build")) == 4

    write_file(tools_sandbox.root, "tests/tools/test_added.py", "def test_added(): pass\n")
    tools_sandbox.run("--upstream", revision, "--offline", "build")
    tests = metadata(tools_sandbox)
    assert tests["compile_digest"] == docs["compile_digest"]
    assert tests["validation_digest"] != docs["validation_digest"]
    assert len(cargo_commands(recording_cargo, "test")) == 6

    tools_sandbox.env["RUSTFLAGS"] = "-C target-cpu=generic"
    tools_sandbox.run("--upstream", revision, "--offline", "build")
    flags = metadata(tools_sandbox)
    assert flags["compile_digest"] != tests["compile_digest"]
    assert len(cargo_commands(recording_cargo, "test")) == 9


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
