"""Linux bundles built by the tool itself inside a distribution container."""

from __future__ import annotations

import json
import os
import platform
import stat
from pathlib import Path

import pytest

pytestmark = pytest.mark.rust

EMULATED = platform.machine().lower() not in ("x86_64", "amd64")


@pytest.mark.parametrize(
    ("value", "base_image", "tag"),
    [
        ("ubuntu-26.04", "docker.io/library/ubuntu:26.04", "ubuntu-26.04-x86_64"),
        ("ubuntu", "docker.io/library/ubuntu:26.04", "ubuntu-26.04-x86_64"),
        ("arch", "docker.io/library/archlinux:base", "arch-base-x86_64"),
        ("debian-13", "docker.io/library/debian:13", "debian-13-x86_64"),
        ("fedora", "docker.io/library/fedora:latest", "fedora-latest-x86_64"),
    ],
)
def test_platform_selects_base_and_builder_images(tools_sandbox, value, base_image, tag):
    data = tools_sandbox.json("--explain", "build", "--platform", value)

    assert data["base_image"] == base_image
    assert data["builder_image"] == f"localhost/wez-vtabs-builder:{tag}"
    assert data["role"] == "desktop"
    assert data["emulated"] is EMULATED
    assert data["output"].endswith("dist")
    assert data["command"][:2] == ["cargo", "xtask"]
    assert data["command"][-6:-2] == ["package", "--role", "desktop", "--no-archive"]


def test_custom_image_and_output(tools_sandbox, tmp_path):
    output = tmp_path / "custom-output"
    data = tools_sandbox.json(
        "--explain",
        "build",
        "--platform",
        "ubuntu-24.04",
        "--image",
        "registry.example/ubuntu:24.04",
        "--output",
        output,
    )

    assert data["base_image"] == "registry.example/ubuntu:24.04"
    assert data["platform"] == "ubuntu-24.04"
    assert data["output"] == str(output)


def test_invalid_platform_is_rejected(tools_sandbox):
    result = tools_sandbox.run("build", "--platform", "Ubuntu 26", "--explain", check=False)

    assert result.returncode != 0
    assert "invalid platform" in result.stderr


@pytest.mark.skipif(os.name == "nt", reason="POSIX fixture command dispatch")
def test_container_runs_the_tool_with_an_isolated_cache(tools_sandbox, tmp_path):
    log = tmp_path / "runtime-args.json"
    runtime = tmp_path / "podman"
    runtime.write_text(
        f"""#!/usr/bin/env python3
import json, sys
from pathlib import Path

args = sys.argv[1:]
if args[:1] == ["info"] or args[:2] == ["image", "inspect"]:
    sys.exit(0)
if args[:1] == ["run"]:
    Path({str(log)!r}).write_text(json.dumps(args))
    output = Path(args[args.index("--output") + 1])
    bundle = output / "wez-vtabs-fixture"
    bundle.mkdir(parents=True)
    print(json.dumps({{"bundle": str(bundle)}}, indent=2))
sys.exit(0)
"""
    )
    runtime.chmod(runtime.stat().st_mode | stat.S_IEXEC)
    output = tmp_path / "out"

    result = tools_sandbox.json(
        "--upstream",
        "0" * 40,
        "build",
        "--platform",
        "ubuntu-26.04",
        "--role",
        "mux",
        "--container-runtime",
        runtime,
        "--output",
        output,
    )

    assert Path(result["bundle"]) == output.resolve() / "wez-vtabs-fixture"
    args = json.loads(log.read_text())
    assert ("--platform" in args) is EMULATED
    assert "--userns=keep-id" in args
    assert args[args.index("--user") + 1].count(":") == 1
    cache = tools_sandbox.cache.resolve() / "platforms/ubuntu-26.04"
    assert f"CARGO_TARGET_DIR={cache / 'xtask-target'}" in args
    tool = args[args.index("localhost/wez-vtabs-builder:ubuntu-26.04-x86_64") + 1 :]
    assert tool[:2] == ["cargo", "xtask"]
    assert tool[tool.index("--cache") + 1] == str(cache)
    assert tool[tool.index("--upstream") + 1] == "0" * 40
    assert tool[tool.index("package") :] == [
        "package",
        "--role",
        "mux",
        "--no-archive",
        "--output",
        str(output.resolve()),
    ]
