"""Protocol messages shared by black-box scenarios."""

from __future__ import annotations

from collections.abc import Iterable
from typing import Any

from .adapter import Terminal

CONFIG: dict[str, Any] = {
    "t": "config",
    "rail_width": 5,
    "position": "left",
    "icons": False,
    "meta": "cwd",
    "meta_sep": " · ",
    "double_click_ms": 300,
    "tear_off": True,
    "wheel": "scroll",
    "context": "popover",
    "hover_timeout_ms": 0,
    "hover_highlight": False,
    "ellipsis": "…",
    "render": {
        "meta": True,
        "padding": {"left": 1, "right": 1, "top": 1, "bottom": 1},
        "frame": False,
        "row_gap": 0,
        "new_tab_button": False,
    },
}

THEME: dict[str, Any] = {
    "t": "theme",
    "scheme": {
        "background": "#1e1e2e",
        "foreground": "#cdd6f4",
        "cursor_bg": "#f5e0dc",
        "active_tab_bg": "#313244",
        "ansi": [
            "#45475a",
            "#f38ba8",
            "#a6e3a1",
            "#f9e2af",
            "#89b4fa",
            "#f5c2e7",
            "#94e2d5",
            "#bac2de",
        ],
    },
    "overrides": {"accent": "#89b4fa", "elevation": 0.06},
    "private": False,
}

MENU_CLOSED: dict[str, Any] = {"t": "menu", "open": False}


def tab(tab_id: int, title: str, *, index: int | None = None) -> dict[str, Any]:
    return {
        "id": tab_id,
        "index": tab_id if index is None else index,
        "title": title,
        "pane_title": title,
        "proc": "zsh",
        "cwd": f"~/work/{title.lower().replace(' ', '-')}",
        "domain": "local",
        "pinned": False,
        "unseen": False,
    }


def sidebar_model(*, active: int | None = None) -> dict[str, Any]:
    return {
        "t": "model",
        "rail": False,
        "active": active,
        "focus": {"on": False, "index": 1},
        "scroll": {"top": 1, "user": False},
        "strip": {
            "buttons": [
                {"id": "toggle_sidebar"},
                {"id": "open_settings"},
            ]
        },
        "footer": [],
    }


def spaces_section(tabs: Iterable[dict[str, Any]], *, active: int | None = None) -> dict[str, Any]:
    records = list(tabs)
    return {
        "t": "spaces",
        "window_id": 1,
        "enabled": False,
        "hook": False,
        "definitions": [],
        "tabs": records,
        "active_tab": active if active is not None else (records[0]["id"] if records else None),
        "last_tabs": [],
        "dynamics": [],
    }


async def dress_sidebar(
    terminal: Terminal,
    tabs: Iterable[dict[str, Any]],
    *,
    active: int | None = None,
) -> None:
    """Send a complete public state; incomplete state intentionally does not draw."""

    records = list(tabs)
    active_tab = active if active is not None else (records[0]["id"] if records else None)
    await terminal.publish(
        CONFIG,
        THEME,
        spaces_section(records, active=active_tab),
        sidebar_model(active=active_tab),
        MENU_CLOSED,
    )
