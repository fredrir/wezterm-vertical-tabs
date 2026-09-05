from __future__ import annotations

import asyncio
import os
import signal
import subprocess
from collections.abc import Mapping, Sequence
from pathlib import Path


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
