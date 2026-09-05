"""Explicit native binaries and private display ownership for integration tests."""

import os
from pathlib import Path

import pytest

from tests.support.headless import HeadlessDisplay


@pytest.fixture(scope="session")
def native_binaries(pytestconfig):
    directory = pytestconfig.getoption("--native-bin-dir") or os.environ.get(
        "VTABS_TEST_NATIVE_BIN"
    )
    if not directory:
        pytest.fail(
            "native tests require --native-bin-dir or VTABS_TEST_NATIVE_BIN; they never build WezTerm"
        )
    directory = Path(directory).resolve()
    suffix = ".exe" if os.name == "nt" else ""
    binaries = {
        name: directory / (name + suffix)
        for name in (
            "wezterm-gui",
            "wezterm",
            "wezterm-mux-server",
            "wez-vtabs-store",
        )
    }
    missing = [str(path) for path in binaries.values() if not path.is_file()]
    if missing:
        pytest.fail("missing prebuilt native binaries: " + ", ".join(missing))
    return binaries


@pytest.fixture
def headless_display(tmp_path):
    with HeadlessDisplay(tmp_path / "display") as display:
        yield display
