"""Preparation behavior through the real manager and disposable Git repositories."""

import json
import os
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest

from tests.tools.conftest import commit, git, write_file

pytestmark = pytest.mark.rust

EXPORT_DIFF = (
    "diff",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--no-renames",
    "--ignore-submodules=all",
    "--full-index",
    "--diff-algorithm=myers",
    "--indent-heuristic",
    "-U3",
    "--inter-hunk-context=0",
    "--src-prefix=a/",
    "--dst-prefix=b/",
)


def exported_diff(checkout: Path, revision: str, entry: str) -> bytes:
    return subprocess.run(
        ["git", *EXPORT_DIFF, revision, "--", entry],
        cwd=checkout,
        env={**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"},
        capture_output=True,
        timeout=20,
        check=True,
    ).stdout


def use_entry_patches(
    project: Path, upstream: Path, revision: str, changes: dict[str, str], scratch: Path
) -> Path:
    """Replace the ordered fixture series with one canonical patch per top-level entry."""
    git(upstream, "clone", "--quiet", str(upstream), str(scratch))
    git(scratch, "checkout", "--quiet", "--detach", revision)
    for name, contents in changes.items():
        write_file(scratch, name, contents)
    patches = project / "wezterm/patches"
    for legacy in patches.glob("*.patch"):
        legacy.unlink()
    for entry in sorted({name.split("/")[0] for name in changes}):
        (patches / f"{entry}.patch").write_bytes(exported_diff(scratch, revision, entry))
    return scratch


def files(directory: Path) -> dict[str, bytes]:
    return {
        path.relative_to(directory).as_posix(): path.read_bytes()
        for path in sorted(directory.rglob("*"))
        if path.is_file()
    }


def drift_lines(**replacements: str) -> str:
    return "".join(f"{replacements.get(f'l{n}', f'line {n}')}\n" for n in range(1, 13))


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


def test_prepare_stops_at_first_incompatible_patch_and_records_failure(
    tools_sandbox, local_upstream
):
    _, revision = local_upstream
    patch = tools_sandbox.root / "wezterm/patches/0002-second.patch"
    patch.write_text(patch.read_text().replace("-first", "-unexpected upstream contents"))

    result = tools_sandbox.run("--upstream", revision, "prepare", check=False)

    assert result.returncode != 0
    assert (tools_sandbox.cache / "worktree/patch-target.txt").read_text() == "first\n"
    assert not (tools_sandbox.cache / "prepared.json").exists()
    assert not (tools_sandbox.cache / "worktree/wezterm-gui/src/vtabs/src/lib.rs").exists()
    assert "0002-second.patch" in result.stdout + result.stderr
    reports = list((tools_sandbox.cache / "runs").glob("*/run.json"))
    assert len(reports) == 1
    report = json.loads(reports[0].read_text())
    assert report["status"] == "failed"
    assert report["version"] == 1
    assert revision in json.dumps(report)
    assert report["configuration"]["WEZ_VTABS_UPSTREAM_URL"] == str(local_upstream[0])
    failed = [command for command in report["commands"] if command["status"] not in (None, 0)]
    assert [command["command"][-1] for command in failed] == [str(patch)] * 2
    assert "--3way" in failed[-1]["command"]
    assert all(command["cwd"] == str(tools_sandbox.cache / "worktree") for command in failed)
    assert all(command["error"] for command in failed)
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
    write_file(tools_sandbox.root, "src/adapter/src/obsolete/nested.rs", "pub fn obsolete() {}\n")
    tools_sandbox.run("--upstream", revision, "prepare")
    checkout = tools_sandbox.cache / "worktree"
    sentinel = write_file(checkout, "keep-checkout.txt", "existing checkout\n")
    module = checkout / "wezterm-gui/src/vtabs/src/storage.rs"
    previous_mtime = module.stat().st_mtime_ns
    test_module = checkout / "wezterm-gui/src/vtabs/tests/storage.rs"
    previous_test_mtime = test_module.stat().st_mtime_ns
    removed = tools_sandbox.root / "src/adapter/src/obsolete/nested.rs"
    removed.unlink()
    removed.parent.rmdir()
    write_file(tools_sandbox.root, "src/adapter/src/new_module.rs", "pub fn added() {}\n")

    adapter_manifest = tools_sandbox.root / "src/adapter/Cargo.toml"
    adapter_manifest.write_text(
        adapter_manifest.read_text()
        + 'fixture-dependency = { version = "1", features = ["extra"] }\n'
    )
    tools_sandbox.run("--upstream", revision, "--offline", "prepare")

    assert sentinel.read_text() == "existing checkout\n"
    assert module.stat().st_mtime_ns == previous_mtime
    assert test_module.stat().st_mtime_ns == previous_test_mtime
    assert not (module.parent / "obsolete").exists()
    assert (module.parent / "new_module.rs").is_file()
    dependencies = tomllib.loads((checkout / "wezterm-gui/Cargo.toml").read_text())["dependencies"]
    assert dependencies["fixture-dependency"] == {"version": "1", "features": ["extra"]}
    for name, directory in (("vtabs-app", "app"), ("vtabs-store", "store")):
        assert dependencies[name] == {
            "path": str(tools_sandbox.root / "src" / directory),
            "default-features": False,
        }


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
        ".git/wez-vtabs.json",
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
    patch = tools_sandbox.root / "wezterm/patches/0002-second.patch"
    patch.write_text(patch.read_text().replace("+second", "+changed second"))

    tools_sandbox.run("--offline", "--upstream", revision, "prepare")

    assert (tools_sandbox.cache / "worktree/patch-target.txt").read_text() == "changed second\n"
    assert (
        tools_sandbox.cache / "worktree/module/module.txt"
    ).read_text() == "cached module source\n"


def test_overlay_files_sync_in_place_without_reapplying_patches(
    tools_sandbox, local_upstream, tmp_path
):
    upstream, revision = local_upstream
    use_entry_patches(
        tools_sandbox.root,
        upstream,
        revision,
        {"wezterm-gui/src/main.rs": "mod owned;\nfn main() {}\n"},
        tmp_path / "patch-source",
    )
    write_file(tools_sandbox.root, "wezterm/overlay/wezterm-gui/src/owned.rs", "pub fn a() {}\n")
    write_file(tools_sandbox.root, "wezterm/overlay/wezterm-gui/src/stale/x.rs", "pub fn x() {}\n")
    tools_sandbox.run("--upstream", revision, "prepare")
    checkout = tools_sandbox.cache / "worktree"
    owned = checkout / "wezterm-gui/src/owned.rs"
    patched = checkout / "wezterm-gui/src/main.rs"
    assert owned.read_text() == "pub fn a() {}\n"
    assert patched.read_text() == "mod owned;\nfn main() {}\n"
    prepared = json.loads((tools_sandbox.cache / "prepared.json").read_text())
    assert sorted(prepared["overlay"]) == ["wezterm-gui/src/owned.rs", "wezterm-gui/src/stale/x.rs"]
    owned_mtime = owned.stat().st_mtime_ns
    patched_mtime = patched.stat().st_mtime_ns
    stale = tools_sandbox.root / "wezterm/overlay/wezterm-gui/src/stale/x.rs"
    stale.unlink()
    stale.parent.rmdir()
    write_file(tools_sandbox.root, "wezterm/overlay/mux/src/added.rs", "pub fn added() {}\n")

    tools_sandbox.run("--upstream", revision, "--offline", "prepare")

    assert owned.stat().st_mtime_ns == owned_mtime
    assert patched.stat().st_mtime_ns == patched_mtime
    assert not (checkout / "wezterm-gui/src/stale").exists()
    assert (checkout / "wezterm-gui/src").is_dir()
    assert (checkout / "mux/src/added.rs").read_text() == "pub fn added() {}\n"

    write_file(tools_sandbox.root, "wezterm/overlay/wezterm-gui/src/owned.rs", "pub fn b() {}\n")
    tools_sandbox.run("--upstream", revision, "--offline", "prepare")

    assert owned.read_text() == "pub fn b() {}\n"
    assert patched.stat().st_mtime_ns == patched_mtime
    prepared = json.loads((tools_sandbox.cache / "prepared.json").read_text())
    assert sorted(prepared["overlay"]) == ["mux/src/added.rs", "wezterm-gui/src/owned.rs"]


def test_overlay_file_refuses_a_path_upstream_later_adds(tools_sandbox, local_upstream):
    upstream, revision = local_upstream
    write_file(tools_sandbox.root, "wezterm/overlay/wezterm-gui/src/later.rs", "pub fn ours() {}\n")
    tools_sandbox.run("--upstream", revision, "prepare")
    write_file(upstream, "wezterm-gui/src/later.rs", "pub fn upstream() {}\n")
    later = commit(upstream, "Upstream adds an overlay path")

    for command in ("prepare",), ("patch", "check"):
        result = tools_sandbox.run("--upstream", later, *command, check=False)
        assert result.returncode != 0
        assert "overlay file tracked upstream: wezterm-gui/src/later.rs" in result.stderr

    checkout = tools_sandbox.cache / "worktree"
    assert (checkout / "wezterm-gui/src/later.rs").read_text() == "pub fn upstream() {}\n"
    assert not (tools_sandbox.cache / "prepared.json").exists()


def test_export_after_prepare_changes_nothing(tools_sandbox, local_upstream, tmp_path):
    upstream, revision = local_upstream
    use_entry_patches(
        tools_sandbox.root,
        upstream,
        revision,
        {
            "wezterm-gui/src/main.rs": "mod owned;\nfn main() {}\n",
            "termwiz/data/wezterm.terminfo": "patched terminfo\n",
        },
        tmp_path / "patch-source",
    )
    write_file(tools_sandbox.root, "wezterm/overlay/wezterm-gui/src/owned.rs", "pub fn a() {}\n")
    tools_sandbox.run("--upstream", revision, "prepare")
    project = tools_sandbox.root / "wezterm"
    before = files(project)
    mtimes = {path: path.stat().st_mtime_ns for path in project.rglob("*")}

    result = tools_sandbox.json("patch", "export")

    assert result == {
        "upstream": revision,
        "patches": 2,
        "overlay": 1,
        "written": [],
        "removed": [],
    }
    assert files(project) == before
    assert {path: path.stat().st_mtime_ns for path in project.rglob("*")} == mtimes


def test_export_writes_worktree_edits_and_prepare_reuses_the_worktree(
    tools_sandbox, local_upstream, tmp_path
):
    upstream, revision = local_upstream
    scratch = use_entry_patches(
        tools_sandbox.root,
        upstream,
        revision,
        {
            "wezterm-gui/src/main.rs": "mod owned;\nfn main() {}\n",
            "termwiz/data/wezterm.terminfo": "patched terminfo\n",
        },
        tmp_path / "patch-source",
    )
    write_file(tools_sandbox.root, "wezterm/overlay/wezterm-gui/src/owned.rs", "pub fn a() {}\n")
    write_file(tools_sandbox.root, "wezterm/overlay/window/src/removed.rs", "pub fn r() {}\n")
    tools_sandbox.run("--upstream", revision, "prepare")
    patches = tools_sandbox.root / "wezterm/patches"
    overlay = tools_sandbox.root / "wezterm/overlay"
    untouched = (patches / "termwiz.patch").read_bytes()
    checkout = tools_sandbox.cache / "worktree"
    edited = "mod owned;\nfn main() {\n    owned::a();\n}\n"
    write_file(checkout, "wezterm-gui/src/main.rs", edited)
    write_file(checkout, "wezterm-gui/src/owned.rs", "pub fn a() {\n}\n")
    write_file(checkout, "mux/src/new.rs", "pub fn new() {}\n")
    (checkout / "window/src/removed.rs").unlink()
    git(checkout, "add", "mux/src/new.rs")
    mtimes = {
        name: (checkout / name).stat().st_mtime_ns
        for name in ("wezterm-gui/src/main.rs", "termwiz/data/wezterm.terminfo")
    }

    result = tools_sandbox.json("patch", "export")

    assert result["written"] == [
        "wezterm/overlay/mux/src/new.rs",
        "wezterm/overlay/wezterm-gui/src/owned.rs",
        "wezterm/patches/wezterm-gui.patch",
    ]
    assert result["removed"] == ["wezterm/overlay/window/src/removed.rs"]
    assert not (overlay / "window").exists()
    assert (patches / "termwiz.patch").read_bytes() == untouched
    write_file(scratch, "wezterm-gui/src/main.rs", edited)
    exported = (patches / "wezterm-gui.patch").read_bytes()
    assert exported == exported_diff(scratch, revision, "wezterm-gui")
    assert b"Cargo.toml" not in exported
    assert b"vtabs" not in exported
    assert (overlay / "wezterm-gui/src/owned.rs").read_text() == "pub fn a() {\n}\n"
    assert (overlay / "mux/src/new.rs").read_text() == "pub fn new() {}\n"

    prepared = tools_sandbox.run("--upstream", revision, "--offline", "--explain", "prepare")

    assert "prepare: reuse patches" in prepared.stderr
    assert (checkout / "wezterm-gui/src/main.rs").read_text() == edited
    assert {
        name: (checkout / name).stat().st_mtime_ns
        for name in ("wezterm-gui/src/main.rs", "termwiz/data/wezterm.terminfo")
    } == mtimes
    assert tools_sandbox.json("patch", "export")["written"] == []


def test_export_migrates_an_ordered_series(tools_sandbox, local_upstream):
    _, revision = local_upstream
    tools_sandbox.run("--upstream", revision, "prepare")

    result = tools_sandbox.json("patch", "export")

    assert result["written"] == ["wezterm/patches/patch-target.txt.patch"]
    assert result["removed"] == [
        "wezterm/patches/0001-first.patch",
        "wezterm/patches/0002-second.patch",
    ]
    patches = tools_sandbox.root / "wezterm/patches"
    assert sorted(path.name for path in patches.iterdir()) == ["patch-target.txt.patch"]
    prepared = tools_sandbox.run("--upstream", revision, "--offline", "--explain", "prepare")
    assert "prepare: reuse patches" in prepared.stderr
    assert (tools_sandbox.cache / "worktree/patch-target.txt").read_text() == "second\n"


def test_export_refuses_without_preparation_or_after_project_changes(tools_sandbox, local_upstream):
    _, revision = local_upstream
    project = tools_sandbox.root / "wezterm"
    missing = tools_sandbox.run("patch", "export", check=False)
    assert missing.returncode != 0
    assert "no prepared worktree" in missing.stderr
    tools_sandbox.run("--upstream", revision, "prepare")
    write_file(tools_sandbox.cache, "worktree/patch-target.txt", "worktree edit\n")

    def assert_refused():
        before = files(project)
        result = tools_sandbox.run("patch", "export", check=False)
        assert result.returncode != 0
        assert "patches or overlay changed since prepare" in result.stderr
        assert files(project) == before

    second = project / "patches/0002-second.patch"
    original = second.read_text()
    second.write_text(original.replace("+second", "+project edit"))
    assert_refused()
    second.write_text(original)
    write_file(project, "overlay/wezterm-gui/src/owned.rs", "pub fn a() {}\n")
    assert_refused()
    assert (tools_sandbox.cache / "worktree/patch-target.txt").read_text() == "worktree edit\n"


def test_context_drift_falls_back_to_a_three_way_merge(tools_sandbox, local_upstream, tmp_path):
    upstream, _ = local_upstream
    write_file(upstream, "wezterm-gui/src/drift.rs", drift_lines())
    base = commit(upstream, "Drift base")
    use_entry_patches(
        tools_sandbox.root,
        upstream,
        base,
        {"wezterm-gui/src/drift.rs": drift_lines(l3="patched 3")},
        tmp_path / "patch-source",
    )
    write_file(upstream, "wezterm-gui/src/drift.rs", drift_lines(l5="upstream 5"))
    drifted = commit(upstream, "Context drift")
    checkout = tools_sandbox.cache / "worktree"

    prepared = tools_sandbox.run("--upstream", drifted, "--explain", "prepare")

    assert "3-way merged wezterm-gui.patch" in prepared.stderr
    merged = drift_lines(l3="patched 3", l5="upstream 5")
    assert (checkout / "wezterm-gui/src/drift.rs").read_text() == merged
    assert git(checkout, "diff", "--cached", "--name-only") == ""
    checked = tools_sandbox.json("--upstream", drifted, "patch", "check")
    assert checked["merged"] == ["wezterm-gui.patch"]
    tools_sandbox.json("patch", "export")
    rebased = tools_sandbox.run("--upstream", drifted, "--explain", "patch", "check")
    assert json.loads(rebased.stdout)["merged"] == []

    write_file(upstream, "wezterm-gui/src/drift.rs", drift_lines(l3="upstream 3"))
    conflicting = commit(upstream, "Conflicting change")
    for command in ("prepare",), ("patch", "check"):
        result = tools_sandbox.run("--upstream", conflicting, *command, check=False)
        assert result.returncode != 0
        assert "wezterm-gui.patch does not apply" in result.stderr
    assert not (tools_sandbox.cache / "prepared.json").exists()
    assert "no prepared worktree" in tools_sandbox.run("patch", "export", check=False).stderr


@pytest.mark.skipif(os.name == "nt", reason="POSIX file transport fixture")
def test_three_way_fallback_fetches_missing_preimages_only_online(
    tools_sandbox, local_upstream, tmp_path
):
    upstream, _ = local_upstream
    git(upstream, "config", "uploadpack.allowFilter", "true")
    git(upstream, "config", "uploadpack.allowAnySHA1InWant", "true")
    tools_sandbox.env["WEZ_VTABS_UPSTREAM_URL"] = upstream.as_uri()
    tools_sandbox.env["GIT_ALLOW_PROTOCOL"] = "file"
    write_file(upstream, "wezterm-gui/src/drift.rs", drift_lines())
    base = commit(upstream, "Drift base")
    write_file(upstream, "wezterm-gui/src/drift.rs", drift_lines(l5="upstream 5"))
    drifted = commit(upstream, "Context drift")
    tools_sandbox.run("--upstream", drifted, "prepare")
    use_entry_patches(
        tools_sandbox.root,
        upstream,
        base,
        {"wezterm-gui/src/drift.rs": drift_lines(l3="patched 3")},
        tmp_path / "patch-source",
    )
    drift = tools_sandbox.cache / "worktree/wezterm-gui/src/drift.rs"

    offline = tools_sandbox.run("--upstream", drifted, "--offline", "prepare", check=False)

    assert offline.returncode != 0
    assert "wezterm-gui.patch does not apply" in offline.stderr
    assert "lacks the necessary blob" in offline.stderr
    assert drift.read_text() == drift_lines(l5="upstream 5")
    assert not (tools_sandbox.cache / "prepared.json").exists()

    tools_sandbox.run("--upstream", drifted, "prepare")

    assert drift.read_text() == drift_lines(l3="patched 3", l5="upstream 5")
