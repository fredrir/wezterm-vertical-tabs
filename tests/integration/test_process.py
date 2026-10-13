"""Shared fixture cleanup owns descendants even after their leader exits."""

import os
import select
import subprocess
import sys
from contextlib import ExitStack
from pathlib import Path

import pytest

from tests.support.process import stop_process


@pytest.mark.skipif(os.name == "nt", reason="POSIX process sessions")
@pytest.mark.parametrize("leader_exits", [False, True])
def test_cleanup_stops_a_descendant_that_ignores_termination(leader_exits, monkeypatch):
    child = (
        "import signal, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); "
        "print('ready', flush=True); time.sleep(60)"
    )
    parent = (
        f"import subprocess, sys, time; subprocess.Popen([sys.executable, '-c', {child!r}]); "
        + ("" if leader_exits else "time.sleep(60)")
    )
    process = subprocess.Popen(
        [sys.executable, "-c", parent],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        start_new_session=True,
    )
    try:
        assert select.select([process.stdout], [], [], 5)[0], "child did not start"
        assert process.stdout.readline() == b"ready\n"
        if leader_exits:
            process.wait(timeout=5)
        stop_process(process, timeout_seconds=0.2)
        assert select.select([process.stdout], [], [], 5)[0], "child kept the output pipe open"
        assert process.stdout.read() == b""
        assert process.returncode is not None
        with monkeypatch.context() as patch:

            def unexpected_signal(*args):
                pytest.fail("Repeated cleanup must not signal the completed process group")

            patch.setattr(os, "killpg", unexpected_signal)
            stop_process(process, timeout_seconds=0.2)
    finally:
        try:
            stop_process(process, timeout_seconds=0.2)
        finally:
            process.stdout.close()


@pytest.mark.skipif(os.name == "nt", reason="POSIX process sessions")
def test_cleanup_retries_after_a_signal_failure(monkeypatch):
    process = subprocess.Popen(
        [sys.executable, "-c", "import time; time.sleep(60)"], start_new_session=True
    )
    try:
        with monkeypatch.context() as patch:

            def denied(*args):
                raise PermissionError("signal denied")

            patch.setattr(os, "killpg", denied)
            with pytest.raises(PermissionError, match="signal denied"):
                stop_process(process, timeout_seconds=0.2)
        assert process.poll() is None
        stop_process(process, timeout_seconds=0.2)
        assert process.returncode is not None
    finally:
        stop_process(process, timeout_seconds=0.2)


@pytest.mark.parametrize("kind", ["probe", "headless"])
def test_fixture_cleanup_finishes_other_resources_after_a_process_error(
    kind, tmp_path, monkeypatch
):
    from tests.support import headless, probe

    with ExitStack() as cleanup:
        processes = []
        for _ in range(2):
            process = subprocess.Popen(
                [sys.executable, "-c", "import time; time.sleep(60)"],
                stdout=subprocess.DEVNULL,
                start_new_session=os.name != "nt",
            )
            processes.append(process)
            cleanup.callback(stop_process, process, timeout_seconds=0.2)
        if kind == "probe":
            module = probe
            fixture = probe.Probe(tmp_path / "probe", Path(sys.executable), None, "local")
            fixture.processes = processes
            display = fixture.display
        else:
            module = headless
            fixture = headless.HeadlessDisplay(tmp_path / "display")
            fixture.xserver, fixture.manager = processes
            display = fixture
        display.env["DISPLAY"] = ":fixture"

        def denied(process, **kwargs):
            if process is processes[-1]:
                raise PermissionError("signal denied")
            stop_process(process, timeout_seconds=0.2)

        with monkeypatch.context() as patch:
            patch.setattr(module, "stop_process", denied)
            with pytest.raises(PermissionError, match="signal denied"):
                fixture.close()

        assert processes[0].poll() is not None
        assert processes[1].poll() is None
        assert "DISPLAY" not in display.env
        fixture.close()
        assert all(process.poll() is not None for process in processes)
