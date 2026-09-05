"""Preparation behavior through the real manager and disposable Git repositories."""

import json
import os
import sys

import pytest

from tests.tools.conftest import commit, git, write_file

pytestmark = pytest.mark.rust


def test_prepare_applies_ordered_patches_and_wires_project_dependencies(
    tools_sandbox, local_upstream
):
    _, revision = local_upstream
    tools_sandbox.run("--upstream", revision, "prepare")

    checkout = tools_sandbox.cache / "worktree"
    assert (checkout / "patch-target.txt").read_text() == "second\n"
    manifest = (checkout / "wezterm-gui/Cargo.toml").read_text()
    assert "default-features = false" in manifest
    assert "vtabs-app" in manifest
    assert "vtabs-store" in manifest
    assert manifest.count("termwiz") == 1
    assert (checkout / "wezterm-gui/src/native_vtabs/storage.rs").read_text() == (
        "pub fn fixture() {}\n"
    )
    prepared = json.loads((tools_sandbox.cache / "prepared.json").read_text())
    assert prepared["upstream"] == revision


def test_prepare_stops_at_first_incompatible_patch_and_records_failure(
    tools_sandbox, local_upstream
):
    _, revision = local_upstream
    patch = tools_sandbox.root / "native/patches/0002-second.patch"
    patch.write_text(patch.read_text().replace("-first", "-unexpected upstream contents"))

    result = tools_sandbox.run("--upstream", revision, "prepare", check=False)

    assert result.returncode != 0
    assert (tools_sandbox.cache / "worktree/patch-target.txt").read_text() == "first\n"
    assert not (tools_sandbox.cache / "prepared.json").exists()
    assert not (tools_sandbox.cache / "worktree/wezterm-gui/src/native_vtabs.rs").exists()
    assert "0002-second.patch" in result.stdout + result.stderr
    reports = list((tools_sandbox.cache / "runs").glob("*/run.json"))
    assert len(reports) == 1
    report = json.loads(reports[0].read_text())
    assert report["status"] == "failed"
    assert report["version"] == 1
    assert revision in json.dumps(report)
    assert report["configuration"]["WEZ_VTABS_UPSTREAM_URL"] == str(local_upstream[0])
    failed = [command for command in report["commands"] if command["status"] not in (None, 0)]
    assert len(failed) == 1
    assert failed[0]["command"][-1] == str(patch)
    assert failed[0]["cwd"] == str(tools_sandbox.cache / "worktree")
    assert failed[0]["error"]
    assert all(
        (reports[0].parent / command[stream]).is_file()
        for command in report["commands"]
        for stream in ("stdout", "stderr")
    )
    logs = list(reports[0].parent.rglob("*.stderr.log"))
    assert any("patch-target.txt" in log.read_text() for log in logs)

    patch.write_text(patch.read_text().replace("-unexpected upstream contents", "-first"))
    tools_sandbox.run("--upstream", revision, "--offline", "prepare")
    assert (tools_sandbox.cache / "worktree/patch-target.txt").read_text() == "second\n"


def test_adapter_sync_reuses_checkout_and_removes_deleted_modules(tools_sandbox, local_upstream):
    _, revision = local_upstream
    write_file(tools_sandbox.root, "native/adapter/obsolete/nested.rs", "pub fn obsolete() {}\n")
    tools_sandbox.run("--upstream", revision, "prepare")
    checkout = tools_sandbox.cache / "worktree"
    sentinel = write_file(checkout, "keep-checkout.txt", "existing checkout\n")
    module = checkout / "wezterm-gui/src/native_vtabs/storage.rs"
    previous_mtime = module.stat().st_mtime_ns
    removed = tools_sandbox.root / "native/adapter/obsolete/nested.rs"
    removed.unlink()
    removed.parent.rmdir()
    write_file(tools_sandbox.root, "native/adapter/new_module.rs", "pub fn added() {}\n")

    tools_sandbox.run("--upstream", revision, "--offline", "prepare")

    assert sentinel.read_text() == "existing checkout\n"
    assert module.stat().st_mtime_ns == previous_mtime
    assert not (module.parent / "obsolete").exists()
    assert (module.parent / "new_module.rs").is_file()
    assert (checkout / "wezterm-gui/Cargo.toml").read_text().count("vtabs-app") == 2


