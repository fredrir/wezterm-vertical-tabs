#!/usr/bin/env python3
"""Compatibility entry for bundles installed before the Rust tooling migration."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    command = [
        "cargo",
        "run",
        "--quiet",
        "--locked",
        "--manifest-path",
        str(root / "tools/Cargo.toml"),
        "--bin",
        "wez-vtabs",
        "--",
        "--project-root",
        str(root),
        *sys.argv[1:],
    ]
    # Old installed updaters call this path after refreshing their owned cache.
    # The new installer replaces Python launch entries with the bundled binary.
    try:
        return subprocess.call(command, env=os.environ)
    except OSError as error:
        print(f"native: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
