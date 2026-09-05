from __future__ import annotations

import asyncio
import itertools
import json
from pathlib import Path

import pytest

from tests.support.process import run_process

pytestmark = pytest.mark.rust
PROFILE = {"kind": "profile", "profile": "test"}


def key(entity: str, field: str = "name", scope: dict | None = None) -> dict:
    return {"scope": PROFILE if scope is None else scope, "entity": entity, "field": field}


def put(item: dict, value, revision: int = 0) -> dict:
    return {"op": "put", "key": item, "value": value, "expected_revision": revision}


@pytest.fixture
def request_store(rust_binaries: dict[str, Path], tmp_path: Path, isolated_env: dict[str, str]):
    ids = itertools.count(1)

    async def request(operations: list[dict], *, private: bool = False):
        request_id = next(ids)
        result = await run_process(
            [rust_binaries["wez-vtabs-store"], "--db", tmp_path / "state.sqlite"],
            env=isolated_env,
            stdin=json.dumps(
                {
                    "version": 1,
                    "request_id": request_id,
                    "private": private,
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


async def test_folder_catalog_survives_independent_helper_processes(request_store):
    folder = key("folder:tools")
    written = await request_store([put(folder, "Tools")])
    restored = await request_store([{"op": "read", "scope": PROFILE}])
    assert "error" not in written
    assert restored["records"] == written["records"]
    assert restored["records"][0]["value"] == "Tools"


async def test_conflict_rolls_back_every_operation_in_the_request(request_store):
    folder = key("folder:tools")
    await request_store([put(folder, "Original")])
    failed = await request_store([put(key("space:work"), "Work"), put(folder, "Stale", 0)])
    assert failed["error"]["code"] == "conflict"
    restored = await request_store([{"op": "read", "scope": PROFILE}])
    assert len(restored["records"]) == 1
    assert restored["records"][0]["value"] == "Original"


async def test_concurrent_helpers_preserve_independent_fields(request_store):
    await request_store([{"op": "read", "scope": PROFILE}])
    writes = await asyncio.gather(
        request_store([put(key("folder:first"), "First")]),
        request_store([put(key("folder:second"), "Second")]),
    )
    assert all("error" not in result for result in writes)
    restored = await request_store([{"op": "read", "scope": PROFILE}])
    assert {record["value"] for record in restored["records"]} == {"First", "Second"}


async def test_session_membership_never_crosses_mux_incarnations(request_store):
    session = {"kind": "session", "profile": "test", "incarnation": "current-mux"}
    previous = {**session, "incarnation": "previous-mux"}
    membership = {"space": "home", "manual": True, "pinned": True, "folder": "tools"}
    await request_store([put(key("tab:7", "membership", session), membership)])
    restored = await request_store([{"op": "read", "scope": previous}])
    assert restored["records"] == []
    restored = await request_store([{"op": "read", "scope": session}])
    assert restored["records"][0]["value"] == membership


async def test_private_writes_leave_the_public_catalog_unchanged(request_store):
    rejected = await request_store([put(key("folder:private"), "Private")], private=True)
    assert rejected["error"]["code"] == "private_write"
    restored = await request_store([{"op": "read", "scope": PROFILE}])
    assert restored["records"] == []


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
    ("payload", "error_code"),
    [
        ("{", "invalid_request"),
        (json.dumps({"version": 99, "request_id": 1, "operations": []}), "invalid_request"),
        (" " * (1024 * 1024 + 1), "limit"),
    ],
    ids=["malformed-json", "unsupported-protocol", "oversized-request"],
)
async def test_invalid_input_returns_bounded_machine_readable_errors(
    rust_binaries: dict[str, Path],
    tmp_path: Path,
    isolated_env: dict[str, str],
    payload,
    error_code,
):
    result = await run_process(
        [rust_binaries["wez-vtabs-store"], "--db", tmp_path / "state.sqlite"],
        env=isolated_env,
        stdin=payload,
    )
    assert result.returncode == 1
    assert len(result.stdout) < 4096
    assert json.loads(result.stdout)["error"]["code"] == error_code
