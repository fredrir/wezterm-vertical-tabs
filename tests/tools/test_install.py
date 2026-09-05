"""Immutable installs, integrity, and managed launchers through the production CLI."""

import json
import os
import subprocess
import time

import pytest

from tests.tools.support import binary_dir, executable_name, write_manifest

pytestmark = pytest.mark.rust


def state(root, name):
    return json.loads((root / f"{name}.json").read_text())


def suppress_daily_update(root):
    (root / "update.json").write_text(json.dumps({"last_attempt": int(time.time())}))


def test_install_stages_a_new_version_without_changing_active_files(tools_sandbox, bundle_factory):
    first = bundle_factory("first")
    tools_sandbox.run("install", "--bundle", first)
    installed = tools_sandbox.install / "versions/first"
    gui = binary_dir(installed) / executable_name("wezterm-gui")
    original = gui.read_bytes()

    tools_sandbox.run("install", "--bundle", bundle_factory("second"), "--stage-only")

    assert state(tools_sandbox.install, "active")["id"] == "first"
    assert state(tools_sandbox.install, "pending")["id"] == "second"
    assert gui.read_bytes() == original
    assert (tools_sandbox.install / "versions/second/build.json").is_file()


@pytest.mark.skipif(os.name == "nt", reason="POSIX fixture application executable")
def test_launch_promotes_complete_pending_version_and_forwards_literal_arguments(
    tools_sandbox, bundle_factory
):
    tools_sandbox.run("install", "--bundle", bundle_factory("first"))
    tools_sandbox.run("install", "--bundle", bundle_factory("second"), "--stage-only")
    suppress_daily_update(tools_sandbox.install)

    result = tools_sandbox.run("launch", "--", "--literal", "a b $() 'quoted'", check=False)

    assert result.returncode == 37
    output = json.loads(result.stdout)
    assert output["id"] == "second"
    assert output["arguments"] == ["--literal", "a b $() 'quoted'"]
    assert output["bundle"] == str(tools_sandbox.install / "versions/second")
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


@pytest.mark.parametrize("identifier", ["../outside", "a/b", "/tmp", "", "a\\b"])
def test_install_rejects_unsafe_identifiers(tools_sandbox, bundle_factory, identifier):
    bundle = bundle_factory("unsafe-id")
    metadata = state(bundle, "build")
    metadata["id"] = identifier
    (bundle / "build.json").write_text(json.dumps(metadata))
    write_manifest(bundle)

    result = tools_sandbox.run("install", "--bundle", bundle, check=False)

    assert result.returncode != 0
    assert "invalid bundle ID" in result.stderr
    assert not (tools_sandbox.install / "active.json").exists()


@pytest.mark.parametrize("damage", ["deleted", "modified", "unlisted", "permissions"])
def test_install_rejects_damaged_bundle_without_publishing_state(
    tools_sandbox, bundle_factory, damage
):
    if damage == "permissions" and os.name == "nt":
        pytest.skip("Unix executable mode is not a Windows bundle property")
    bundle = bundle_factory("damaged")
    executable = binary_dir(bundle) / executable_name("wezterm-gui")
    if damage == "deleted":
        executable.unlink()
    elif damage == "modified":
        executable.write_bytes(b"different application")
    elif damage == "permissions":
        executable.chmod(0o644)
    else:
        (bundle / "unlisted-file").write_text("not in the checksum manifest")

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


def test_install_rejects_incompatible_target(tools_sandbox, bundle_factory):
    bundle = bundle_factory("wrong-target")
    metadata = state(bundle, "build")
    metadata["target"] = "incompatible-target"
    (bundle / "build.json").write_text(json.dumps(metadata))
    write_manifest(bundle)

    result = tools_sandbox.run("install", "--bundle", bundle, check=False)

    assert result.returncode != 0
    assert "target mismatch" in result.stderr
    assert not (tools_sandbox.install / "active.json").exists()


@pytest.mark.skipif(os.name == "nt", reason="POSIX launcher execution")
def test_custom_install_launcher_remembers_root_without_environment(
    tools_sandbox, bundle_factory, tmp_path
):
    tools_sandbox.install = tmp_path / "install with 'quotes' and $symbols"
    tools_sandbox.run("install", "--bundle", bundle_factory("custom"))
    suppress_daily_update(tools_sandbox.install)
    environment = {**tools_sandbox.env, "XDG_DATA_HOME": str(tmp_path / "unrelated default")}
    environment.pop("WEZ_VTABS_INSTALL", None)
    result = subprocess.run(
        [str(tools_sandbox.install / "wez-vtabs"), "--literal", "a b $()"],
        cwd=tmp_path,
        env=environment,
        text=True,
        capture_output=True,
        timeout=20,
        check=False,
    )

    assert result.returncode == 37, result.stderr
    output = json.loads(result.stdout)
    assert output["arguments"] == ["--literal", "a b $()"]
    assert output["bundle"] == str(tools_sandbox.install / "versions/custom")
    assert not (tools_sandbox.install / "native.py").exists()


def test_daily_update_is_skipped_after_a_recent_failed_attempt(tools_sandbox):
    tools_sandbox.install.mkdir()
    original = {"last_attempt": int(time.time()), "status": "failed", "error": "previous failure"}
    (tools_sandbox.install / "update.json").write_text(json.dumps(original))
    tools_sandbox.env["WEZ_VTABS_UPSTREAM_URL"] = str(tools_sandbox.root / "missing-upstream")

    tools_sandbox.run("update", "--daily", "--stage-only")

    assert state(tools_sandbox.install, "update") == original
    assert not (tools_sandbox.cache / "upstream").exists()


@pytest.mark.skipif(os.name == "nt", reason="POSIX fixture application executable")
@pytest.mark.parametrize("direct", [False, True])
def test_offline_launch_suppresses_both_updaters_and_reaches_the_application(
    tools_sandbox, bundle_factory, direct
):
    tools_sandbox.run("install", "--bundle", bundle_factory("offline"))
    if direct:
        tools_sandbox.binary = binary_dir(
            tools_sandbox.install / "versions/offline"
        ) / executable_name("wez-vtabs")

    result = tools_sandbox.run("--offline", "launch", check=False)

    assert result.returncode == 37, result.stderr
    assert json.loads(result.stdout)["offline"] == "1"
    assert not (tools_sandbox.install / "update.log").exists()
    assert not (tools_sandbox.install / "update.json").exists()
    assert not (tools_sandbox.cache / "project").exists()
