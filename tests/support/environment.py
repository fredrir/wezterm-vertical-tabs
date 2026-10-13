"""Child process environments without the caller's desktop, session or application state."""

from __future__ import annotations

import os
from collections.abc import Mapping
from pathlib import Path

SESSION_PREFIXES = ("WEZTERM_", "WEZ_VTABS_", "TMUX", "XDG_SESSION")
SESSION_KEYS = frozenset(
    {
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
        "SSH_AUTH_SOCK",
        "DBUS_SESSION_BUS_ADDRESS",
        "ZDOTDIR",
        "ENV",
        "BASH_ENV",
    }
)
XDG_DIRECTORIES = {
    "config": "XDG_CONFIG_HOME",
    "cache": "XDG_CACHE_HOME",
    "data": "XDG_DATA_HOME",
    "state": "XDG_STATE_HOME",
    "runtime": "XDG_RUNTIME_DIR",
}


def session_variable(key: str) -> bool:
    return key.startswith(SESSION_PREFIXES) or key in SESSION_KEYS


def clean_env(source: Mapping[str, str] = os.environ) -> dict[str, str]:
    return {key: value for key, value in source.items() if not session_variable(key)}


def xdg_tree(root: Path) -> dict[str, str]:
    environment = {}
    for name, variable in XDG_DIRECTORIES.items():
        directory = root / name
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        directory.chmod(0o700)
        environment[variable] = str(directory)
    return environment
