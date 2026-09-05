"""Portable bundle fixtures; application behavior is still exercised through Rust."""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import stat
import sys
from pathlib import Path


def binary_dir(bundle: Path) -> Path:
    if sys.platform == "darwin":
        return bundle / "WezTerm.app/Contents/MacOS"
    return bundle if os.name == "nt" else bundle / "bin"


def executable_name(name: str) -> str:
    return name + (".exe" if os.name == "nt" else "")


def write_manifest(bundle: Path) -> None:
    metadata = json.loads((bundle / "build.json").read_text())
    files = {}
    for path in sorted(bundle.rglob("*")):
        if path == bundle / "checksums.json" or path.is_dir():
            continue
        entry = (
            {"symlink": os.readlink(path)}
            if path.is_symlink()
            else {"sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
        )
        if os.name != "nt":
            entry["mode"] = stat.S_IMODE(path.lstat().st_mode)
        files[path.relative_to(bundle).as_posix()] = entry
    (bundle / "checksums.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "id": metadata["id"],
                "target": metadata["target"],
                "source_digest": metadata.get("source_digest", ""),
                "files": files,
            }
        ),
        encoding="utf-8",
    )


def create_bundle(
    bundle: Path,
    name: str,
    tools_binary: Path,
    target: str,
    native_binaries: dict[str, Path] | None = None,
) -> Path:
    binaries = binary_dir(bundle)
    binaries.mkdir(parents=True)
    source = bundle / "source"
    (source / "tools").mkdir(parents=True)
    (source / "Cargo.toml").write_text('[workspace]\nmembers = ["tools"]\n', encoding="utf-8")
    (source / "tools/Cargo.toml").write_text(
        '[package]\nname = "vtabs-tools"\nversion = "0.1.0"\n', encoding="utf-8"
    )
    shutil.copy2(tools_binary, binaries / executable_name("wez-vtabs"))
    for executable in ("wezterm-gui", "wez-vtabs-store"):
        destination = binaries / executable_name(executable)
        if native_binaries is not None:
            shutil.copy2(native_binaries[executable], destination)
        elif os.name == "nt":
            shutil.copy2(tools_binary, destination)
        else:
            destination.write_text(
                f"#!{sys.executable}\n"
                "import json, os, sys\n"
                f"print(json.dumps({{'id': {name!r}, 'arguments': sys.argv[1:], "
                "'bundle': os.environ.get('WEZ_VTABS_BUNDLE'), "
                "'offline': os.environ.get('WEZ_VTABS_OFFLINE')}))\n"
                "raise SystemExit(37)\n",
                encoding="utf-8",
            )
            destination.chmod(0o755)
    (bundle / "build.json").write_text(
        json.dumps({"id": name, "capability": 1, "target": target}), encoding="utf-8"
    )
    marker_dir = bundle / "WezTerm.app/Contents/Resources" if sys.platform == "darwin" else binaries
    marker_dir.mkdir(parents=True, exist_ok=True)
    (marker_dir / "native-bundle.json").write_text(
        json.dumps(
            {
                "root": os.path.relpath(bundle, marker_dir),
                "capability": 1,
                "updater_protocol": 1,
                "tool": (binaries / executable_name("wez-vtabs")).relative_to(bundle).as_posix(),
            }
        ),
        encoding="utf-8",
    )
    write_manifest(bundle)
    return bundle
