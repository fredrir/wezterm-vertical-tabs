from __future__ import annotations

import asyncio
import os
import signal
import subprocess
from collections.abc import Mapping, Sequence
from pathlib import Path
from weakref import WeakSet

_stopped_processes: WeakSet[subprocess.Popen] = WeakSet()


def stop_process(process: subprocess.Popen, *, timeout_seconds: float = 5) -> None:
    """Reap an owned process and its session, including children of an exited leader."""
    if process in _stopped_processes:
        return
    if os.name == "nt":
        if process.poll() is None:
            process.terminate()
    else:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    try:
        process.wait(timeout=timeout_seconds)
    except subprocess.TimeoutExpired:
        if os.name == "nt":
            process.kill()
    finally:
        if os.name != "nt":
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        process.wait(timeout=timeout_seconds)
    _stopped_processes.add(process)


async def run_process(
    command: Sequence[str | Path],
    *,
    env: Mapping[str, str],
    cwd: Path | None = None,
    stdin: str | None = None,
    timeout_seconds: float = 10,
) -> subprocess.CompletedProcess[str]:
    arguments = [str(argument) for argument in command]
    process = await asyncio.create_subprocess_exec(
        *arguments,
        cwd=cwd,
        env=env,
        stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
        start_new_session=os.name != "nt",
    )
    try:
        async with asyncio.timeout(timeout_seconds):
            output, error = await process.communicate(None if stdin is None else stdin.encode())
    except BaseException:
        if process.returncode is None:
            if os.name == "nt":
                process.kill()
            else:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            await process.wait()
        raise
    return subprocess.CompletedProcess(
        arguments, process.returncode, output.decode("utf-8"), error.decode("utf-8")
    )
