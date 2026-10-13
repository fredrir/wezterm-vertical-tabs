from __future__ import annotations

import asyncio
import itertools
import json
from pathlib import Path

import pytest

from tests.support.process import run_process

pytestmark = pytest.mark.rust
PROFILE = {"kind": "profile", "profile": "test"}


def key(entity: str, field: str = "name") -> dict:
    return {
        "scope": PROFILE,
        "entity": entity,
        "field": field,
    }


def put(item: dict, value) -> dict:
    return {"op": "put", "key": item, "value": value, "expected_revision": 0}


@pytest.fixture
def request_store(rust_binaries: dict[str, Path], tmp_path: Path, isolated_env: dict[str, str]):
    ids = itertools.count(1)

    async def request(operations: list[dict]):
        request_id = next(ids)
        result = await run_process(
            [rust_binaries["wez-vtabs-store"], "--db", tmp_path / "state.sqlite"],
            env=isolated_env,
            stdin=json.dumps(
                {
                    "version": 1,
                    "request_id": request_id,
                    "operations": operations,
                }
            ),
        )
        response = json.loads(result.stdout)
        assert response["version"] == 1
        assert response["request_id"] == request_id
        assert result.stderr == ""
        assert result.returncode == (1 if "error" in response else 0)
        return response

    return request


async def test_large_response_is_flushed_and_survives_independent_helper_processes(request_store):
    folder = key("folder:tools")
    value = '"\\\n-é🙂' * 2048
    written = await request_store([put(folder, value)])
    restored = await request_store([{"op": "read", "scope": PROFILE}])
    assert "error" not in written
    assert written["revision"] == 1
    assert len(json.dumps(written)) > 8192
    assert restored["records"] == written["records"]
    assert restored["records"][0]["value"] == value


async def test_concurrent_helpers_preserve_independent_fields(request_store):
    await request_store([{"op": "read", "scope": PROFILE}])
    writes = await asyncio.gather(
        request_store([put(key("folder:first"), "First")]),
        request_store([put(key("folder:second"), "Second")]),
    )
    assert all("error" not in result for result in writes)
    restored = await request_store([{"op": "read", "scope": PROFILE}])
    assert {record["value"] for record in restored["records"]} == {"First", "Second"}


async def test_null_preference_and_deleted_preference_remain_distinct(request_store):
    preference = key("settings", "default_domain")
    written = await request_store([put(preference, None)])
    assert written["records"][0]["value"] is None
    revision = written["records"][0]["revision"]
    deleted = await request_store(
        [{"op": "delete", "key": preference, "expected_revision": revision}]
    )
    assert "value" not in deleted["records"][0]
    restored = await request_store([{"op": "read", "scope": PROFILE}])
    assert "value" not in restored["records"][0]
    assert restored["records"][0]["revision"] > revision


@pytest.mark.parametrize(
    ("payload", "error_code", "request_id"),
    [
        ("{", "invalid_request", 0),
        (
            json.dumps({"version": 99, "request_id": 1, "operations": []}),
            "invalid_request",
            1,
        ),
        (" " * (1024 * 1024 + 1), "limit", 0),
    ],
    ids=["malformed-json", "unsupported-protocol", "oversized-request"],
)
async def test_invalid_input_returns_bounded_machine_readable_errors(
    rust_binaries: dict[str, Path],
    tmp_path: Path,
    isolated_env: dict[str, str],
    payload,
    error_code,
    request_id,
):
    result = await run_process(
        [rust_binaries["wez-vtabs-store"], "--db", tmp_path / "state.sqlite"],
        env=isolated_env,
        stdin=payload,
    )
    assert result.returncode == 1
    assert len(result.stdout) < 4096
    response = json.loads(result.stdout)
    assert response["version"] == 1
    assert response["request_id"] == request_id
    assert response["error"]["code"] == error_code
