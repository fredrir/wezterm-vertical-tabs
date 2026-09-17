"""Container-based build orchestration for target operating systems."""

from __future__ import annotations

import json
import stat
from pathlib import Path

import pytest

pytestmark = pytest.mark.rust


def test_container_build_explain_ubuntu_26(tools_sandbox):
    data = tools_sandbox.json("build", "--ubuntu", "--26", "--explain")
    assert data["target_os"] == "ubuntu"
    assert data["version"] == "26.04"
    assert data["base_image"] == "docker.io/library/ubuntu:26.04"
    assert data["builder_image"] == "localhost/wez-vtabs-builder:ubuntu-26.04"
    assert data["output_dir"].endswith("dist/ubuntu-26.04")
    assert set(data["binaries"]) == {
        "wezterm-gui",
        "wezterm",
        "wezterm-mux-server",
        "strip-ansi-escapes",
        "wez-vtabs-store",
        "wez-vtabs",
    }
    assert not data["skip_tests"]


def test_container_build_explain_debian(tools_sandbox):
    data = tools_sandbox.json("build", "--debian", "--explain")
    assert data["target_os"] == "debian"
    assert data["version"] == "latest"
    assert data["base_image"] == "docker.io/library/debian:latest"
    assert data["builder_image"] == "localhost/wez-vtabs-builder:debian-latest"
    assert data["output_dir"].endswith("dist/debian-latest")


def test_container_build_explain_debian_versions(tools_sandbox):
    d12 = tools_sandbox.json("build", "--debian", "--12", "--explain")
    assert d12["target_os"] == "debian"
    assert d12["version"] == "12"
    assert d12["base_image"] == "docker.io/library/debian:12"

    d13 = tools_sandbox.json("build", "--debian", "--13", "--explain")
    assert d13["target_os"] == "debian"
    assert d13["version"] == "13"
    assert d13["base_image"] == "docker.io/library/debian:13"


def test_container_build_explain_fedora_and_arch_and_alpine(tools_sandbox):
    fedora = tools_sandbox.json("build", "--fedora", "--41", "--explain")
    assert fedora["target_os"] == "fedora"
    assert fedora["version"] == "41"
    assert fedora["base_image"] == "docker.io/library/fedora:41"

    arch = tools_sandbox.json("build", "--arch", "--explain")
    assert arch["target_os"] == "arch"
    assert arch["version"] == "base"
    assert arch["base_image"] == "docker.io/library/archlinux:base"

    alpine = tools_sandbox.json("build", "--alpine", "--explain")
    assert alpine["target_os"] == "alpine"
    assert alpine["version"] == "latest"
    assert alpine["base_image"] == "docker.io/library/alpine:latest"


def test_container_build_explain_custom_options(tools_sandbox, tmp_path):
    custom_out = tmp_path / "custom-output"
    data = tools_sandbox.json(
        "build",
        "--os",
        "centos",
        "--os-version",
        "9",
        "--output-dir",
        str(custom_out),
        "--skip-tests",
        "--clean-builder",
        "--explain",
    )
    assert data["target_os"] == "centos"
    assert data["version"] == "9"
    assert data["output_dir"] == str(custom_out)
    assert data["skip_tests"] is True
    assert data["clean_builder"] is True


