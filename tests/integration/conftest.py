"""Explicit binaries and private display ownership for integration tests."""

import os
from contextlib import ExitStack
from pathlib import Path

import pytest

from tests.support.headless import HeadlessDisplay
from tests.support.probe import Probe


@pytest.fixture(scope="session")
def wezterm_binaries(pytestconfig):
    directory = pytestconfig.getoption("--wezterm-bin-dir") or os.environ.get("VTABS_TEST_BIN")
    if not directory:
        pytest.fail("tests require --wezterm-bin-dir or VTABS_TEST_BIN; they never build WezTerm")
    directory = Path(directory).resolve()
    suffix = ".exe" if os.name == "nt" else ""
    binaries = {
        name: directory / (name + suffix)
        for name in (
            "wezterm-gui",
            "wezterm",
            "wezterm-mux-server",
            "wez-vtabs-store",
            "strip-ansi-escapes",
        )
    }
    missing = [str(path) for path in binaries.values() if not path.is_file()]
    if missing:
        pytest.fail("missing prebuilt binaries: " + ", ".join(missing))
    return binaries


@pytest.fixture
def headless_display(tmp_path):
    with HeadlessDisplay(tmp_path / "display") as display:
        yield display


@pytest.fixture
def probe_factory(wezterm_binaries, headless_display, tmp_path):
    probes = []

    def create(name, domain="local", **options):
        probe = Probe(
            tmp_path / name,
            wezterm_binaries["wezterm-gui"],
            wezterm_binaries["wez-vtabs-store"],
            domain,
            server=wezterm_binaries["wezterm-mux-server"],
            display=headless_display,
            **options,
        )
        probes.append(probe)
        return probe

    yield create
    with ExitStack() as cleanup:
        for probe in probes:
            cleanup.callback(probe.close)
