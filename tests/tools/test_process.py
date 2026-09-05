"""Cancellation owns the whole command group, including inherited output pipes."""

import json
import os
import signal
import subprocess
import sys
import time

import pytest

pytestmark = [pytest.mark.rust, pytest.mark.skipif(os.name == "nt", reason="POSIX fork fixture")]


def test_cancellation_terminates_descendant_after_command_leader_exits(tools_sandbox, tmp_path):
    commands = tmp_path / "fixture-git"
    commands.mkdir()
    child_pid = tmp_path / "descendant.pid"
    git = commands / "git"
    git.write_text(
        f"#!{sys.executable}\nimport os, pathlib, time\n"
        "if os.fork(): os._exit(0)\n"
        f"pathlib.Path({str(child_pid)!r}).write_text(str(os.getpid()))\n"
        "time.sleep(60)\n"
    )
    git.chmod(0o755)
    tools_sandbox.env["PATH"] = str(commands) + os.pathsep + tools_sandbox.env["PATH"]
    log = tmp_path / "process.log"
    with log.open("w") as output:
        process = subprocess.Popen(
            [
                str(tools_sandbox.binary),
                "--project-root",
                str(tools_sandbox.root),
                "--cache",
                str(tools_sandbox.cache),
                "--install-root",
                str(tools_sandbox.install),
                "prepare",
            ],
            cwd=tools_sandbox.root,
            env=tools_sandbox.env,
            stdout=output,
            stderr=output,
            start_new_session=True,
        )
        try:
            deadline = time.monotonic() + 5
            while not child_pid.exists() and time.monotonic() < deadline:
                time.sleep(0.02)
            assert child_pid.exists(), log.read_text()
            process.send_signal(signal.SIGINT)
            try:
                status = process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pytest.fail("Cancellation blocked on output pipes inherited by a descendant")
            assert status == 130, log.read_text()
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            if child_pid.exists():
                try:
                    os.kill(int(child_pid.read_text()), signal.SIGKILL)
                except ProcessLookupError:
                    pass
    report = json.loads(next((tools_sandbox.cache / "runs").glob("*/run.json")).read_text())
    assert report["status"] == "failed"
    assert "cancelled" in report["error"]
    assert report["commands"][-1]["error"]
