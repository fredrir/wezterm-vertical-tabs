"""Bounded readiness polling with caller-owned probes and failure diagnostics."""

from __future__ import annotations

import subprocess
import time
from collections.abc import Callable
from pathlib import Path


def wait_until(
    predicate,
    *,
    timeout: float,
    message: str,
    interval: float = 0.05,
    check_live: Callable[[], None] | None = None,
    diagnostics: Callable[[], str] | None = None,
    error: type[BaseException] = AssertionError,
):
    """Return the first truthy result; probes retain their own retry exception policy."""
    if timeout < 0 or interval <= 0:
        raise ValueError("polling requires a nonnegative timeout and positive interval")

    def failure_details():
        try:
            return diagnostics()
        except Exception as failure:
            return f"Could not collect readiness diagnostics: {failure}"

    deadline = time.monotonic() + timeout
    while True:
        try:
            if check_live is not None:
                check_live()
            value = predicate()
        except Exception as failure:
            if diagnostics is not None:
                failure.add_note(failure_details())
            raise
        if value:
            return value
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            details = f"{message} (timed out after {timeout:g} seconds)"
            if diagnostics is not None:
                details += "\n" + failure_details()
            raise error(details)
        time.sleep(min(interval, remaining))


def check_process(
    process: subprocess.Popen,
    name: str,
    *,
    error: type[BaseException] = AssertionError,
) -> None:
    status = process.poll()
    if status is not None:
        raise error(f"{name} exited with status {status}")


def log_diagnostics(path: Path) -> str:
    try:
        return f"{path}:\n{path.read_text(encoding='utf-8', errors='replace')}"
    except OSError as error:
        return f"{path}: {error}"
