"""Failure replay, cache retention, and installed launch diagnostics."""

import json
import shutil
import subprocess
from pathlib import Path

import pytest
from filelock import FileLock

from tests.tools.conftest import write_file

pytestmark = pytest.mark.rust


def test_repro_preserves_failed_source_revision_configuration_and_logs(
    tools_sandbox, local_upstream
):
    _, revision = local_upstream
    patch = tools_sandbox.root / "wezterm/patches/0002-second.patch"
    patch.write_text(patch.read_text().replace("-first", "-incompatible"))
    tools_sandbox.env["RUSTFLAGS"] = "-C debuginfo=1"
    tools_sandbox.env["UNRELATED_SECRET_TOKEN"] = "must-not-be-recorded"
    failure = tools_sandbox.run("--upstream", revision, "prepare", check=False)
    assert failure.returncode != 0
    report_path = next((tools_sandbox.cache / "runs").glob("*/run.json"))
    report = json.loads(report_path.read_text())
    snapshot = report_path.parent / "source"
    assert report["metadata"]["source_snapshot"] == "source"
    assert (snapshot / "wezterm/patches/0002-second.patch").read_text() == patch.read_text()
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
        paths[name] = bundles / f"wez-vtabs-{name}"
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
    lease = tools_sandbox.cache / "leases/wez-vtabs-running.lock"
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
