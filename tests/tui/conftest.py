from __future__ import annotations

import os
import shutil
import subprocess
from collections.abc import AsyncIterator, Awaitable, Callable
from pathlib import Path
from typing import Any

import pytest
import pytest_asyncio

from .adapter import COLS, ROWS, Terminal


ROOT = Path(__file__).resolve().parents[2]
BACKEND = ROOT / "backend"
TUI_TESTS = Path(__file__).resolve().parent


@pytest.hookimpl(hookwrapper=True)
def pytest_runtest_makereport(item: pytest.Item, call: pytest.CallInfo[Any]):
    outcome = yield
    report = outcome.get_result()
    setattr(item, f"rep_{report.when}", report)


def pytest_collection_modifyitems(items: list[pytest.Item]) -> None:
    for item in items:
        if Path(str(item.path)).resolve().is_relative_to(TUI_TESTS):
            item.add_marker("tui")


@pytest.fixture(scope="session")
def wez_vtabs_binary() -> Path:
    """Build once per run, or use an explicitly supplied executable."""

    override = os.environ.get("WEZ_VTABS_BIN")
    if override:
        binary = Path(override).expanduser().resolve()
        if not binary.is_file():
            pytest.fail(f"WEZ_VTABS_BIN is not a file: {binary}")
        if os.name != "nt" and not os.access(binary, os.X_OK):
            pytest.fail(f"WEZ_VTABS_BIN is not executable: {binary}")
        return binary

    cargo = shutil.which("cargo")
    if cargo is None:
        pytest.fail("cargo is required to build wez-vtabs (or set WEZ_VTABS_BIN)")
    subprocess.run(
        [cargo, "build", "--locked", "--quiet", "-p", "wez-vtabs"],
        cwd=BACKEND,
        check=True,
    )
    executable = "wez-vtabs.exe" if os.name == "nt" else "wez-vtabs"
    return (BACKEND / "target" / "debug" / executable).resolve(strict=True)


@pytest.fixture(scope="session")
def tui_artifact_dir() -> Path:
    configured = os.environ.get("VTABS_TEST_ARTIFACTS")
    return Path(configured).expanduser() if configured else ROOT / ".pytest-artifacts"


TerminalFactory = Callable[..., Awaitable[Terminal]]


@pytest_asyncio.fixture
async def terminal_factory(
    wez_vtabs_binary: Path,
    tui_artifact_dir: Path,
    request: pytest.FixtureRequest,
) -> AsyncIterator[TerminalFactory]:
    terminals: list[Terminal] = []

    async def launch(
        *,
        cols: int = COLS,
        rows: int = ROWS,
        role: str = "sidebar",
        post_exit_sentinel: str | None = None,
    ) -> Terminal:
        terminal = Terminal(
            wez_vtabs_binary,
            cols=cols,
            rows=rows,
            role=role,
            post_exit_sentinel=post_exit_sentinel,
        )
        terminals.append(terminal)
        return await terminal.start()

    try:
        yield launch
    finally:
        report = getattr(request.node, "rep_call", None)
        try:
            if report is not None and report.failed:
                try:
                    captured: list[Path] = []
                    for terminal in terminals:
                        captured.extend(
                            await terminal.capture_failure(tui_artifact_dir, request.node.nodeid)
                        )
                    if captured:
                        report.sections.append(
                            ("tui-test artifacts", "\n".join(str(path) for path in captured))
                        )
                except Exception as error:
                    report.sections.append(("tui-test artifact error", repr(error)))
        finally:
            for terminal in reversed(terminals):
                await terminal.close()


@pytest_asyncio.fixture
async def terminal(terminal_factory: TerminalFactory) -> Terminal:
    return await terminal_factory()
