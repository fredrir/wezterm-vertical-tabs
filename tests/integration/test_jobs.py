"""Zsh job control through the same OSC snapshots and ZLE input used by the GUI."""

import base64
import os
import re
import select
import shlex
import shutil
import signal
import sys
import time

import pytest

pytestmark = [pytest.mark.pty, pytest.mark.skipif(os.name == "nt", reason="Zsh PTY")]
SNAPSHOT = re.compile(rb"\x1b\]1337;SetUserVar=vtabs_jobs=([A-Za-z0-9+/=\r\n]*)\x07")


class Shell:
    def __init__(self, pid, fd):
        self.pid, self.fd = pid, fd
        self.output = b""
        self.pending = b""
        self.last = None
        self.jobs = set()

    def send(self, value):
        os.write(self.fd, value.encode())

    def snapshot(self, predicate=lambda value: value["ready"], timeout=5):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for match in list(SNAPSHOT.finditer(self.pending)):
                lines = base64.b64decode(match[1]).decode().splitlines()
                version, shell, generation, ready, host = lines[0].split("\t")
                assert version == "1"
                rows = [line.split("\t", 3) for line in lines[1:]]
                value = dict(
                    shell=shell, generation=generation, ready=ready == "1", host=host, rows=rows
                )
                self.jobs.update(int(row[1]) for row in rows)
                self.pending = self.pending[match.end() :]
                self.last = value
                if predicate(value):
                    return value
                break
            else:
                if select.select([self.fd], [], [], max(0, deadline - time.monotonic()))[0]:
                    chunk = os.read(self.fd, 65536)
                    self.output += chunk
                    self.pending += chunk
        pytest.fail(f"Missing jobs snapshot; output: {self.output!r}")

    def request(self, action, row=None, snapshot=None):
        state = snapshot or self.last
        number, pid = (row or ["0", "0"])[:2]
        self.send(f"\x1b[777;{state['shell']};{state['generation']};{number};{pid};{action}~")

    def wait_foreground(self, row):
        deadline = time.monotonic() + 3
        while os.tcgetpgrp(self.fd) != int(row[1]) and time.monotonic() < deadline:
            if select.select([self.fd], [], [], 0.05)[0]:
                chunk = os.read(self.fd, 65536)
                self.output += chunk
                self.pending += chunk
        assert os.tcgetpgrp(self.fd) == int(row[1])


@pytest.fixture
def shell(project_root, isolated_env, tmp_path):
    zsh = shutil.which("zsh")
    if not zsh:
        pytest.skip("zsh unavailable")
    import pty

    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(tmp_path)
        os.execve(
            zsh, [zsh, "-df"], {**isolated_env, "TERM": "xterm-256color", "ZDOTDIR": str(tmp_path)}
        )
    session = Shell(pid, fd)
    try:
        session.send(f"source {shlex.quote(str(project_root / 'plugin/vtabs.zsh'))}\n")
        session.snapshot()
        yield session
    finally:
        for job in session.jobs:
            try:
                os.killpg(job, signal.SIGKILL)
            except ProcessLookupError:
                pass
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        os.waitpid(pid, 0)
        os.close(fd)


def test_foreground_suspend_background_and_terminate_preserve_typed_input(shell):
    shell.send("sleep 60 &\n")
    state = shell.snapshot(lambda value: value["ready"] and len(value["rows"]) == 1)
    row = state["rows"][0]
    assert row[2:] == ["running", "sleep 60"]
    shell.send("print -r -- retained-input")
    shell.request(1, row)
    shell.snapshot(lambda value: not value["ready"])
    shell.wait_foreground(row)
    shell.send("\x1a")
    state = shell.snapshot(lambda value: value["ready"] and value["rows"][0][2] == "suspended")
    shell.request(2, state["rows"][0])
    state = shell.snapshot(lambda value: value["ready"] and value["rows"][0][2] == "running")
    shell.request(3, state["rows"][0])
    shell.snapshot()
    shell.send("\n")
    state = shell.snapshot(lambda value: value["ready"] and not value["rows"])
    assert b"retained-input\r\n" in shell.output
    assert state["shell"] == str(shell.pid)


