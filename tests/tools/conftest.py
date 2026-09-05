from __future__ import annotations

import json
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

import pytest

from tests.tools.support import create_bundle


def git(repository: Path, *arguments: str) -> str:
    result = subprocess.run(
        ["git", *arguments],
        cwd=repository,
        capture_output=True,
        text=True,
        timeout=20,
        check=True,
    )
    return result.stdout.strip()


def commit(repository: Path, message: str = "Fixture source") -> str:
    git(repository, "add", ".")
    git(
        repository,
        "-c",
        "user.name=Tooling test",
        "-c",
        "user.email=tooling@example.invalid",
        "commit",
        "--quiet",
        "-m",
        message,
    )
    return git(repository, "rev-parse", "HEAD")


def write_file(root: Path, name: str, contents: str) -> Path:
    path = root / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(contents, encoding="utf-8")
    return path


@dataclass
class ToolSandbox:
    binary: Path
    root: Path
    cache: Path
    install: Path
    env: dict[str, str]

    def run(self, *arguments: object, check: bool = True) -> subprocess.CompletedProcess[str]:
        result = subprocess.run(
            [
                str(self.binary),
                "--project-root",
                str(self.root),
                "--cache",
                str(self.cache),
                "--install-root",
                str(self.install),
                *map(str, arguments),
            ],
            cwd=self.root,
            env=self.env,
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        if check:
            assert result.returncode == 0, (
                f"Tool command failed: {result.args!r}\n{result.stdout}\n{result.stderr}"
            )
        return result

    def json(self, *arguments: object) -> dict:
        return json.loads(self.run("--json", *arguments).stdout)


@pytest.fixture
def tools_sandbox(tools_binary: Path, isolated_env: dict[str, str], tmp_path: Path) -> ToolSandbox:
    root = tmp_path / "project"
    root.mkdir()
    write_file(root, "Cargo.toml", '[workspace]\nmembers = []\nresolver = "2"\n')
    write_file(root, "Cargo.lock", "version = 4\n")
    write_file(root, "tools/Cargo.toml", '[package]\nname = "vtabs-tools"\nversion = "0.1.0"\n')
    for name in ("vtabs-app", "vtabs-store"):
        write_file(
            root,
            f"crates/{name}/Cargo.toml",
            f'[package]\nname = "{name}"\nversion = "0.1.0"\n',
        )
        write_file(root, f"crates/{name}/src/lib.rs", "pub fn fixture() {}\n")
    write_file(root, "native/adapter/mod.rs", "mod storage;\n")
    write_file(root, "native/adapter/storage.rs", "pub fn fixture() {}\n")
    write_file(root, "plugin/init.lua", "return {}\n")
    write_file(root, "README.md", "Fixture project\n")
    git(root, "init", "--quiet", "--initial-branch=native")
    commit(root)
    return ToolSandbox(
        binary=tools_binary,
        root=root,
        cache=tmp_path / "cache",
        install=tmp_path / "install",
        env={**isolated_env, "WEZ_VTABS_PROJECT_URL": str(root)},
    )


@pytest.fixture
def local_upstream(tools_sandbox: ToolSandbox, tmp_path: Path) -> tuple[Path, str]:
    root = tmp_path / "upstream"
    root.mkdir()
    write_file(root, "Cargo.toml", '[workspace]\nmembers = ["wezterm-gui"]\nresolver = "2"\n')
    write_file(
        root,
        "wezterm-gui/Cargo.toml",
        '[package]\nname = "wezterm-gui"\nversion = "0.1.0"\n[dependencies]\n'
        "termwiz.workspace = true\n",
    )
    write_file(root, "wezterm-gui/src/main.rs", "fn main() {}\n")
    write_file(root, "patch-target.txt", "initial\n")
    for name in (
        "LICENSE.md",
        "assets/fonts/LICENSE-fixture",
        "assets/shell-integration/wezterm.sh",
        "assets/shell-completion/bash",
        "assets/shell-completion/zsh",
        "assets/icon/terminal.png",
        "assets/wezterm.desktop",
        "assets/wezterm.appdata.xml",
        "assets/macos/WezTerm.app/Contents/Info.plist",
        "assets/macos/WezTerm.app/Contents/Resources/terminal.icns",
        "termwiz/data/wezterm.terminfo",
    ):
        write_file(root, name, f"Fixture asset: {name}\n")
    git(root, "init", "--quiet", "--initial-branch=main")
    revision = commit(root)
    for name, before, after in (
        ("0001-first.patch", "initial", "first"),
        ("0002-second.patch", "first", "second"),
    ):
        write_file(
            tools_sandbox.root,
            f"native/patches/{name}",
            "diff --git a/patch-target.txt b/patch-target.txt\n"
            "--- a/patch-target.txt\n+++ b/patch-target.txt\n@@ -1 +1 @@\n"
            f"-{before}\n+{after}\n",
        )
    tools_sandbox.env["WEZ_VTABS_UPSTREAM_URL"] = str(root)
    return root, revision


@pytest.fixture
def bundle_factory(tools_binary: Path, rust_host: str, tmp_path: Path):
    def create(name: str, directory: str | None = None) -> Path:
        return create_bundle(
            tmp_path / (directory or f"bundle-{name}"), name, tools_binary, rust_host
        )

    return create


@pytest.fixture
def failing_cargo(tools_sandbox: ToolSandbox, tmp_path: Path) -> Path:
    if os.name == "nt":
        pytest.skip("POSIX fixture command dispatch")
    directory = tmp_path / "fixture-commands"
    directory.mkdir()
    log = directory / "cargo.jsonl"
    command = directory / "cargo"
    command.write_text(
        f"#!{sys.executable}\n"
        "import json, sys\n"
        f"with open({str(log)!r}, 'a', encoding='utf-8') as stream:\n"
        "    stream.write(json.dumps(sys.argv[1:]) + '\\n')\n"
        "print('fixture compiler failure', file=sys.stderr)\n"
        "raise SystemExit(61)\n",
        encoding="utf-8",
    )
    command.chmod(0o755)
    tools_sandbox.env["PATH"] = str(directory) + os.pathsep + tools_sandbox.env["PATH"]
    return log


@pytest.fixture
def recording_cargo(tools_sandbox: ToolSandbox, tmp_path: Path) -> Path:
    if os.name == "nt":
        pytest.skip("POSIX fixture command dispatch")
    directory = tmp_path / "fixture-cargo"
    directory.mkdir()
    log = directory / "commands.jsonl"
    artifacts = tmp_path / "external-target/release"
    artifacts.mkdir(parents=True)
    command = directory / "cargo"
    command.write_text(
        f"#!{sys.executable}\n"
        "import json, os, pathlib, sys\n"
        f"with open({str(log)!r}, 'a', encoding='utf-8') as stream:\n"
        "    stream.write(json.dumps(sys.argv[1:]) + '\\n')\n"
        "arguments = sys.argv[1:]\n"
        "if arguments == ['-V']:\n"
        "    print('cargo 1.99.0 (fixture)')\n"
        "elif arguments[0] == 'build':\n"
        f"    if pathlib.Path({str(directory / 'fail-build')!r}).exists():\n"
        "        print('fixture watch compiler failure', file=sys.stderr)\n"
        "        raise SystemExit(61)\n"
        "    for index, argument in enumerate(arguments[:-1]):\n"
        "        if argument != '-p': continue\n"
        "        package = arguments[index + 1]\n"
        "        name = 'wez-vtabs-store' if package == 'vtabs-store' else package\n"
        f"        artifact = pathlib.Path({str(artifacts)!r}) / name\n"
        f"        if pathlib.Path({str(directory / 'honor-target-dir')!r}).exists():\n"
        "            profile = arguments[arguments.index('--profile') + 1]\n"
        "            artifact = pathlib.Path(os.environ['CARGO_TARGET_DIR']) / profile / name\n"
        "            artifact.parent.mkdir(parents=True, exist_ok=True)\n"
        "        if not artifact.exists(): artifact.write_text('#!/bin/sh\\nexit 0\\n')\n"
        "        artifact.chmod(0o755)\n"
        "        print(json.dumps({'reason': 'compiler-artifact', 'target': {'name': name}, "
        "'executable': str(artifact)}))\n"
        f"    change = pathlib.Path({str(directory / 'change-source')!r})\n"
        "    if change.exists():\n"
        "        pathlib.Path(change.read_text()).write_text('edited during compilation\\n')\n"
        "        change.unlink()\n",
        encoding="utf-8",
    )
    command.chmod(0o755)
    for name in ("tic", "codesign"):
        tool = directory / name
        tool.write_text(f"#!{sys.executable}\nraise SystemExit(0)\n", encoding="utf-8")
        tool.chmod(0o755)
    tools_sandbox.env["PATH"] = str(directory) + os.pathsep + tools_sandbox.env["PATH"]
    return log
