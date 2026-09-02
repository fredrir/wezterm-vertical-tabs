"""Small compatibility boundary around tui-test 0.1.0b2."""

from __future__ import annotations

import asyncio
import json
import os
import re
import secrets
import shlex
from collections.abc import Callable, Mapping
from dataclasses import asdict
from pathlib import Path
from time import monotonic
from typing import Any

from tui_test import Colors, Profile, Timeouts, TuiTest, get_recording

from .protocol import cast_output, protocol_events, user_vars


COLS = 28
ROWS = 20
_WAIT_SECONDS = 3.0
_POLL_SECONDS = 0.005
_SAFE_NAME = re.compile(r"[^A-Za-z0-9_.-]+")

_ENV = {
    "COLORTERM": "truecolor",
    "LANG": "C.UTF-8",
    "LC_ALL": "C",
    "TERM": "xterm-256color",
    "VTABS_BG": "",
    "VTABS_LOG": "",
    "VTABS_PANIC_ON_READY": "",
    "VTABS_USERVAR": "vtabs",
    "WEZTERM_EXECUTABLE_DIR": "",
    "WEZTERM_PANE": "",
    "WEZTERM_UNIX_SOCKET": "",
}

_PROFILE = Profile(
    scrollback=100,
    colors=Colors(
        foreground="#cdd6f4",
        background="#1e1e2e",
        cursor="#f5e0dc",
        black="#45475a",
        red="#f38ba8",
        green="#a6e3a1",
        yellow="#f9e2af",
        blue="#89b4fa",
        magenta="#f5c2e7",
        cyan="#94e2d5",
        white="#bac2de",
    ),
)


