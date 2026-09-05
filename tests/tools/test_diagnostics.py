"""Failure replay, cache retention, and version selection are observable CLI contracts."""

import json
import shutil
import subprocess
import sys
from pathlib import Path

import pytest
from filelock import FileLock

from tests.tools.conftest import commit, git, write_file

pytestmark = pytest.mark.rust


def test_repro_preserves_failed_source_revision_configuration_and_logs(
    tools_sandbox, local_upstream
):
    _, revision = local_upstream
    patch = tools_sandbox.root / "native/patches/0002-second.patch"
    patch.write_text(patch.read_text().replace("-first", "-incompatible"))
    tools_sandbox.env["RUSTFLAGS"] = "-C debuginfo=1"
    tools_sandbox.env["UNRELATED_SECRET_TOKEN"] = "must-not-be-recorded"
    failure = tools_sandbox.run("--upstream", revision, "prepare", check=False)
    assert failure.returncode != 0
    report_path = next((tools_sandbox.cache / "runs").glob("*/run.json"))
    report = json.loads(report_path.read_text())
    snapshot = report_path.parent / "source"
    assert report["metadata"]["source_snapshot"] == "source"
    assert (snapshot / "native/patches/0002-second.patch").read_text() == patch.read_text()
    assert report["configuration"]["RUSTFLAGS"] == "-C debuginfo=1"
    assert "UNRELATED_SECRET_TOKEN" not in report["configuration"]
    assert "must-not-be-recorded" not in report_path.read_text()
    patch.write_text(patch.read_text().replace("-incompatible", "-first"))

    def reproduce(*arguments):
        return subprocess.run(
            [
                str(tools_sandbox.binary),
                "--cache",
                str(tools_sandbox.cache),
                "--json",
                "repro",
                str(report_path),
                *arguments,
            ],
            cwd=tools_sandbox.root,
            env=tools_sandbox.env,
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )

    inspected = reproduce()
    assert inspected.returncode == 0, inspected.stderr
    inspection = json.loads(inspected.stdout)
    assert inspection["source"] == str(snapshot)
    assert inspection["upstream"] == revision
    assert inspection["commands"] == report["commands"]
    assert not Path(inspection["replay_root"]).exists()

    replayed = reproduce("--execute")
    assert replayed.returncode != 0
    repeated = list((tools_sandbox.cache / "reproductions").glob("*/cache/runs/*/run.json"))
    assert len(repeated) == 1
    repeated_report = json.loads(repeated[0].read_text())
    assert repeated_report["status"] == "failed"
    assert repeated_report["metadata"]["upstream"]["revision"] == revision
    failed_commands = [
        command for command in repeated_report["commands"] if command["status"] not in (0, None)
    ]
    assert failed_commands[-1]["command"][-1].endswith("0002-second.patch")
    assert (tools_sandbox.cache / "worktree/patch-target.txt").read_text() == "first\n"


def test_cache_gc_dry_run_preserves_active_pending_leased_and_unowned_paths(
    tools_sandbox, bundle_factory
):
    bundles = tools_sandbox.cache / "bundles"
    bundles.mkdir(parents=True)
    paths = {}
    for name in ("active", "pending", "running", "expired"):
        paths[name] = bundles / f"wez-vtabs-native-{name}"
        shutil.copytree(bundle_factory(name), paths[name])
    tools_sandbox.install.mkdir()
    for name in ("active", "pending"):
        write_file(tools_sandbox.install, f"{name}.json", json.dumps({"id": name}))
    sentinel = write_file(bundles, "unowned/user-edits.rs", "user source\n")
    archives = [
        paths["expired"].with_name(paths["expired"].name + suffix)
        for suffix in (".zip", ".zip.manifest.json", ".tar.gz", ".tar.gz.manifest.json")
    ]
    for path in archives:
        path.write_text("archive fixture\n")
    lease = tools_sandbox.cache / "leases/wez-vtabs-native-running.lock"
    lease.parent.mkdir()
    with FileLock(lease):
        preview = tools_sandbox.json("cache", "gc", "--dry-run", "--keep", "0")
        entries = {Path(entry["path"]): entry for entry in preview["entries"]}
        assert entries[paths["expired"]]["action"] == "would_remove"
        assert all(entries[paths[name]]["protected"] for name in ("active", "pending", "running"))
        assert all(path.exists() for path in paths.values())
        assert all(path.exists() for path in archives)

        tools_sandbox.run("cache", "gc", "--keep", "0")

        assert not paths["expired"].exists()
        assert all(paths[name].exists() for name in ("active", "pending", "running"))
        assert not any(path.exists() for path in archives)
        assert sentinel.read_text() == "user source\n"


def test_status_versions_and_rollback_preserve_immutable_bundles(tools_sandbox, bundle_factory):
    tools_sandbox.run("install", "--bundle", bundle_factory("first"))
    tools_sandbox.run("install", "--bundle", bundle_factory("second"))
    tools_sandbox.run("install", "--bundle", bundle_factory("third"), "--stage-only")
    status = tools_sandbox.json("status")
    assert status["active"]["id"] == "second"
    assert status["pending"]["id"] == "third"
    assert status["previous"]["id"] == "first"

    tools_sandbox.run("rollback")

    status = tools_sandbox.json("status")
    assert status["active"]["id"] == "first"
    assert status["pending"] is None
    versions = tools_sandbox.json("versions")
    assert {version["id"] for version in versions} == {"first", "second", "third"}
    assert sum(version["active"] for version in versions) == 1
    assert next(version for version in versions if version["active"])["id"] == "first"


