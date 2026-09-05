"""Watch mode keeps the current application usable throughout rebuilds."""

import json
import os
import signal
import subprocess
import sys
import time

import pytest

from tests.tools.conftest import write_file

pytestmark = [pytest.mark.rust, pytest.mark.skipif(os.name == "nt", reason="POSIX process fixture")]


def wait_until(predicate, log, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.05)
    pytest.fail(f"Watch condition did not become ready:\n{log.read_text()}")


def alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    return True


def test_watch_restages_resources_and_retains_current_gui_when_compilation_fails(
    tools_sandbox, local_upstream, recording_cargo, tmp_path
):
    _, revision = local_upstream
    events = tmp_path / "gui-events.jsonl"
    executable = tmp_path / "external-target/release/wezterm-gui"
    executable.write_text(
        f"#!{sys.executable}\n"
        "import json, os, time\n"
        f"with open({str(events)!r}, 'a') as stream:\n"
        "    stream.write(json.dumps({'pid': os.getpid(), 'bundle': os.environ['WEZ_VTABS_BUNDLE']}) + '\\n')\n"
        "while True: time.sleep(0.05)\n"
    )
    executable.chmod(0o755)

    def started():
        return (
            [json.loads(line) for line in events.read_text().splitlines()]
            if events.exists()
            else []
        )

    def builds():
        return [
            line
            for line in recording_cargo.read_text().splitlines()
            if json.loads(line)[0] == "build"
        ]

    log = tmp_path / "watch.log"
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
                "--upstream",
                revision,
                "dev",
                "--watch",
                "--debounce-ms",
                "50",
            ],
            cwd=tools_sandbox.root,
            env=tools_sandbox.env,
            stdout=output,
            stderr=output,
            start_new_session=True,
        )
        try:
            wait_until(lambda: len(started()) == 1, log)
            first_pid = started()[0]["pid"]
            first_builds = len(builds())
            assert first_builds == 2

            write_file(tools_sandbox.root, "plugin/init.lua", "return { changed = true }\n")
            wait_until(lambda: len(started()) == 2, log)
            current_pid = started()[1]["pid"]
            assert not alive(first_pid)
            assert alive(current_pid)
            assert len(builds()) == first_builds

            failure = recording_cargo.parent / "fail-build"
            failure.touch()
            write_file(tools_sandbox.root, "crates/vtabs-app/src/lib.rs", "pub fn broken() {}\n")
            wait_until(lambda: "fixture watch compiler failure" in log.read_text(), log)
            assert alive(current_pid)
            assert len(started()) == 2

            failure.unlink()
            write_file(tools_sandbox.root, "crates/vtabs-app/src/lib.rs", "pub fn repaired() {}\n")
            wait_until(lambda: len(started()) == 3, log)
            assert not alive(current_pid)
            assert alive(started()[2]["pid"])
            assert len(builds()) > first_builds
            assert not list((tools_sandbox.cache / "bundles").glob("*.zip"))
            assert not list((tools_sandbox.cache / "bundles").glob("*.tar.gz"))
        finally:
            process.send_signal(signal.SIGINT)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
            for event in started():
                if alive(event["pid"]):
                    os.kill(event["pid"], signal.SIGKILL)
    assert process.returncode == 130
