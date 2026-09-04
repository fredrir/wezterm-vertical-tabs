from __future__ import annotations

import os
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path(__file__).parent / "fixtures"
CONFIG = Path(__file__).parent / ".luarc.json"

pytestmark = pytest.mark.typecheck


@dataclass(frozen=True)
class LuaLsResult:
    returncode: int
    output: str


def _check(fixture: str, tmp_path: Path) -> LuaLsResult:
    configured = os.environ.get("LUA_LANGUAGE_SERVER", "lua-language-server")
    server = shutil.which(configured)
    if server is None:
        pytest.fail(f"Lua language server not found: {configured}")

    environment = os.environ.copy()
    environment.update({"LANG": "C.UTF-8", "LC_ALL": "C.UTF-8"})
    completed = subprocess.run(
        [
            server,
            f"--check={FIXTURES / fixture}",
            f"--configpath={CONFIG}",
            "--check_format=pretty",
            "--checklevel=Warning",
            "--locale=en-us",
            f"--logpath={tmp_path / 'log'}",
        ],
        cwd=ROOT,
        env=environment,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    return LuaLsResult(completed.returncode, completed.stdout + completed.stderr)


def test_public_types_accept_valid_configuration(tmp_path: Path) -> None:
    result = _check("valid", tmp_path)

    assert result.returncode == 0, result.output


@pytest.mark.parametrize(
    ("fixture", "diagnostic", "field"),
    [
        pytest.param("invalid-enum", "assign-type-mismatch", "top", id="enum"),
        pytest.param("invalid-field", "inject-field", "positon", id="top-level-field"),
        pytest.param("invalid-nested-field", "inject-field", "pth", id="nested-field"),
    ],
)
def test_public_types_reject_invalid_configuration(
    tmp_path: Path,
    fixture: str,
    diagnostic: str,
    field: str,
) -> None:
    result = _check(fixture, tmp_path)

    assert f"({diagnostic})" in result.output, result.output
    assert field in result.output, result.output
