from pathlib import Path

import pytest

from .harness import LuaHarness

PLUGIN_TESTS = Path(__file__).resolve().parent


def pytest_collection_modifyitems(items: list[pytest.Item]) -> None:
    for item in items:
        if Path(str(item.path)).resolve().is_relative_to(PLUGIN_TESTS):
            item.add_marker("plugin")


@pytest.fixture
def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


@pytest.fixture
def lua(repo_root: Path, tmp_path: Path) -> LuaHarness:
    return LuaHarness(repo_root=repo_root, scratch=tmp_path)