class Terminal:
    """A fresh wez-vtabs process inside a deterministic virtual PTY."""

    def __init__(
        self,
        binary: Path,
        *,
        cols: int = COLS,
        rows: int = ROWS,
        role: str = "sidebar",
        post_exit_sentinel: str | None = None,
    ) -> None:
        self.binary = binary
        self.cols = cols
        self.rows = rows
        self.role = role
        self.post_exit_sentinel = post_exit_sentinel
        self._token = secrets.token_hex(16)
        self._client = TuiTest.ephemeral(
            "wez-vtabs",
            profile=_PROFILE,
            timeouts=Timeouts(text=3000, idle=3000, exit=3000, ready=3000),
        )

    @property
    def session(self) -> str:
        return self._client.session

    async def start(self) -> Terminal:
        program = os.fspath(self.binary)
        args = ("--role", self.role)
        if self.post_exit_sentinel is not None:
            program = "/bin/sh"
            command = " ".join(shlex.quote(part) for part in (os.fspath(self.binary), *args))
            sentinel = shlex.quote(self.post_exit_sentinel)
            args = (
                "-c",
                f'before=$(stty -g) || exit; printf "%s-before\\n" {sentinel}; '
                f'{command}; status=$?; after=$(stty -g); '
                'if [ "$before" = "$after" ]; then mode=tty-restored; '
                'else mode=tty-changed; fi; '
                f'printf "%s-after\\n%s-%s\\n" {sentinel} {sentinel} "$mode"; '
                'exit "$status"',
            )
        await self._client.run(
            program,
            *args,
            cols=self.cols,
            rows=self.rows,
            env=_ENV,
            profile=_PROFILE,
        )
        await self.wait_event("ready")
        await self.send({"t": "auth", "token": self._token, "caps": []})
        echoed = await self.wait_user_var("vtabs_token")
        if echoed != self._token:
            raise AssertionError("backend did not authenticate the test control session")
        return self

    async def close(self) -> None:
        await self._client.close_quiet()

    async def send(self, *commands: Mapping[str, Any]) -> None:
        wire = "".join(
            "\x1eVTABS "
            + self._token
            + " "
            + json.dumps(command, separators=(",", ":"), ensure_ascii=False)
            + "\n"
            for command in commands
        )
        await self._client.write(wire)

    async def write(self, data: str) -> None:
        await self._client.write(data)

    async def wait_text(
        self,
        text: str,
        *,
        absent: bool = False,
        timeout: float = _WAIT_SECONDS,
    ) -> None:
        await self._client.wait_text(
            text,
            not_=absent,
            timeout=max(1, round(timeout * 1000)),
        )

    async def text(self) -> str:
        return await self._client.text(full=True)

    async def title(self) -> str | None:
        return await self._client.get_title()

    async def click_text(self, text: str) -> None:
        await self._client.mouse.click(on_text=text)

    async def locate_text(self, text: str) -> tuple[int, int]:
        """Return a zero-based point in test-owned visible text."""

        cells = await self._client.cells(0, 0, self.cols, self.rows)
        for y in range(self.rows):
            row = sorted((cell for cell in cells if cell.y == y), key=lambda cell: cell.x)
            visible: list[str] = []
            columns: list[int] = []
            for cell in row:
                if not cell.char:
                    continue
                visible.append(cell.char)
                columns.extend([cell.x] * len(cell.char))
            rendered = "".join(visible)
            if (start := rendered.find(text)) >= 0:
                end = start + len(text) - 1
                return ((columns[start] + columns[end]) // 2, y)
        raise AssertionError(f"visible text not found: {text!r}\nscreen:\n{await self.text()}")

    async def mouse_down(self, x: int, y: int, *, button: int = 0) -> None:
        await self._client.mouse.down(x, y, button=button)

    async def mouse_up(self, x: int, y: int, *, button: int = 0) -> None:
        await self._client.mouse.up(x, y, button=button)

    async def resize(self, cols: int, rows: int) -> None:
        self.cols = cols
        self.rows = rows
        await self._client.resize(cols, rows)

    async def recording(self) -> str:
        return await get_recording(self.session)

    async def output(self) -> str:
        return cast_output(await self.recording())

    async def events(self) -> list[dict[str, Any]]:
        return protocol_events(await self.recording())

    async def last_event_sequence(self) -> int:
        sequences = [
            event["n"]
            for event in await self.events()
            if isinstance(event.get("n"), int)
        ]
        return max(sequences, default=0)

    async def events_after(self, sequence: int) -> list[dict[str, Any]]:
        return [
            event
            for event in await self.events()
            if isinstance(event.get("n"), int) and event["n"] > sequence
        ]

    async def wait_event(
        self,
        event_type: str,
        *,
        timeout: float = _WAIT_SECONDS,
        where: Mapping[str, Any] | None = None,
        predicate: Callable[[dict[str, Any]], bool] | None = None,
    ) -> dict[str, Any]:
        expected = dict(where or {})

        def matches(event: dict[str, Any]) -> bool:
            return (
                event.get("t") == event_type
                and all(event.get(key) == value for key, value in expected.items())
                and (predicate is None or predicate(event))
            )

        deadline = monotonic() + timeout
        while True:
            events = await self.events()
            if event := next((event for event in events if matches(event)), None):
                return event
            if monotonic() >= deadline:
                screen = await self.text()
                raise AssertionError(
                    f"no {event_type!r} event matching {expected!r} within {timeout:.3f}s"
                    f"\nprotocol events: {events!r}\nscreen:\n{screen}"
                )
            await asyncio.sleep(_POLL_SECONDS)

    async def wait_user_var(
        self,
        name: str,
        *,
        timeout: float = _WAIT_SECONDS,
    ) -> str:
        deadline = monotonic() + timeout
        while True:
            values = [item.value for item in user_vars(await self.recording()) if item.name == name]
            if values:
                return values[-1]
            if monotonic() >= deadline:
                raise AssertionError(f"no {name!r} user variable within {timeout:.3f}s")
            await asyncio.sleep(_POLL_SECONDS)

    async def quit(self) -> None:
        await self.send({"t": "quit"})
        await self._client.wait_exit(timeout=3000)

    async def exit_status(self) -> int | None:
        return (await self._client.state()).exited

    async def capture_failure(self, directory: Path, test_name: str) -> list[Path]:
        directory.mkdir(parents=True, exist_ok=True)
        stem = _SAFE_NAME.sub("-", test_name).strip("-") or "tui-test"
        prefix = directory / f"{stem}-{self.session}"
        paths: list[Path] = []

        try:
            recording = await self.recording()
            cast_path = Path(f"{prefix}.cast")
            cast_path.write_text(recording, encoding="utf-8")
            paths.append(cast_path)
        except Exception:
            recording = ""

        try:
            state = await self._client.state()
            diagnostics = {
                "state": asdict(state),
                "screen": await self.text(),
                "events": protocol_events(recording) if recording else [],
            }
            diagnostics_path = Path(f"{prefix}.json")
            diagnostics_path.write_text(
                json.dumps(diagnostics, ensure_ascii=False, indent=2, default=str) + "\n",
                encoding="utf-8",
            )
            paths.append(diagnostics_path)
        except Exception:
            pass

        try:
            svg_path = Path(f"{prefix}.svg")
            await self._client.screenshot(os.fspath(svg_path))
            paths.append(svg_path)
        except Exception:
            pass
        return paths
