"""Verified bundles recover interrupted archive publication without rebuilding."""

import hashlib
import json
import os
import sys
from pathlib import Path

import pytest

from tests.tools.conftest import write_file
from tests.tools.support import binary_dir, executable_name, write_manifest

pytestmark = pytest.mark.rust


def archive_path(bundle):
    return bundle.with_name(
        bundle.name + (".zip" if os.name == "nt" or sys.platform == "darwin" else ".tar.gz")
    )


def test_package_existing_bundle_recovers_missing_archive_and_manifest(
    tools_sandbox, bundle_factory
):
    bundle = bundle_factory("archive-recovery")
    original = (bundle / "checksums.json").read_bytes()
    tools_sandbox.run("package", "--bundle", bundle)
    archive = archive_path(bundle)
    sidecar = archive.with_name(archive.name + ".manifest.json")
    assert archive.is_file()
    manifest = json.loads(sidecar.read_text())
    assert manifest["id"] == "archive-recovery"
    assert manifest["sha256"] == hashlib.sha256(archive.read_bytes()).hexdigest()

    archive.unlink()
    tools_sandbox.run("package", "--bundle", bundle)
    assert archive.is_file()
    sidecar.unlink()
    tools_sandbox.run("package", "--bundle", bundle)
    assert sidecar.is_file()
    assert (bundle / "checksums.json").read_bytes() == original
    assert not list(bundle.parent.glob(".archive-*"))
    assert not (tools_sandbox.cache / "upstream").exists()


def test_package_rejects_a_corrupted_existing_archive(tools_sandbox, bundle_factory):
    bundle = bundle_factory("corrupted-archive")
    tools_sandbox.run("package", "--bundle", bundle)
    archive_path(bundle).write_bytes(b"interrupted or modified archive")

    result = tools_sandbox.run("package", "--bundle", bundle, check=False)

    assert result.returncode != 0
    assert "archive checksum mismatch" in result.stderr


@pytest.mark.skipif(os.name == "nt", reason="POSIX symlink fixture")
def test_verify_rejects_a_symlink_escaping_the_bundle(tools_sandbox, bundle_factory, tmp_path):
    bundle = bundle_factory("escaping-link")
    target = tmp_path / "outside.txt"
    target.write_text("external file\n")
    (bundle / "outside-link").symlink_to("../outside.txt")
    write_manifest(bundle)

    result = tools_sandbox.run("verify", "--bundle", bundle, check=False)

    assert result.returncode != 0
    assert "symlink escapes root" in result.stderr
    assert target.read_text() == "external file\n"


def test_package_assembles_platform_runtime_assets_tool_and_reproducible_source(
    tools_sandbox, local_upstream, recording_cargo
):
    _, revision = local_upstream
    fixtures = {
        "pyproject.toml": '[project]\nname = "fixture-tests"\n',
        "uv.lock": "version = 1\n",
        "tests/containers/ssh/Dockerfile": "FROM fixture\n",
        "tests/lua/configuration.lua": "return {}\n",
        "tests/tools/test_example.py": "def test_example(): pass\n",
    }
    for name, contents in fixtures.items():
        write_file(tools_sandbox.root, name, contents)
    write_file(tools_sandbox.root, "tests/__pycache__/generated.pyc", "generated\n")

    result = tools_sandbox.json("--upstream", revision, "package")

    bundle = Path(result["bundle"])
    binaries = binary_dir(bundle)
    for name in (
        "wezterm-gui",
        "wezterm",
        "wezterm-mux-server",
        "strip-ansi-escapes",
        "wez-vtabs-store",
        "wez-vtabs",
    ):
        assert (binaries / executable_name(name)).is_file()
    resources = (
        bundle / "WezTerm.app/Contents/Resources" if sys.platform == "darwin" else bundle / "share"
    )
    marker_dir = resources if sys.platform == "darwin" else binaries
    marker = json.loads((marker_dir / "native-bundle.json").read_text())
    assert (marker_dir / marker["root"]).resolve() == bundle.resolve()
    assert bundle / marker["tool"] == binaries / executable_name("wez-vtabs")
    assert (resources / "licenses/WezTerm-LICENSE.md").is_file()
    assert (resources / "plugin/init.lua").is_file()
    for name, contents in fixtures.items():
        assert (bundle / "source" / name).read_text() == contents
    assert not (bundle / "source/tests/__pycache__").exists()
    if sys.platform == "darwin":
        assert (resources / "wezterm.sh").is_file()
        assert (resources / "shell-completion/bash").is_file()
    elif sys.platform.startswith("linux"):
        assert (bundle / "etc/profile.d/wezterm.sh").is_file()
        assert (bundle / "share/bash-completion/completions/wezterm").is_file()
        assert (bundle / "share/zsh/site-functions/_wezterm").is_file()
    tools_sandbox.run("verify", "--bundle", bundle)
    assert archive_path(bundle).is_file()