def test_container_build_execution_with_mock_runtime(tools_sandbox, local_upstream, tmp_path):
    _, revision = local_upstream

    # Create a mock container runtime script (mocking podman/docker)
    mock_runtime = tmp_path / "mock-runtime.py"
    args_log = tmp_path / "mock-runtime-args.json"
    mock_runtime.write_text(
        f"""#!/usr/bin/env python3
import sys
import json
from pathlib import Path

args = sys.argv[1:]

if not args:
    sys.exit(0)

if args[0] == "info":
    sys.exit(0)

if args[:2] == ["image", "inspect"]:
    # Pretend builder image already exists
    sys.exit(0)

if args[0] == "run":
    Path({repr(str(args_log))}).write_text(json.dumps(args))
    # Find CARGO_TARGET_DIR from arguments
    target_dir = None
    for i, arg in enumerate(args):
        if arg == "-e" and i + 1 < len(args) and args[i + 1].startswith("CARGO_TARGET_DIR="):
            target_dir = Path(args[i + 1].split("=", 1)[1])
            break

    if target_dir:
        release_dir = target_dir / "release"
        release_dir.mkdir(parents=True, exist_ok=True)
        binaries = [
            "wezterm-gui",
            "wezterm",
            "wezterm-mux-server",
            "strip-ansi-escapes",
            "wez-vtabs-store",
            "wez-vtabs",
        ]
        for b in binaries:
            p = release_dir / b
            p.write_bytes(f"mock-binary-{{b}}".encode())
            p.chmod(0o755)

    sys.exit(0)

sys.exit(0)
"""
    )
    mock_runtime.chmod(mock_runtime.stat().st_mode | stat.S_IEXEC)

    # Run container build targeting Ubuntu 26.04
    result = tools_sandbox.json(
        "--upstream",
        revision,
        "build",
        "--ubuntu",
        "--26",
        "--container-runtime",
        str(mock_runtime),
    )

    assert result["status"] == "completed"
    assert result["target_os"] == "ubuntu"
    assert result["version"] == "26.04"
    assert result["runtime"] == str(mock_runtime)

    output_dir = Path(result["output_dir"])
    assert output_dir.exists()

    expected_binaries = [
        "wezterm-gui",
        "wezterm",
        "wezterm-mux-server",
        "strip-ansi-escapes",
        "wez-vtabs-store",
        "wez-vtabs",
    ]
    for b in expected_binaries:
        bin_path = output_dir / b
        assert bin_path.is_file()
        assert bin_path.read_bytes() == f"mock-binary-{b}".encode()
        assert b in result["artifacts"]
        assert b in result["hashes"]

    # Verify checksums.json
    checksums_file = output_dir / "checksums.json"
    assert checksums_file.is_file()
    checksums = json.loads(checksums_file.read_text())
    assert checksums["target_os"] == "ubuntu"
    assert checksums["version"] == "26.04"
    assert set(checksums["hashes"].keys()) == set(expected_binaries)

    # Verify archive .tar.gz was created
    archive_path = Path(result["archive"])
    assert archive_path.is_file()
    assert archive_path.stat().st_size > 0


def test_container_build_podman_keep_id(tools_sandbox, local_upstream, tmp_path):
    _, revision = local_upstream

    args_log = tmp_path / "podman-args.json"
    mock_podman = tmp_path / "mock-podman"
    mock_podman.write_text(
        f"""#!/usr/bin/env python3
import sys
import json
from pathlib import Path

args = sys.argv[1:]
if not args:
    sys.exit(0)

if args[0] == "info":
    sys.exit(0)

if args[:2] == ["image", "inspect"]:
    sys.exit(0)

if args[0] == "run":
    Path({repr(str(args_log))}).write_text(json.dumps(args))
    target_dir = None
    for i, arg in enumerate(args):
        if arg == "-e" and i + 1 < len(args) and args[i + 1].startswith("CARGO_TARGET_DIR="):
            target_dir = Path(args[i + 1].split("=", 1)[1])
            break

    if target_dir:
        release_dir = target_dir / "release"
        release_dir.mkdir(parents=True, exist_ok=True)
        binaries = [
            "wezterm-gui",
            "wezterm",
            "wezterm-mux-server",
            "strip-ansi-escapes",
            "wez-vtabs-store",
            "wez-vtabs",
        ]
        for b in binaries:
            p = release_dir / b
            p.write_bytes(f"mock-binary-{{b}}".encode())
            p.chmod(0o755)

    sys.exit(0)

sys.exit(0)
"""
    )
    mock_podman.chmod(mock_podman.stat().st_mode | stat.S_IEXEC)

    result = tools_sandbox.json(
        "--upstream",
        revision,
        "build",
        "--ubuntu",
        "--26",
        "--container-runtime",
        str(mock_podman),
    )

    assert result["status"] == "completed"
    assert args_log.exists()
    logged_args = json.loads(args_log.read_text())
    assert "--userns=keep-id" in logged_args
    assert "--user" in logged_args
