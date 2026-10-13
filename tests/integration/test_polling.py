"""Readiness waits preserve observations and report failures without waiting out dead owners."""

import os
import sys
from pathlib import Path
from types import SimpleNamespace

import pytest

from tests.support import polling
from tests.support.probe import Probe


@pytest.fixture
def clock(monkeypatch):
    class Clock:
        elapsed = 0

        def __init__(self):
            self.sleeps = []

        def monotonic(self):
            return self.elapsed

        def sleep(self, duration):
            self.sleeps.append(duration)
            self.elapsed += duration

    clock = Clock()
    monkeypatch.setattr(polling, "time", clock)
    return clock


def test_wait_returns_the_observation_without_collecting_diagnostics(clock):
    observation = {"ready": True}
    attempts = iter([None, False, observation])

    def unexpected_diagnostics():
        pytest.fail("Successful readiness must not collect failure diagnostics")

    assert (
        polling.wait_until(
            lambda: next(attempts),
            timeout=1,
            message="fixture not ready",
            interval=0.1,
            diagnostics=unexpected_diagnostics,
        )
        is observation
    )
    assert clock.sleeps == [0.1, 0.1]


def test_timeout_bounds_the_final_sleep_and_collects_current_diagnostics(clock):
    attempts = []

    def ready():
        attempts.append(clock.monotonic())
        return False

    with pytest.raises(RuntimeError, match="fixture not ready.*0.25 seconds") as failure:
        polling.wait_until(
            ready,
            timeout=0.25,
            interval=0.1,
            message="fixture not ready",
            diagnostics=lambda: f"attempts: {len(attempts)}",
            error=RuntimeError,
        )
    assert clock.elapsed == 0.25
    assert clock.sleeps == pytest.approx([0.1, 0.1, 0.05])
    assert "attempts: 4" in str(failure.value)


def test_slow_probe_does_not_add_another_sleep_after_the_deadline(clock):
    def ready():
        clock.elapsed += 2
        return False

    with pytest.raises(AssertionError, match="fixture not ready"):
        polling.wait_until(ready, timeout=1, message="fixture not ready")
    assert not clock.sleeps


def test_dead_process_fails_before_the_readiness_probe(clock):
    process = SimpleNamespace(poll=lambda: 23)

    def unexpected_probe():
        pytest.fail("An exited server must not satisfy readiness from a stale socket")

    with pytest.raises(RuntimeError, match="mux exited with status 23") as failure:
        polling.wait_until(
            unexpected_probe,
            timeout=15,
            message="mux not ready",
            check_live=lambda: polling.check_process(process, "mux", error=RuntimeError),
            diagnostics=lambda: "mux.log: configuration rejected",
        )
    assert failure.value.__notes__ == ["mux.log: configuration rejected"]
    assert not clock.sleeps


def test_probe_errors_propagate_without_retry(clock):
    error = RuntimeError("probe failed")

    def failed():
        raise error

    with pytest.raises(RuntimeError, match="probe failed") as failure:
        polling.wait_until(failed, timeout=1, message="fixture not ready")
    assert failure.value is error
    assert not clock.sleeps


def test_failed_diagnostics_do_not_mask_the_readiness_failure(clock):
    def unavailable():
        raise OSError("log disappeared")

    with pytest.raises(AssertionError, match="fixture not ready") as failure:
        polling.wait_until(
            lambda: False,
            timeout=0,
            message="fixture not ready",
            diagnostics=unavailable,
        )
    assert "log disappeared" in str(failure.value)


def test_probe_returns_a_matching_empty_state_even_after_gui_exit(tmp_path):
    probe = Probe.__new__(Probe)
    probe.root, probe.window, probe.latest = tmp_path, None, {}
    probe.read = lambda: None
    probe.gui_process = SimpleNamespace(poll=lambda: 0)
    assert probe.wait(lambda state: state == {}) == {}


@pytest.mark.skipif(os.name == "nt", reason="Unix mux fixture")
def test_unix_probe_reports_an_exited_mux_without_waiting_for_its_socket(tmp_path, monkeypatch):
    display = SimpleNamespace(env={"DISPLAY": ":fixture"}, assert_live=lambda: None)
    probe = Probe(tmp_path / "probe", Path(sys.executable), None, "unix", display=display)
    server = probe.gui.with_name("wezterm-mux-server")
    server.write_text(
        f"#!{sys.executable}\nprint('invalid fixture config', flush=True)\nraise SystemExit(23)\n"
    )
    server.chmod(0o755)
    start_process = probe.start_process

    def start_and_exit(*args, **kwargs):
        process = start_process(*args, **kwargs)
        process.wait(timeout=5)
        return process

    def unexpected_sleep(duration):
        pytest.fail("Startup must fail immediately after the owned mux exits")

    monkeypatch.setattr(probe, "start_process", start_and_exit)
    monkeypatch.setattr(
        polling, "time", SimpleNamespace(monotonic=lambda: 0, sleep=unexpected_sleep)
    )
    try:
        with pytest.raises(AssertionError, match="owned Unix mux exited with status 23") as failure:
            probe.start()
        assert "invalid fixture config" in "\n".join(failure.value.__notes__)
        assert "mux.log" in "\n".join(failure.value.__notes__)
    finally:
        probe.close()