def test_pinned_revision_can_prepare_offline_after_upstream_moves(tools_sandbox, local_upstream):
    upstream, revision = local_upstream
    tools_sandbox.run("--upstream", revision, "prepare")
    write_file(upstream, "later.txt", "new upstream commit\n")
    later_revision = commit(upstream, "Upstream moved")
    assert later_revision != revision
    upstream.rename(upstream.with_name("unavailable-upstream"))

    tools_sandbox.run("--upstream", revision, "--offline", "prepare")

    prepared = json.loads((tools_sandbox.cache / "prepared.json").read_text())
    assert prepared["upstream"] == revision
    assert not (tools_sandbox.cache / "worktree/later.txt").exists()


def test_offline_missing_revision_fails_without_fetching(tools_sandbox, local_upstream):
    _, revision = local_upstream
    tools_sandbox.run("--upstream", revision, "prepare")

    result = tools_sandbox.run("--upstream", "f" * 40, "--offline", "prepare", check=False)

    assert result.returncode != 0
    assert "offline" in (result.stdout + result.stderr).lower()
    prepared = json.loads((tools_sandbox.cache / "prepared.json").read_text())
    assert prepared["upstream"] == revision


def test_prepare_refuses_an_unowned_worktree_without_touching_files(tools_sandbox, local_upstream):
    _, revision = local_upstream
    sentinel = write_file(tools_sandbox.cache, "worktree/uncommitted.rs", "user edits\n")

    result = tools_sandbox.run("--upstream", revision, "prepare", check=False)

    assert result.returncode != 0
    assert sentinel.read_text() == "user edits\n"
    assert not (tools_sandbox.cache / "prepared.json").exists()


@pytest.mark.skipif(os.name == "nt", reason="POSIX controlled Git transport fixture")
def test_offline_preparation_disables_even_explicitly_allowed_submodule_transports(
    tools_sandbox, local_upstream, tmp_path
):
    upstream, revision = local_upstream
    sentinel = tmp_path / "transport-was-executed"
    transport = tmp_path / "transport.py"
    transport.write_text(
        f"from pathlib import Path\nPath({str(sentinel)!r}).write_text('transport executed')\n"
        "raise SystemExit(1)\n"
    )
    write_file(
        upstream,
        ".gitmodules",
        f'[submodule "probe"]\n    path = probe\n    url = ext::{sys.executable} {transport}\n',
    )
    git(upstream, "add", ".gitmodules")
    git(upstream, "update-index", "--add", "--cacheinfo", f"160000,{revision},probe")
    git(
        upstream,
        "-c",
        "user.name=Tooling test",
        "-c",
        "user.email=tooling@example.invalid",
        "commit",
        "--quiet",
        "-m",
        "Fixture with external submodule transport",
    )
    revision = git(upstream, "rev-parse", "HEAD")
    checkout = tools_sandbox.cache / "upstream"
    checkout.parent.mkdir()
    git(upstream, "clone", str(upstream), str(checkout))
    write_file(
        checkout,
        ".git/wez-vtabs-native.json",
        json.dumps({"path": str(checkout.resolve()), "remote": str(upstream), "capability": 1}),
    )
    tools_sandbox.env["GIT_ALLOW_PROTOCOL"] = "ext"

    result = tools_sandbox.run("--offline", "--upstream", revision, "prepare", check=False)

    assert result.returncode != 0
    assert not sentinel.exists()
    assert not (tools_sandbox.cache / "prepared.json").exists()


def test_offline_patch_changes_reuse_cached_submodule_objects(
    tools_sandbox, local_upstream, tmp_path
):
    upstream, _ = local_upstream
    module = tmp_path / "module-upstream"
    module.mkdir()
    git(module, "init", "--quiet", "--initial-branch=main")
    write_file(module, "module.txt", "cached module source\n")
    commit(module)
    git(upstream, "-c", "protocol.file.allow=always", "submodule", "add", str(module), "module")
    revision = commit(upstream, "Upstream with local submodule")
    tools_sandbox.env["GIT_ALLOW_PROTOCOL"] = "file"
    tools_sandbox.run("--upstream", revision, "prepare")
    module.rename(module.with_name("unavailable-module"))
    patch = tools_sandbox.root / "native/patches/0002-second.patch"
    patch.write_text(patch.read_text().replace("+second", "+changed second"))

    tools_sandbox.run("--offline", "--upstream", revision, "prepare")

    assert (tools_sandbox.cache / "worktree/patch-target.txt").read_text() == "changed second\n"
    assert (
        tools_sandbox.cache / "worktree/module/module.txt"
    ).read_text() == "cached module source\n"