def test_doctor_launch_reports_missing_install_without_development_tools(tools_sandbox):
    tools_sandbox.env["PATH"] = ""

    result = tools_sandbox.run("--json", "doctor", "--for", "launch", check=False)

    assert result.returncode == 1
    report = json.loads(result.stdout)
    assert report["ok"] is False
    assert not any(check["name"] in {"cargo", "git", "rustc"} for check in report["checks"])
    assert any(
        check["name"] == "active bundle" and check["ok"] is False for check in report["checks"]
    )


def test_native_replay_rebuilds_recorded_revision_and_remaps_only_fixture_paths(
    tools_sandbox, local_upstream, recording_cargo, tmp_path
):
    _, revision = local_upstream
    (recording_cargo.parent / "honor-target-dir").touch()
    tools_sandbox.run("--upstream", revision, "build")
    metadata = json.loads((tools_sandbox.cache / "build.json").read_text())
    original_binaries = Path(metadata["artifacts"]["wezterm-gui"]).parent
    uv_calls = tmp_path / "uv-calls.jsonl"
    uv = recording_cargo.parent / "uv"
    uv.write_text(
        f"#!{sys.executable}\nimport json, sys\n"
        f"with open({str(uv_calls)!r}, 'a') as stream:\n"
        "    stream.write(json.dumps(sys.argv[1:]) + '\\n')\n"
        "print('fixture native test failure', file=sys.stderr)\nraise SystemExit(51)\n"
    )
    uv.chmod(0o755)
    original_output = tmp_path / "original-test-output"
    failed = tools_sandbox.run(
        "test",
        "native",
        "--",
        f"--native-bin-dir={original_binaries}",
        f"--basetemp={original_output}",
        "-k",
        "literal and not another",
        check=False,
    )
    assert failed.returncode != 0
    report = next(
        path
        for path in (tools_sandbox.cache / "runs").glob("*/run.json")
        if json.loads(path.read_text())["status"] == "failed"
    )

    result = tools_sandbox.run("repro", report, "--execute", check=False)

    assert result.returncode != 0
    assert "fixture native test failure" in result.stderr
    calls = [json.loads(line) for line in uv_calls.read_text().splitlines()]
    assert len(calls) == 2
    replayed = calls[1]
    assert replayed[replayed.index("-k") + 1] == "literal and not another"
    assert "--project-root" not in replayed
    assert "--cache" not in replayed
    assert "--upstream" not in replayed
    new_binaries = next(
        arg.removeprefix("--native-bin-dir=")
        for arg in replayed
        if arg.startswith("--native-bin-dir=")
    )
    new_output = next(
        arg.removeprefix("--basetemp=") for arg in replayed if arg.startswith("--basetemp=")
    )
    assert new_binaries != str(original_binaries)
    assert new_output != str(original_output)
    assert "reproductions" in new_binaries
    assert "reproductions" in new_output
    built = next((tools_sandbox.cache / "reproductions").glob("*/cache/build.json"))
    assert json.loads(built.read_text())["upstream"] == revision


def test_offline_replay_uses_owned_cached_objects_after_origin_becomes_unavailable(
    tools_sandbox, local_upstream
):
    upstream, _ = local_upstream
    module = upstream.with_name("module-upstream")
    module.mkdir()
    git(module, "init", "--quiet", "--initial-branch=main")
    write_file(module, "module.txt", "cached replay module\n")
    commit(module)
    git(upstream, "-c", "protocol.file.allow=always", "submodule", "add", str(module), "module")
    revision = commit(upstream, "Upstream with replay submodule")
    tools_sandbox.env["GIT_ALLOW_PROTOCOL"] = "file"
    tools_sandbox.run("--upstream", revision, "prepare")
    patch = tools_sandbox.root / "native/patches/0002-second.patch"
    patch.write_text(patch.read_text().replace("-first", "-incompatible"))
    failed = tools_sandbox.run("--offline", "--upstream", revision, "prepare", check=False)
    assert failed.returncode != 0
    original = next(
        path
        for path in (tools_sandbox.cache / "runs").glob("*/run.json")
        if json.loads(path.read_text())["status"] == "failed"
    )
    upstream.rename(upstream.with_name("unavailable-origin"))
    module.rename(module.with_name("unavailable-module-origin"))

    result = tools_sandbox.run("repro", original, "--execute", check=False)

    assert result.returncode != 0
    reports = list((tools_sandbox.cache / "reproductions").glob("*/cache/runs/*/run.json"))
    assert len(reports) == 1, result.stderr
    replayed = json.loads(reports[0].read_text())
    assert replayed["status"] == "failed"
    assert replayed["metadata"]["upstream"]["revision"] == revision
    failed_commands = [
        command for command in replayed["commands"] if command["status"] not in (0, None)
    ]
    assert failed_commands[-1]["command"][-1].endswith("0002-second.patch")
    assert not any("fetch" in command["command"] for command in replayed["commands"])
    assert "upstream cache missing" not in result.stderr
    replay_cache = reports[0].parents[2]
    assert (replay_cache / "worktree/module/module.txt").read_text() == "cached replay module\n"
    assert (
        tools_sandbox.cache / "worktree/module/module.txt"
    ).read_text() == "cached replay module\n"
