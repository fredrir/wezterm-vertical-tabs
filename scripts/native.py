#!/usr/bin/env python3
"""Build latest WezTerm main and install immutable native bundles."""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import os
import plistlib
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
UPSTREAM_URL = "https://github.com/wezterm/wezterm.git"
PROJECT_URL = "https://github.com/fredrir/wezterm-vertical-tabs.git"
PROJECT_BRANCH = "native"
CAPABILITY = 1
DAY = 24 * 60 * 60
BINARIES = ("wezterm-gui", "wezterm", "wezterm-mux-server", "strip-ansi-escapes")
WINDOWS_RUNTIME = (
    ("conhost", "conpty.dll"),
    ("conhost", "OpenConsole.exe"),
    ("angle", "libEGL.dll"),
    ("angle", "libGLESv2.dll"),
)
SOURCE_ITEMS = (
    "Cargo.toml",
    "Cargo.lock",
    "pyproject.toml",
    "uv.lock",
    "README.md",
    "justfile",
    "crates",
    "native",
    "plugin",
    "docs",
    "scripts/native.py",
    "tests",
)


def run(
    *args: object, cwd: Path | None = None, capture: bool = False, env: dict[str, str] | None = None
) -> str:
    command = [str(arg) for arg in args]
    if not capture:
        print("+ " + " ".join(command), flush=True)
    result = subprocess.run(
        command,
        cwd=cwd or ROOT,
        env=env,
        check=True,
        text=True,
        stdout=subprocess.PIPE if capture else None,
    )
    return result.stdout.strip() if capture else ""


def read_json(path: Path, default: object = None):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return default


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    try:
        temporary.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def cache_root() -> Path:
    if value := os.environ.get("WEZ_VTABS_CACHE"):
        return Path(value).expanduser().resolve()
    base = os.environ.get("LOCALAPPDATA") if os.name == "nt" else os.environ.get("XDG_CACHE_HOME")
    return (Path(base) if base else Path.home() / ".cache") / "wez-vtabs-native"


def install_root() -> Path:
    if value := os.environ.get("WEZ_VTABS_INSTALL"):
        return Path(value).expanduser().resolve()
    base = os.environ.get("LOCALAPPDATA") if os.name == "nt" else os.environ.get("XDG_DATA_HOME")
    return (Path(base) if base else Path.home() / ".local" / "share") / "wez-vtabs-native"


