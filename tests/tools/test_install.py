"""Immutable installs, integrity, and managed launchers through the production CLI."""

import json
import os

import pytest
from filelock import FileLock

from tests.tools.support import binary_dir, executable_name, write_manifest

pytestmark = pytest.mark.rust


def state(root, name):
    return json.loads((root / f"{name}.json").read_text())


@pytest.mark.skipif(os.name == "nt", reason="POSIX fixture application executable")
def test_launch_promotes_complete_pending_version_and_forwards_literal_arguments(
    tools_sandbox, bundle_factory
):
    tools_sandbox.run("install", "--bundle", bundle_factory("first"))
    original = binary_dir(tools_sandbox.install / "versions/first") / executable_name("wezterm-gui")
    contents = original.read_bytes()
    tools_sandbox.run("install", "--bundle", bundle_factory("second"), "--stage-only")
    assert state(tools_sandbox.install, "active")["id"] == "first"
    assert state(tools_sandbox.install, "pending")["id"] == "second"
    assert original.read_bytes() == contents

    result = tools_sandbox.run(
        "--offline", "launch", "--", "--literal", "a b $() 'quoted'", check=False
    )

    assert result.returncode == 37
    output = json.loads(result.stdout)
    assert output["id"] == "second"
    assert output["arguments"] == ["--literal", "a b $() 'quoted'"]
    assert output["bundle"] == str(tools_sandbox.install / "versions/second")
    assert output["offline"] == "1"
    assert not (tools_sandbox.install / "update.json").exists()
    assert state(tools_sandbox.install, "active")["id"] == "second"
    assert not (tools_sandbox.install / "pending.json").exists()
    assert (tools_sandbox.install / "versions/first/build.json").is_file()


def test_launch_rejects_incomplete_pending_bundle_and_preserves_active(
    tools_sandbox, bundle_factory
):
    tools_sandbox.run("install", "--bundle", bundle_factory("first"))
    tools_sandbox.run("install", "--bundle", bundle_factory("second"), "--stage-only")
    incomplete = tools_sandbox.install / "versions/second"
    (binary_dir(incomplete) / executable_name("wezterm-gui")).unlink()

    result = tools_sandbox.run("launch", check=False)

    assert result.returncode != 0
    assert state(tools_sandbox.install, "active")["id"] == "first"
    assert state(tools_sandbox.install, "pending")["id"] == "second"


def test_install_rejects_path_traversal_in_bundle_id(tools_sandbox, bundle_factory):
    bundle = bundle_factory("unsafe-id")
    metadata = state(bundle, "build")
    metadata["id"] = "../outside"
    (bundle / "build.json").write_text(json.dumps(metadata))
    write_manifest(bundle)

    result = tools_sandbox.run("install", "--bundle", bundle, check=False)

    assert result.returncode != 0
    assert "invalid bundle ID" in result.stderr
    assert not (tools_sandbox.install / "active.json").exists()


def test_install_rejects_modified_bundle_without_publishing_state(tools_sandbox, bundle_factory):
    bundle = bundle_factory("damaged")
    executable = binary_dir(bundle) / executable_name("wezterm-gui")
    executable.write_bytes(b"different application")

    result = tools_sandbox.run("install", "--bundle", bundle, check=False)

    assert result.returncode != 0
    assert "checksum" in result.stderr
    assert not (tools_sandbox.install / "active.json").exists()


def test_install_rejects_same_id_with_different_contents(tools_sandbox, bundle_factory):
    tools_sandbox.run("install", "--bundle", bundle_factory("collision"))
    replacement = bundle_factory("collision", directory="replacement")
    (replacement / "source/Cargo.toml").write_text("different source\n")
    write_manifest(replacement)

    result = tools_sandbox.run("install", "--bundle", replacement, check=False)

    assert result.returncode != 0
    assert "collision" in result.stderr.lower()
    installed = tools_sandbox.install / "versions/collision/source/Cargo.toml"
    assert installed.read_text() != "different source\n"


def test_install_prunes_old_versions_but_preserves_running_apps(tools_sandbox, bundle_factory):
    for name in ("first", "second"):
        tools_sandbox.run("install", "--bundle", bundle_factory(name))
    versions = tools_sandbox.install / "versions"
    leases = tools_sandbox.install / "leases"
    leases.mkdir(exist_ok=True)
    with FileLock(leases / "first.lock"):
        tools_sandbox.run("install", "--bundle", bundle_factory("third"))
        assert sorted(path.name for path in versions.iterdir()) == ["first", "second", "third"]

    tools_sandbox.run("install", "--bundle", bundle_factory("fourth"))

    assert sorted(path.name for path in versions.iterdir()) == ["fourth", "third"]
    assert state(tools_sandbox.install, "active")["id"] == "fourth"
    assert state(tools_sandbox.install, "previous")["id"] == "third"
    assert not (leases / "first.lock").exists()
