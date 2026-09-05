from __future__ import annotations

import json
from pathlib import Path

import pytest

from tests.support.process import run_process

pytestmark = pytest.mark.rust


async def test_public_schema_describes_every_setting_default(
    rust_binaries: dict[str, Path], isolated_env: dict[str, str]
):
    result = await run_process([rust_binaries["gen-schema"], "json"], env=isolated_env)
    assert result.returncode == 0
    schema = json.loads(result.stdout)
    assert {option["key"] for option in schema["options"]} == set(schema["defaults"])
    assert schema["defaults"]["width"] > 0
    assert schema["defaults"]["default_domain"] is None


@pytest.mark.parametrize("format_name", ["lua", "types", "markdown"])
async def test_generated_files_accept_platform_line_endings_and_reject_content_changes(
    rust_binaries: dict[str, Path], tmp_path: Path, isolated_env: dict[str, str], format_name: str
):
    binary = rust_binaries["gen-schema"]
    path = tmp_path / f"generated.{format_name}"
    written = await run_process([binary, "--write", format_name, path], env=isolated_env)
    assert written.returncode == 0
    path.write_bytes(path.read_bytes().replace(b"\n", b"\r\n"))
    checked = await run_process([binary, "--check", format_name, path], env=isolated_env)
    assert checked.returncode == 0
    path.write_bytes(path.read_bytes() + b"unexpected content\r\n")
    checked = await run_process([binary, "--check", format_name, path], env=isolated_env)
    assert checked.returncode == 1
    assert "is stale" in checked.stderr