def test_stale_process_and_prompt_requests_leave_the_job_running(shell):
    shell.send("sleep 60 &\n")
    old = shell.snapshot(lambda value: value["ready"] and len(value["rows"]) == 1)
    row = old["rows"][0]
    shell.send("true\n")
    shell.snapshot(lambda value: value["ready"] and value["generation"] != old["generation"])
    shell.request(3, row, old)
    shell.request(3, [row[0], "1"])
    shell.send("true\n")
    state = shell.snapshot()
    assert state["rows"][0][:3] == row[:3]
    os.kill(int(row[1]), 0)


def test_foreground_job_receives_terminal_input_and_returns_to_prompt(shell):
    reader = shlex.quote('print("received:" + input())')
    shell.send(f"{shlex.quote(sys.executable)} -c {reader} &\n")
    state = shell.snapshot(lambda value: value["ready"] and len(value["rows"]) == 1)
    row = state["rows"][0]
    shell.request(1, row)
    shell.snapshot(lambda value: not value["ready"])
    shell.wait_foreground(row)
    shell.send("job-input\n")
    shell.snapshot(lambda value: value["ready"] and not value["rows"])
    assert b"received:job-input\r\n" in shell.output


def test_pipeline_is_one_job_and_termination_reaches_all_processes(shell):
    shell.send("sleep 60 | cat &\n")
    state = shell.snapshot(lambda value: value["ready"] and len(value["rows"]) == 1)
    row = state["rows"][0]
    assert row[3] == "sleep 60 | cat"
    shell.request(3, row)
    shell.snapshot()
    shell.send("true\n")
    shell.snapshot(lambda value: value["ready"] and not value["rows"])
    with pytest.raises(ProcessLookupError):
        os.killpg(int(row[1]), 0)


def test_refresh_removes_finished_jobs_without_submitting_prompt_input(shell):
    shell.send("sleep 60 &\n")
    state = shell.snapshot(lambda value: value["ready"] and len(value["rows"]) == 1)
    row = state["rows"][0]
    shell.send("print -r -- still-editing")
    os.kill(int(row[1]), signal.SIGTERM)
    # Wait for Zsh to reap the child, then exercise the launcher's refresh request.
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        try:
            os.kill(int(row[1]), 0)
        except ProcessLookupError:
            break
        if select.select([shell.fd], [], [], 0.05)[0]:
            chunk = os.read(shell.fd, 65536)
            shell.output += chunk
            shell.pending += chunk
    shell.request(0)
    refreshed = shell.snapshot(lambda value: value["ready"] and not value["rows"])
    assert refreshed["generation"] == state["generation"]
    shell.send("\n")
    shell.snapshot(lambda value: value["ready"] and value["generation"] != state["generation"])
    assert b"still-editing\r\n" in shell.output


def test_suspended_jobs_can_be_terminated(shell):
    shell.send("sleep 60 &\n")
    state = shell.snapshot(lambda value: value["ready"] and len(value["rows"]) == 1)
    row = state["rows"][0]
    shell.send(f"kill -STOP %{row[0]}\n")
    state = shell.snapshot(lambda value: value["ready"] and value["rows"][0][2] == "suspended")
    shell.request(3, state["rows"][0])
    shell.snapshot()
    shell.send("true\n")
    shell.snapshot(lambda value: value["ready"] and not value["rows"])
    with pytest.raises(ProcessLookupError):
        os.killpg(int(row[1]), 0)


def test_vi_keymap_and_existing_editor_hooks_survive_installation(shell, project_root):
    shell.send("bindkey -v; zle-line-init() { print -n -- HOOK; }; zle -N zle-line-init\n")
    # Reinstall in a fresh nested shell with a pre-existing ZLE hook.
    shell.send("zsh -df\n")
    shell.send(
        "bindkey -v; zle-line-init() { print -n -- HOOK; }; zle -N zle-line-init; "
        f"source {shlex.quote(str(project_root / 'plugin/vtabs.zsh'))}; sleep 60 &\n"
    )
    state = shell.snapshot(
        lambda value: value["ready"] and value["shell"] != str(shell.pid) and value["rows"]
    )
    assert b"HOOK" in shell.output
    shell.request(3, state["rows"][0])
    shell.snapshot()
    shell.send("true\n")
    shell.snapshot(lambda value: value["ready"] and not value["rows"])
