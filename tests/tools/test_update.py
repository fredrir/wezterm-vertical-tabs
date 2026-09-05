"""Update ownership and source provenance using local Git transports only."""

import hashlib
import io
import json
import os
import tarfile
from pathlib import Path

import pytest

from tests.tools.conftest import commit, git, write_file
from tests.tools.support import write_manifest

pytestmark = pytest.mark.rust


def use_installed_source(tools_sandbox, bundle_factory, branch="native", remote=None):
    bundle = bundle_factory("installed-source")
    metadata = json.loads((bundle / "build.json").read_text())
    metadata["project_source"] = {
        "remote": remote or tools_sandbox.env["WEZ_VTABS_PROJECT_URL"],
        "branch": branch,
        "revision": "a" * 40,
    }
    (bundle / "build.json").write_text(json.dumps(metadata))
    write_manifest(bundle)
    tools_sandbox.root = bundle / "source"


def test_update_refuses_an_unowned_project_checkout(tools_sandbox, bundle_factory, failing_cargo):
    sentinel = write_file(tools_sandbox.cache, "project/uncommitted.rs", "user edits\n")
    use_installed_source(tools_sandbox, bundle_factory)

    result = tools_sandbox.run("update", "--stage-only", check=False)

    assert result.returncode != 0
    assert "unowned" in result.stderr
    assert sentinel.read_text() == "user edits\n"
    assert not failing_cargo.exists()


def test_installed_update_preserves_recorded_branch_and_refreshes_legacy_single_branch_cache(
    tools_sandbox, local_upstream, bundle_factory, failing_cargo
):
    repository = tools_sandbox.root
    git(repository, "branch", "main")
    git(repository, "switch", "-c", "release/native")
    expected = commit(repository, "Native implementation on release branch")
    checkout = tools_sandbox.cache / "project"
    checkout.parent.mkdir(parents=True)
    git(repository, "clone", "--single-branch", "--branch", "main", str(repository), str(checkout))
    write_file(
        checkout,
        ".git/wez-vtabs-native.json",
        json.dumps({"path": str(checkout.resolve()), "remote": str(repository), "capability": 1}),
    )
    use_installed_source(tools_sandbox, bundle_factory, branch="release/native")

    result = tools_sandbox.run("update", "--stage-only", check=False)

    assert result.returncode != 0
    assert "fixture compiler failure" in result.stderr
    assert git(checkout, "rev-parse", "HEAD") == expected
    assert (checkout / "native/patches/0001-first.patch").is_file()
    commands = [json.loads(line) for line in failing_cargo.read_text().splitlines()]
    assert commands
    assert any("tools/Cargo.toml" in " ".join(arguments) for arguments in commands)
    state = json.loads((tools_sandbox.install / "update.json").read_text())
    assert state["status"] == "failed"
    assert (tools_sandbox.root.parent / "build.json").is_file()


@pytest.mark.parametrize(
    "branch",
    [
        "--upload-pack=bad",
        "../main",
        "native^{commit}",
        "a..b",
        "a.lock",
        "a//b",
        "a/.b",
        "a/",
        "a.",
        "a b",
        "a\\b",
    ],
)
def test_update_rejects_unsafe_recorded_branch_before_fetching(
    tools_sandbox, bundle_factory, branch
):
    use_installed_source(tools_sandbox, bundle_factory, branch=branch)

    result = tools_sandbox.run("update", check=False)

    assert result.returncode != 0
    assert "invalid project update branch" in result.stderr
    assert not (tools_sandbox.cache / "project").exists()


def test_update_rejects_an_unexpected_recorded_remote(tools_sandbox, bundle_factory):
    use_installed_source(tools_sandbox, bundle_factory, remote="https://example.invalid/other.git")

    result = tools_sandbox.run("update", check=False)

    assert result.returncode != 0
    assert "unexpected recorded project source remote" in result.stderr
    assert not (tools_sandbox.cache / "project").exists()


def test_verified_prebuilt_update_installs_offline_without_compilation(
    tools_sandbox, bundle_factory
):
    bundle = bundle_factory("prebuilt")
    metadata = json.loads((bundle / "build.json").read_text())
    metadata.update(
        source_digest="c" * 64,
        upstream="a" * 40,
        project_source={
            "remote": tools_sandbox.env["WEZ_VTABS_PROJECT_URL"],
            "branch": "native",
            "revision": git(tools_sandbox.root, "rev-parse", "HEAD"),
        },
    )
    (bundle / "build.json").write_text(json.dumps(metadata))
    write_manifest(bundle)
    archive = Path(tools_sandbox.json("package", "--bundle", bundle)["archive"])
    manifest = archive.with_name(archive.name + ".manifest.json")
    tools_sandbox.root = bundle / "source"
    tools_sandbox.env["PATH"] = ""

    result = tools_sandbox.run("--offline", "update", "--manifest", manifest, "--stage-only")

    assert json.loads(result.stdout)["status"] == "ready"
    assert json.loads((tools_sandbox.install / "pending.json").read_text())["id"] == "prebuilt"
    assert (tools_sandbox.install / "versions/prebuilt/checksums.json").is_file()
    assert not (tools_sandbox.cache / "upstream").exists()


@pytest.mark.parametrize("kind", ["parent-traversal", "symlink-parent-chain"])
def test_prebuilt_update_rejects_archive_paths_that_escape_extraction(
    tools_sandbox, rust_host, tmp_path, kind
):
    if kind == "symlink-parent-chain" and os.name == "nt":
        pytest.skip("POSIX symlink archive fixture")
    archive_path = tmp_path / "malformed.tar.gz"
    with tarfile.open(archive_path, "w:gz") as archive:
        if kind == "parent-traversal":
            entry = tarfile.TarInfo("../escaped-file")
            entry.size = 7
            archive.addfile(entry, io.BytesIO(b"outside"))
        else:
            for name, target in (
                ("bad/x", "."),
                ("bad/x/x/x/escape", "../../../.."),
                ("bad/escape/escaped-link", "."),
            ):
                entry = tarfile.TarInfo(name)
                entry.type = tarfile.SYMTYPE
                entry.linkname = target
                archive.addfile(entry)
    manifest = tmp_path / "malformed.manifest.json"
    manifest.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "id": "malformed",
                "target": rust_host,
                "source_digest": "c" * 64,
                "upstream": "a" * 40,
                "project_source": {
                    "remote": tools_sandbox.env["WEZ_VTABS_PROJECT_URL"],
                    "branch": "native",
                    "revision": git(tools_sandbox.root, "rev-parse", "HEAD"),
                },
                "archive": archive_path.name,
                "sha256": hashlib.sha256(archive_path.read_bytes()).hexdigest(),
                "size": archive_path.stat().st_size,
            }
        )
    )

    result = tools_sandbox.run("--offline", "update", "--manifest", manifest, check=False)

    assert result.returncode != 0
    assert "unsafe" in result.stderr or "symlink" in result.stderr
    assert not os.path.lexists(tmp_path / "escaped-link")
    assert not (tools_sandbox.install / "active.json").exists()
