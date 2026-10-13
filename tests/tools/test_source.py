"""Preparation behavior through the real manager and disposable Git repositories."""

import json
from pathlib import Path

import pytest

from tests.tools.conftest import git, write_file

pytestmark = pytest.mark.rust


def files(directory: Path) -> dict[str, bytes]:
    return {
        path.relative_to(directory).as_posix(): path.read_bytes()
        for path in sorted(directory.rglob("*"))
        if path.is_file()
    }


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
    assert (checkout / "wezterm-gui/src/vtabs/src/storage.rs").read_text() == (
        "pub fn fixture() {}\n"
    )
    adapter = checkout / "wezterm-gui/src/vtabs"
    assert (adapter / "src/lib.rs").is_file()
    assert (adapter / "tests/storage.rs").read_text() == "#[test]\nfn storage() {}\n"
    assert (adapter / "Cargo.toml").read_text() == (
        tools_sandbox.root / "src/adapter/Cargo.toml"
    ).read_text()
    prepared = json.loads((tools_sandbox.cache / "prepared.json").read_text())
    assert prepared["upstream"] == revision


def test_prepare_stops_at_first_incompatible_patch(tools_sandbox, local_upstream):
    _, revision = local_upstream
    patch = tools_sandbox.root / "wezterm/patches/0002-second.patch"
    patch.write_text(patch.read_text().replace("-first", "-unexpected upstream contents"))

    result = tools_sandbox.run("--upstream", revision, "prepare", check=False)

    assert result.returncode != 0
    assert (tools_sandbox.cache / "worktree/patch-target.txt").read_text() == "first\n"
    assert not (tools_sandbox.cache / "prepared.json").exists()
    assert not (tools_sandbox.cache / "worktree/wezterm-gui/src/vtabs/src/lib.rs").exists()
    assert "0002-second.patch" in result.stdout + result.stderr


def test_prepare_refuses_an_unowned_worktree_without_touching_files(tools_sandbox, local_upstream):
    _, revision = local_upstream
    sentinel = write_file(tools_sandbox.cache, "worktree/uncommitted.rs", "user edits\n")

    result = tools_sandbox.run("--upstream", revision, "prepare", check=False)

    assert result.returncode != 0
    assert sentinel.read_text() == "user edits\n"
    assert not (tools_sandbox.cache / "prepared.json").exists()


def test_export_preserves_project_edits_made_since_preparation(tools_sandbox, local_upstream):
    _, revision = local_upstream
    project = tools_sandbox.root / "wezterm"
    tools_sandbox.run("--upstream", revision, "prepare")
    write_file(tools_sandbox.cache, "worktree/patch-target.txt", "worktree edit\n")

    second = project / "patches/0002-second.patch"
    second.write_text(second.read_text().replace("+second", "+project edit"))
    before = files(project)

    result = tools_sandbox.run("patch", "export", check=False)

    assert result.returncode != 0
    assert "patches or overlay changed since prepare" in result.stderr
    assert files(project) == before
    assert (tools_sandbox.cache / "worktree/patch-target.txt").read_text() == "worktree edit\n"


def test_exported_patches_reproduce_worktree_edits(tools_sandbox, local_upstream, tmp_path):
    upstream, revision = local_upstream
    tools_sandbox.run("--upstream", revision, "prepare")
    checkout = tools_sandbox.cache / "worktree"
    edited = "fn main() { changed(); }\n"
    write_file(checkout, "wezterm-gui/src/main.rs", edited)
    added = "pub fn added() {}\n"
    write_file(checkout, "wezterm-gui/src/added.rs", added)

    tools_sandbox.run("patch", "export")

    patches = tools_sandbox.root / "wezterm/patches"
    replay = tmp_path / "replay"
    git(upstream, "clone", "--quiet", str(upstream), str(replay))
    git(replay, "checkout", "--quiet", "--detach", revision)
    git(replay, "apply", *map(str, sorted(patches.glob("*.patch"))))
    assert (replay / "wezterm-gui/src/main.rs").read_text() == edited
    assert (replay / "patch-target.txt").read_text() == "second\n"
    assert "vtabs" not in (replay / "wezterm-gui/Cargo.toml").read_text()
    assert (tools_sandbox.root / "wezterm/overlay/wezterm-gui/src/added.rs").read_text() == added
