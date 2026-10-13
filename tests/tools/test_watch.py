"""Watch mode keeps the current application usable throughout rebuilds."""

import json
import os
import signal
import subprocess
import sys
import time

import pytest

from tests.support.polling import log_diagnostics, wait_until
from tests.tools.conftest import write_file

pytestmark = [pytest.mark.rust, pytest.mark.skipif(os.name == "nt", reason="POSIX process fixture")]


def wait_for_watch(predicate, log, timeout=20):
    wait_until(
        predicate,
        timeout=timeout,
        message="Watch condition did not become ready",
        diagnostics=lambda: log_diagnostics(log),
        error=pytest.fail.Exception,
    )


def alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    return True


def test_watch_restages_resources_and_retains_current_gui_when_compilation_fails(
    tools_sandbox, local_upstream, recording_cargo, tmp_path, rust_host
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
    rustc = tmp_path / "fixture-rustc"
    rustc.write_text(f"#!{sys.executable}\nprint('rustc 1.99.0 (fixture)\\nhost: {rust_host}')\n")
    rustc.chmod(0o755)
    tools_sandbox.env["RUSTC"] = str(rustc)

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
            wait_for_watch(lambda: len(started()) == 1, log)
            first_pid = started()[0]["pid"]
            first_builds = len(builds())
            assert first_builds == 2

            write_file(tools_sandbox.root, "plugin/init.lua", "return { changed = true }\n")
            wait_for_watch(lambda: len(started()) == 2, log)
            current_pid = started()[1]["pid"]
            assert not alive(first_pid)
            assert alive(current_pid)
            assert len(builds()) == first_builds

            for path in ("src/app/src/.save", "plugin/init.lua~", "src/app/src/lib.rs.swp"):
                write_file(tools_sandbox.root, path, "editor temporary file\n")
            time.sleep(0.5)
            assert len(started()) == 2
            assert len(builds()) == first_builds

            # Configuration additions, updates, and removals all require Cargo.
            for path, contents in (
                (".cargo/config.toml", "[build]\nincremental = true\n"),
                (".cargo/config.toml", "[build]\nincremental = false\n"),
                (".cargo/config.toml", None),
                ("rust-toolchain", "fixture\n"),
                ("rust-toolchain", None),
                ("rust-toolchain.toml", '[toolchain]\nchannel = "fixture"\n'),
                ("rust-toolchain.toml", None),
            ):
                before_starts, before_builds = len(started()), len(builds())
                if contents is None:
                    (tools_sandbox.root / path).unlink()
                else:
                    write_file(tools_sandbox.root, path, contents)
                wait_for_watch(lambda expected=before_starts + 1: len(started()) == expected, log)
                assert not alive(current_pid)
                current_pid = started()[-1]["pid"]
                assert alive(current_pid)
                assert len(builds()) == before_builds + 2

            failure = recording_cargo.parent / "fail-build"
            failure.touch()
            write_file(tools_sandbox.root, "src/app/src/lib.rs", "pub fn broken() {}\n")
            wait_for_watch(lambda: "fixture watch compiler failure" in log.read_text(), log)
            assert alive(current_pid)
            before_starts = len(started())

            failure.unlink()
            write_file(tools_sandbox.root, "src/app/src/lib.rs", "pub fn repaired() {}\n")
            wait_for_watch(lambda: len(started()) == before_starts + 1, log)
            assert not alive(current_pid)
            assert alive(started()[-1]["pid"])
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