@contextlib.contextmanager
def locked(path: Path, wait: bool = True):
    """OS-owned lock; a crashed process cannot leave a stale lock."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a+b") as handle:
        if os.name == "nt":
            import msvcrt

            if path.stat().st_size == 0:
                handle.write(b"0")
                handle.flush()
            handle.seek(0)
            while True:
                try:
                    msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
                    break
                except OSError:
                    if not wait:
                        raise BlockingIOError("native operation already running") from None
                    time.sleep(0.25)
        else:
            import fcntl

            mode = fcntl.LOCK_EX | (0 if wait else fcntl.LOCK_NB)
            fcntl.flock(handle.fileno(), mode)
        try:
            yield
        finally:
            if os.name == "nt":
                handle.seek(0)
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(handle.fileno(), fcntl.LOCK_UN)


def source_files(root: Path):
    for item in SOURCE_ITEMS:
        path = root / item
        if path.is_file():
            yield path
        elif path.is_dir():
            yield from sorted(
                child
                for child in path.rglob("*")
                if child.is_file()
                and not set(child.relative_to(path).parts) & {"target", "__pycache__", ".git"}
            )


def source_digest(root: Path) -> str:
    digest = hashlib.sha256()
    for path in source_files(root):
        digest.update(path.relative_to(root).as_posix().encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def integration_digest(root: Path) -> str:
    digest = hashlib.sha256(str(root.resolve()).encode())
    for path in source_files(root):
        relative = path.relative_to(root)
        if relative.parts[0] == "native" or relative.as_posix() == "scripts/native.py":
            digest.update(relative.as_posix().encode())
            digest.update(path.read_bytes())
    return digest.hexdigest()


def target_triple() -> str:
    for line in run("rustc", "-vV", capture=True).splitlines():
        if line.startswith("host: "):
            return line.removeprefix("host: ")
    raise RuntimeError("rustc host target missing")


def safe_id(value: str) -> str:
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,159}", value):
        raise RuntimeError("invalid bundle ID")
    return value


def refresh(cache: Path) -> tuple[Path, str]:
    upstream = cache / "upstream"
    if not upstream.exists():
        run(
            "git",
            "clone",
            "--filter=blob:none",
            "--single-branch",
            "--branch",
            "main",
            UPSTREAM_URL,
            upstream,
        )
    origin = run("git", "remote", "get-url", "origin", cwd=upstream, capture=True)
    if origin.rstrip("/").removesuffix(".git") != UPSTREAM_URL.removesuffix(".git"):
        raise RuntimeError(f"unexpected upstream remote: {origin}")
    run("git", "fetch", "--prune", "origin", "main", cwd=upstream)
    revision = run("git", "rev-parse", "origin/main", cwd=upstream, capture=True)
    return upstream, revision


def project_branch(value: str) -> str:
    if (
        not isinstance(value, str)
        or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._/-]{0,127}", value)
        or ".." in value
        or value.endswith(("/", "."))
        or any(
            not part or part.startswith(".") or part.endswith(".lock") for part in value.split("/")
        )
    ):
        raise RuntimeError("invalid project update branch")
    return value


def project_source() -> dict:
    metadata = read_json(ROOT.parent / "build.json", {})
    recorded = metadata.get("project_source", {}) if isinstance(metadata, dict) else {}
    if not isinstance(recorded, dict):
        raise RuntimeError("invalid recorded project source")
    if recorded.get("remote", PROJECT_URL) != PROJECT_URL:
        raise RuntimeError("unexpected recorded project source remote")
    branch = project_branch(
        os.environ.get("WEZ_VTABS_PROJECT_BRANCH") or recorded.get("branch", PROJECT_BRANCH)
    )
    revision = recorded.get("revision")
    if (ROOT / ".git").exists():
        revision = run("git", "rev-parse", "HEAD", cwd=ROOT, capture=True)
    return {"remote": PROJECT_URL, "branch": branch, "revision": revision}


def refresh_project(cache: Path, branch: str = PROJECT_BRANCH) -> Path:
    branch = project_branch(branch)
    project = cache / "project"
    ownership = project / ".git" / "wez-vtabs-native.json"
    identity = {"path": str(project.resolve()), "remote": PROJECT_URL, "capability": CAPABILITY}
    if not project.exists():
        run(
            "git",
            "clone",
            "--filter=blob:none",
            "--single-branch",
            "--branch",
            branch,
            PROJECT_URL,
            project,
        )
        write_json(ownership, identity)
    elif read_json(ownership) != identity:
        raise RuntimeError(
            "refusing to rewrite an unowned project cache; choose an empty WEZ_VTABS_CACHE"
        )
    origin = run("git", "remote", "get-url", "origin", cwd=project, capture=True)
    if origin.rstrip("/").removesuffix(".git") != PROJECT_URL.removesuffix(".git"):
        raise RuntimeError("unexpected project cache remote")
    remote_ref = f"refs/remotes/origin/{branch}"
    run("git", "fetch", "--prune", "origin", f"+refs/heads/{branch}:{remote_ref}", cwd=project)
    run("git", "reset", "--hard", remote_ref, cwd=project)
    run("git", "clean", "-ffd", cwd=project)
    if not all(
        (project / name).is_file()
        for name in ("Cargo.toml", "crates/vtabs-app/Cargo.toml", "scripts/native.py")
    ) or not list((project / "native/patches").glob("*.patch")):
        raise RuntimeError(
            f"project branch {branch} does not contain a complete native implementation"
        )
    return project


def stage_adapter(worktree: Path) -> None:
    adapter = ROOT / "native" / "adapter"
    if not (adapter / "mod.rs").is_file():
        raise RuntimeError("native/adapter/mod.rs missing")
    gui = worktree / "wezterm-gui"
    shutil.copy2(adapter / "mod.rs", gui / "src" / "native_vtabs.rs")
    modules = gui / "src" / "native_vtabs"
    modules.mkdir(exist_ok=True)
    for path in adapter.iterdir():
        if path.name == "mod.rs":
            continue
        destination = modules / path.name
        if path.is_dir():
            shutil.copytree(path, destination, dirs_exist_ok=True)
        else:
            shutil.copy2(path, destination)
    manifest = gui / "Cargo.toml"
    text = manifest.read_text(encoding="utf-8")
    marker = "[dependencies]\n"
    if text.count(marker) != 1:
        raise RuntimeError("WezTerm GUI dependency section changed")
    additions = "".join(
        f"{name} = {{ path = {json.dumps(str(ROOT / 'crates' / name))}, default-features = false }}\n"
        for name in ("vtabs-app", "vtabs-store")
    )
    additions += 'ratatui = { version = "0.30.2", default-features = false }\n'
    if not re.search(r"^unicode-width\s*[.=]", text, re.MULTILINE):
        additions += 'unicode-width = "0.2"\n'
    manifest.write_text(text.replace(marker, marker + additions, 1), encoding="utf-8")


def prepare(cache: Path, upstream: Path, revision: str) -> Path:
    worktree = cache / "worktree"
    integration = integration_digest(ROOT)
    prepared = read_json(cache / "prepared.json", {})
    if worktree.exists():
        common = Path(
            run(
                "git",
                "rev-parse",
                "--path-format=absolute",
                "--git-common-dir",
                cwd=worktree,
                capture=True,
            )
        ).resolve()
        expected = Path(
            run(
                "git",
                "rev-parse",
                "--path-format=absolute",
                "--git-common-dir",
                cwd=upstream,
                capture=True,
            )
        ).resolve()
        if common != expected or worktree.resolve() == upstream.resolve():
            raise RuntimeError("refusing to replace an unowned worktree")
        if prepared.get("upstream") == revision and prepared.get("integration") == integration:
            print("native checkout current")
            return worktree
        run("git", "worktree", "remove", "--force", "--force", worktree, cwd=upstream)
    run("git", "worktree", "prune", cwd=upstream)
    run("git", "worktree", "add", "--detach", worktree, revision, cwd=upstream)
    run("git", "submodule", "update", "--init", "--recursive", "--depth", "1", cwd=worktree)
    patches = sorted((ROOT / "native" / "patches").glob("*.patch"))
    if not patches:
        raise RuntimeError("native patches missing")
    for patch in patches:
        run("git", "apply", "--check", patch, cwd=worktree)
        run("git", "apply", patch, cwd=worktree)
    stage_adapter(worktree)
    write_json(cache / "prepared.json", {"upstream": revision, "integration": integration})
    return worktree


def build(cache: Path, debug: bool = False) -> dict:
    upstream, revision = refresh(cache)
    fingerprint = source_digest(ROOT)
    target = target_triple()
    profile = "debug" if debug else "release"
    project = project_source()
    source_id = hashlib.sha256(json.dumps(project, sort_keys=True).encode()).hexdigest()[:8]
    key = f"{revision[:12]}-{fingerprint[:12]}-{source_id}-{target}-{profile}"
    metadata = {
        "id": key,
        "capability": CAPABILITY,
        "upstream": revision,
        "source_digest": fingerprint,
        "target": target,
        "profile": profile,
        "project_source": project,
    }
    previous = read_json(cache / "build.json", {})
    binaries = upstream / "target" / profile
    extension = ".exe" if os.name == "nt" else ""
    prepared = read_json(cache / "prepared.json", {})
    if (
        previous.get("id") == key
        and prepared.get("upstream") == revision
        and prepared.get("integration") == integration_digest(ROOT)
        and (cache / "worktree").is_dir()
        and all(
            (binaries / (name + extension)).is_file() for name in (*BINARIES, "wez-vtabs-store")
        )
    ):
        print(f"native build current: {key}")
        return previous
    worktree = prepare(cache, upstream, revision)
    env = {**os.environ, "CARGO_TARGET_DIR": str(upstream / "target")}
    flags = [] if debug else ["--release"]
    run(
        "cargo",
        "build",
        *flags,
        "--locked",
        "--manifest-path",
        ROOT / "Cargo.toml",
        "-p",
        "vtabs-store",
        "--features",
        "sqlite",
        env=env,
    )
    run(
        "cargo",
        "build",
        *flags,
        "-p",
        "wezterm-gui",
        "-p",
        "wezterm",
        "-p",
        "wezterm-mux-server",
        "-p",
        "strip-ansi-escapes",
        cwd=worktree,
        env=env,
    )
    if os.name == "nt":
        copy_windows_runtime(worktree, (binaries, binaries / "deps"))
    run("cargo", "test", *flags, "-p", "wezterm-gui", "native_", cwd=worktree, env=env)
    run("cargo", "test", *flags, "-p", "wezterm-client", "native_", "--lib", cwd=worktree, env=env)
    run(
        "cargo",
        "test",
        *flags,
        "-p",
        "wezterm-input-types",
        "native_",
        "--lib",
        cwd=worktree,
        env=env,
    )
    verify_source(metadata)
    metadata["built_at"] = int(time.time())
    write_json(cache / "build.json", metadata)
    return metadata


def copy_source(destination: Path) -> None:
    for path in source_files(ROOT):
        target = destination / path.relative_to(ROOT)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)


def verify_source(metadata: dict) -> None:
    if source_digest(ROOT) != metadata.get("source_digest"):
        raise RuntimeError("project source changed during build; run again")


def package(cache: Path, metadata: dict, output: Path) -> Path:
    verify_source(metadata)
    name = safe_id("wez-vtabs-native-" + metadata["id"])
    output.mkdir(parents=True, exist_ok=True)
    destination = output / name
    if destination.exists():
        if read_json(destination / "build.json", {}).get("id") == metadata["id"]:
            archive_bundle(destination)
            return destination
        raise RuntimeError("bundle already exists with different metadata")
    staging = Path(tempfile.mkdtemp(prefix=".bundle-", dir=output))
    source = cache / "worktree"
    binaries = cache / "upstream" / "target" / metadata["profile"]
    try:
        if sys.platform == "darwin":
            app = staging / "WezTerm.app"
            shutil.copytree(source / "assets" / "macos" / "WezTerm.app", app, symlinks=True)
            for library in app.glob("*.dylib"):
                library.unlink()
            bindir = app / "Contents" / "MacOS"
            resources = app / "Contents" / "Resources"
        elif os.name == "nt":
            bindir = staging
            resources = staging / "share"
        else:
            bindir = staging / "bin"
            resources = staging / "share"
        bindir.mkdir(parents=True, exist_ok=True)
        resources.mkdir(parents=True, exist_ok=True)
        licenses = resources / "licenses"
        licenses.mkdir()
        shutil.copy2(source / "LICENSE.md", licenses / "WezTerm-LICENSE.md")
        for license_file in sorted((source / "assets/fonts").glob("LICENSE*")):
            shutil.copy2(license_file, licenses / license_file.name)
        extension = ".exe" if os.name == "nt" else ""
        for name in (*BINARIES, "wez-vtabs-store"):
            shutil.copy2(binaries / (name + extension), bindir / (name + extension))
        shell_assets(source, staging, resources)
        if os.name == "nt":
            copy_windows_runtime(source, (bindir,))
        else:
            run(
                "tic",
                "-xe",
                "wezterm",
                "-o",
                resources / "terminfo",
                source / "termwiz" / "data" / "wezterm.terminfo",
            )
        if sys.platform.startswith("linux"):
            shutil.copytree(source / "assets" / "icon", resources / "icons")
            shutil.copy2(source / "assets" / "wezterm.desktop", resources / "wezterm.desktop")
            shutil.copy2(
                source / "assets" / "wezterm.appdata.xml", resources / "wezterm.appdata.xml"
            )
        shutil.copytree(ROOT / "plugin", resources / "plugin")
        copy_source(staging / "source")
        if source_digest(staging / "source") != metadata["source_digest"]:
            raise RuntimeError("project source changed during packaging; run again")
        write_json(staging / "build.json", metadata)
        # macOS keeps data outside its code-only MacOS directory.
        marker_dir = resources if sys.platform == "darwin" else bindir
        write_json(
            marker_dir / "native-bundle.json",
            {"root": os.path.relpath(staging, marker_dir), "capability": CAPABILITY},
        )
        if sys.platform == "darwin":
            run("codesign", "--force", "--deep", "--sign", "-", staging / "WezTerm.app")
            run("codesign", "--verify", "--deep", "--strict", staging / "WezTerm.app")
        staging.rename(destination)
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    archive_bundle(destination)
    print(destination)
    return destination


def archive_bundle(destination: Path) -> Path:
    zipped = os.name == "nt" or sys.platform == "darwin"
    archive_path = destination.with_name(destination.name + (".zip" if zipped else ".tar.gz"))
    if archive_path.is_file():
        return archive_path
    staging = Path(tempfile.mkdtemp(prefix=".archive-", dir=destination.parent))
    try:
        if zipped:
            temporary = staging / "bundle.zip"
            # zipfile preserves source Unix permissions in external_attr.
            with zipfile.ZipFile(temporary, "w", zipfile.ZIP_DEFLATED) as archive:
                for path in sorted(destination.rglob("*")):
                    archive.write(path, path.relative_to(destination.parent))
        else:
            temporary = Path(
                shutil.make_archive(
                    str(staging / "bundle"),
                    "gztar",
                    root_dir=destination.parent,
                    base_dir=destination.name,
                )
            )
        os.replace(temporary, archive_path)
    finally:
        shutil.rmtree(staging, ignore_errors=True)
    return archive_path


def copy_windows_runtime(source: Path, destinations: tuple[Path, ...]) -> None:
    assets = source / "assets" / "windows"
    for destination in destinations:
        destination.mkdir(parents=True, exist_ok=True)
        for directory, name in WINDOWS_RUNTIME:
            shutil.copy2(assets / directory / name, destination / name)
        shutil.copytree(assets / "mesa", destination / "mesa", dirs_exist_ok=True)


def shell_assets(source: Path, bundle: Path, resources: Path) -> None:
    assets = source / "assets"
    if sys.platform == "darwin":
        shutil.copytree(assets / "shell-integration", resources, dirs_exist_ok=True)
        shutil.copytree(assets / "shell-completion", resources / "shell-completion")
    elif sys.platform.startswith("linux"):
        for original, relative in (
            ("shell-integration/wezterm.sh", "etc/profile.d/wezterm.sh"),
            ("shell-completion/bash", "share/bash-completion/completions/wezterm"),
            ("shell-completion/zsh", "share/zsh/site-functions/_wezterm"),
        ):
            destination = bundle / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(assets / original, destination)
    else:
        for name in ("shell-integration", "shell-completion"):
            shutil.copytree(assets / name, resources / name)


def desktop_quote(value: str) -> str:
    # Desktop Entry quoting has its own escaping rules, separate from a shell.
    value = value.replace("%", "%%")
    for character in ("\\", '"', "`", "$"):
        value = value.replace(character, "\\" + character)
    value = (
        value.replace("\\", "\\\\").replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t")
    )
    return '"' + value + '"'


def install_entry(root: Path, bundle: Path) -> Path:
    python = str(Path(sys.executable).resolve())
    launcher = root / "native.py"
    if sys.platform == "win32":
        entry = root / "wez-vtabs.cmd"
        python = python.replace("%", "%%")
        entry.write_text(f'@echo off\n"{python}" "%~dp0native.py" launch -- %*\n', encoding="utf-8")
    else:
        entry = root / "wez-vtabs"
        entry.write_text(
            f'#!/bin/sh\nexec {shlex.quote(python)} {shlex.quote(str(launcher))} launch -- "$@"\n',
            encoding="utf-8",
        )
        entry.chmod(0o755)
        if sys.platform == "darwin":
            app = root / "WezTerm Native.app"
            contents = app / "Contents"
            (contents / "MacOS").mkdir(parents=True, exist_ok=True)
            (contents / "Resources").mkdir(exist_ok=True)
            executable = contents / "MacOS" / "launch"
            shutil.copy2(entry, executable)
            icon = bundle / "WezTerm.app/Contents/Resources/terminal.icns"
            if icon.is_file():
                shutil.copy2(icon, contents / "Resources/terminal.icns")
            with (contents / "Info.plist").open("wb") as output:
                plistlib.dump(
                    {
                        "CFBundleExecutable": "launch",
                        "CFBundleIdentifier": "dev.fredrir.wez-vtabs.launcher",
                        "CFBundleName": "WezTerm Native",
                        "CFBundleDisplayName": "WezTerm Native",
                        "CFBundlePackageType": "APPL",
                        "CFBundleVersion": "1",
                        "CFBundleIconFile": "terminal.icns",
                        "NSHighResolutionCapable": True,
                    },
                    output,
                )
            entry = app
        else:
            icon = bundle / "share/icons/terminal.png"
            desktop = root / "wez-vtabs.desktop"
            icon_value = (
                str(icon)
                .replace("\\", "\\\\")
                .replace("\n", "\\n")
                .replace("\r", "\\r")
                .replace("\t", "\\t")
            )
            desktop.write_text(
                "[Desktop Entry]\nName=WezTerm Native\nType=Application\nTerminal=false\nCategories=System;TerminalEmulator;\nStartupWMClass=org.wezfurlong.wezterm\n"
                + f"Exec={desktop_quote(python)} {desktop_quote(str(launcher))} launch\nIcon={icon_value}\n",
                encoding="utf-8",
            )
            desktop.chmod(0o755)
    return entry


def install(bundle: Path, root: Path, stage_only: bool = False) -> Path:
    metadata = read_json(bundle / "build.json")
    if not isinstance(metadata, dict) or metadata.get("capability") != CAPABILITY:
        raise RuntimeError("native bundle metadata missing or incompatible")
    executable = gui_path(bundle)
    helper = executable.parent / ("wez-vtabs-store.exe" if os.name == "nt" else "wez-vtabs-store")
    if not all(
        path.is_file()
        for path in (
            executable,
            helper,
            bundle / "source" / "Cargo.toml",
            bundle / "source" / "scripts" / "native.py",
        )
    ):
        raise RuntimeError("native bundle incomplete")
    identifier = safe_id(metadata["id"])
    versions = root / "versions"
    versions.mkdir(parents=True, exist_ok=True)
    destination = versions / identifier
    with locked(root / "install.lock"):
        if not destination.exists():
            staging = Path(tempfile.mkdtemp(prefix=".install-", dir=versions))
            try:
                shutil.copytree(bundle, staging, dirs_exist_ok=True, symlinks=True)
                staging.rename(destination)
            except BaseException:
                shutil.rmtree(staging, ignore_errors=True)
                raise
        elif read_json(destination / "build.json", {}).get("id") != identifier:
            raise RuntimeError("installed bundle ID collision")
        write_json(root / ("pending.json" if stage_only else "active.json"), {"id": identifier})
        if not stage_only:
            (root / "pending.json").unlink(missing_ok=True)
        write_json(root / "runtime.json", {"python": str(Path(sys.executable).resolve())})
        launcher = root / "native.py"
        if Path(__file__).resolve() != launcher.resolve():
            shutil.copy2(Path(__file__), launcher)
        entry = install_entry(root, destination)
    print(f"{'staged' if stage_only else 'installed'}: {destination}")
    print(f"launcher: {entry}")
    return destination


def current_bundle(root: Path, promote: bool = True) -> Path:
    with locked(root / "install.lock"):
        pending = read_json(root / "pending.json") if promote else None
        if pending:
            identifier = safe_id(pending["id"])
            if not (root / "versions" / identifier / "build.json").is_file():
                raise RuntimeError("pending bundle incomplete")
            write_json(root / "active.json", pending)
            (root / "pending.json").unlink()
        active = read_json(root / "active.json")
        if not active:
            raise RuntimeError("native bundle not installed; run just install")
        bundle = root / "versions" / safe_id(active["id"])
        if not (bundle / "build.json").is_file():
            raise RuntimeError("active bundle missing")
        return bundle


def gui_path(bundle: Path) -> Path:
    if sys.platform == "darwin":
        return bundle / "WezTerm.app" / "Contents" / "MacOS" / "wezterm-gui"
    return bundle / ("wezterm-gui.exe" if os.name == "nt" else "bin/wezterm-gui")


def queue_daily_update(bundle: Path, root: Path) -> None:
    state = read_json(root / "update.json", {})
    if time.time() - state.get("last_attempt", 0) < DAY:
        return
    script = bundle / "source" / "scripts" / "native.py"
    root.mkdir(parents=True, exist_ok=True)
    with (root / "update.log").open("ab") as output:
        options = (
            {"creationflags": subprocess.CREATE_NO_WINDOW}
            if os.name == "nt"
            else {"start_new_session": True}
        )
        subprocess.Popen(
            [sys.executable, str(script), "update", "--daily", "--stage-only"],
            stdin=subprocess.DEVNULL,
            stdout=output,
            stderr=output,
            env={**os.environ, "WEZ_VTABS_INSTALL": str(root)},
            **options,
        )


def update(cache: Path, root: Path, daily: bool, stage_only: bool, output: Path) -> None:
    delegate = None
    project = None
    source_synced = os.environ.get("WEZ_VTABS_UPDATE_SOURCE_SYNCED") == "1"
    with locked(root / "update.lock"):
        state = read_json(root / "update.json", {})
        if daily and not source_synced and time.time() - state.get("last_attempt", 0) < DAY:
            return
        state = {"last_attempt": int(time.time()), "status": "building"}
        write_json(root / "update.json", state)
        try:
            with locked(cache / "build.lock"):
                if (ROOT.parent / "build.json").is_file() and not source_synced:
                    project = project_source()
                    checkout = refresh_project(cache, project["branch"])
                    delegate = [
                        sys.executable,
                        str(checkout / "scripts/native.py"),
                        "update",
                        "--output",
                        str(output),
                    ]
                    if stage_only:
                        delegate.append("--stage-only")
                    state.update(status="project_synced")
                else:
                    metadata = build(cache)
                    bundle = package(cache, metadata, output)
                    install(bundle, root, stage_only)
                    state.update(status="ready", id=metadata["id"])
        except BaseException as error:
            state.update(status="failed", error=str(error))
            raise
        finally:
            write_json(root / "update.json", state)
    if delegate:
        # New updater code runs outside both locks; its own update transaction owns
        # build/install. Developer checkouts are never fetched or rewritten.
        try:
            run(
                *delegate,
                env={
                    **os.environ,
                    "WEZ_VTABS_UPDATE_SOURCE_SYNCED": "1",
                    "WEZ_VTABS_PROJECT_BRANCH": project["branch"],
                    "WEZ_VTABS_INSTALL": str(root),
                    "WEZ_VTABS_CACHE": str(cache),
                },
            )
        except BaseException as error:
            write_json(
                root / "update.json",
                {"last_attempt": int(time.time()), "status": "failed", "error": str(error)},
            )
            raise


def check() -> None:
    run("uv", "lock", "--check")
    run("uv", "run", "--locked", "ruff", "check", "scripts", "tests")
    run("uv", "run", "--locked", "ruff", "format", "--check", "scripts", "tests")
    environment = {**os.environ, "CARGO_TARGET_DIR": str(ROOT / "target" / "pytest")}
    run("cargo", "fmt", "--all", "--check")
    run("cargo", "test", "--workspace", "--all-features", "--locked", env=environment)
    run(
        "cargo",
        "clippy",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--locked",
        "--",
        "-D",
        "warnings",
        env=environment,
    )
    for format_name, path in (
        ("lua", "plugin/schema.lua"),
        ("types", "plugin/types.lua"),
        ("markdown", "docs/options.md"),
    ):
        run(
            "cargo",
            "run",
            "--quiet",
            "--locked",
            "-p",
            "vtabs-core",
            "--bin",
            "gen-schema",
            "--",
            "--check",
            format_name,
            path,
            env=environment,
        )
    run("uv", "run", "--locked", "pytest", "-n", "2")


def main(argv: list[str] | None = None) -> int:
    global ROOT
    installed = Path(__file__).resolve().parent
    managed_launcher = (installed / "active.json").is_file()
    if managed_launcher:
        ROOT = current_bundle(installed, promote=False) / "source"
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "command",
        choices=(
            "prepare",
            "build",
            "dev",
            "check",
            "package",
            "install",
            "update",
            "launch",
            "doctor",
        ),
    )
    parser.add_argument("--debug", action="store_true")
    parser.add_argument("--daily", action="store_true")
    parser.add_argument("--stage-only", action="store_true")
    parser.add_argument("--bundle", type=Path)
    parser.add_argument("--output", type=Path)
    args, gui_args = parser.parse_known_args(argv)
    if gui_args[:1] == ["--"]:
        gui_args = gui_args[1:]
    if gui_args and args.command not in ("dev", "launch"):
        parser.error("unexpected arguments: " + " ".join(gui_args))
    cache = cache_root()
    root = (
        installed
        if managed_launcher and not os.environ.get("WEZ_VTABS_INSTALL")
        else install_root()
    )
    if args.command == "check":
        check()
    elif args.command == "doctor":
        for command in ("git", "cargo", "rustc"):
            run(command, "--version")
        print(
            json.dumps(
                {
                    "cache": str(cache),
                    "install": str(root),
                    "build": read_json(cache / "build.json"),
                    "update": read_json(root / "update.json"),
                },
                indent=2,
            )
        )
    elif args.command == "launch":
        bundle = current_bundle(root)
        queue_daily_update(bundle, root)
        return subprocess.call(
            [str(gui_path(bundle)), *(gui_args or ["start"])],
            env={**os.environ, "WEZ_VTABS_BUNDLE": str(bundle)},
        )
    elif args.command == "update":
        update(cache, root, args.daily, args.stage_only, args.output or cache / "bundles")
    elif args.command == "install" and args.bundle:
        install(args.bundle.resolve(), root, args.stage_only)
    else:
        with locked(cache / "build.lock"):
            if args.command == "prepare":
                upstream, revision = refresh(cache)
                prepare(cache, upstream, revision)
                return 0
            metadata = build(cache, args.debug)
            if args.command in ("package", "install", "dev"):
                bundle = package(cache, metadata, args.output or ROOT / "dist")
                if args.command == "install":
                    install(bundle, root, args.stage_only)
        if args.command == "dev":
            return subprocess.call(
                [str(gui_path(bundle)), *(gui_args or ["start", "--always-new-process"])],
                env={**os.environ, "WEZ_VTABS_BUNDLE": str(bundle)},
            )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"native: {error}", file=sys.stderr)
        raise SystemExit(1) from None
