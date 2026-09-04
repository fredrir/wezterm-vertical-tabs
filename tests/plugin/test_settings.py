import json
import os
import stat
from pathlib import Path

import pytest

SETTINGS_PATH_ENV = "VTABS_TEST_SETTINGS_PATH"


def test_settings_round_trip_through_private_file(lua, tmp_path: Path) -> None:
    path = tmp_path / "settings.json"
    document = {
        "options": {
            "backend": {"env": {"A.B": "one"}},
            "spaces": [{"id": "work", "match": {"cwd": ["/src"]}}],
            "width": 37,
        }
    }

    result = lua.run(
        """
        local settings = require "vtabs.settings"
        local cfg = { settings = { path = os.getenv "VTABS_TEST_SETTINGS_PATH" } }
        local saved = settings.save_body(cfg, os.getenv "VTABS_TEST_SETTINGS_BODY")
        return { saved = saved, loaded = settings.load(cfg) }
        """,
        env={
            SETTINGS_PATH_ENV: str(path),
            "VTABS_TEST_SETTINGS_BODY": json.dumps(document, separators=(",", ":")),
        },
    )

    assert result == {"saved": True, "loaded": document["options"]}
    assert json.loads(path.read_text(encoding="utf-8")) == document
    if os.name != "nt":
        assert stat.S_IMODE(path.stat().st_mode) == 0o600


def test_empty_settings_file_is_ignored(lua, tmp_path: Path) -> None:
    path = tmp_path / "settings.json"
    path.write_bytes(b"")

    loaded = lua.run(
        """
        local settings = require "vtabs.settings"
        local cfg = { settings = { path = os.getenv "VTABS_TEST_SETTINGS_PATH" } }
        return settings.load(cfg)
        """,
        env={SETTINGS_PATH_ENV: str(path)},
    )

    assert loaded is None


def test_corrupt_settings_file_is_ignored(lua, tmp_path: Path) -> None:
    path = tmp_path / "settings.json"
    path.write_text('{"options":', encoding="utf-8")

    loaded = lua.run(
        """
        local settings = require "vtabs.settings"
        return settings.load {
          settings = { path = os.getenv "VTABS_TEST_SETTINGS_PATH" },
        }
        """,
        env={SETTINGS_PATH_ENV: str(path)},
    )

    assert loaded is None


@pytest.mark.parametrize(
    "document",
    [
        [],
        {},
        {"options": 1},
        {"options": {}, "unexpected_envelope_field": True},
    ],
    ids=["array-root", "missing-options", "non-object-options", "extra-envelope-field"],
)
def test_incompatible_settings_envelope_is_ignored(lua, tmp_path: Path, document) -> None:
    path = tmp_path / "settings.json"
    path.write_text(json.dumps(document), encoding="utf-8")

    loaded = lua.run(
        """
        local settings = require "vtabs.settings"
        return settings.load {
          settings = { path = os.getenv "VTABS_TEST_SETTINGS_PATH" },
        }
        """,
        env={SETTINGS_PATH_ENV: str(path)},
    )

    assert loaded is None


def test_safe_path_rejects_relative_and_traversing_paths(lua) -> None:
    result = lua.run(
        r"""
        local settings = require "vtabs.settings"
        local platform = require "vtabs.platform"

        platform.is_windows = false
        local unix = {
          absolute = settings.safe_path "/tmp/settings.json",
          relative = settings.safe_path "tmp/settings.json",
          traversal = settings.safe_path "/tmp/../settings.json",
          dotted_segment = settings.safe_path "/tmp/my..notes/settings.json",
        }

        platform.is_windows = true
        local windows = {
          drive = settings.safe_path [[C:\Users\me\settings.json]],
          forward_slash_drive = settings.safe_path [[C:/Users/me/settings.json]],
          unc = settings.safe_path [[\\server\share\settings.json]],
          relative_drive = settings.safe_path [[C:settings.json]],
          traversal = settings.safe_path [[C:\Users\..\settings.json]],
        }
        return { unix = unix, windows = windows }
        """
    )

    assert result == {
        "unix": {
            "absolute": True,
            "relative": False,
            "traversal": False,
            "dotted_segment": True,
        },
        "windows": {
            "drive": True,
            "forward_slash_drive": True,
            "unc": True,
            "relative_drive": False,
            "traversal": False,
        },
    }


def test_failed_private_write_leaves_destination_unchanged(lua, tmp_path: Path) -> None:
    path = tmp_path / "settings.json"
    path.write_text("original", encoding="utf-8")

    saved = lua.run(
        """
        local settings = require "vtabs.settings"
        local util = require "vtabs.util"
        local real_open = util.private_open
        util.private_open = function()
          return nil
        end
        local saved = settings.save_body(
          { settings = { path = os.getenv "VTABS_TEST_SETTINGS_PATH" } },
          "replacement"
        )
        util.private_open = real_open
        return saved
        """,
        env={SETTINGS_PATH_ENV: str(path)},
    )

    assert saved is False
    assert path.read_text(encoding="utf-8") == "original"


def test_settings_body_limit_is_inclusive_and_preserves_existing_file(lua, tmp_path: Path) -> None:
    path = tmp_path / "settings.json"

    result = lua.run(
        """
        local protocol = require "vtabs.gen.protocol"
        local settings = require "vtabs.settings"
        local cfg = { settings = { path = os.getenv "VTABS_TEST_SETTINGS_PATH" } }
        local exact = string.rep("x", protocol.SETTINGS_BODY_MAX_BYTES)
        local at_limit = settings.save_body(cfg, exact)
        local readable = settings.read_body(cfg)
        local over_limit = settings.save_body(cfg, exact .. "x")
        return {
          limit = protocol.SETTINGS_BODY_MAX_BYTES,
          at_limit = at_limit,
          readable_bytes = readable and #readable or -1,
          over_limit = over_limit,
        }
        """,
        env={SETTINGS_PATH_ENV: str(path)},
    )

    assert result["limit"] > 0
    assert result == {
        "limit": result["limit"],
        "at_limit": True,
        "readable_bytes": result["limit"],
        "over_limit": False,
    }
    assert path.read_bytes() == b"x" * result["limit"]


def test_opaque_lua_values_restore_by_identity(lua) -> None:
    result = lua.run(
        """
        local settings_model = require "vtabs.settings_model"
        local callback = function()
          return "called"
        end
        local cycle = {}
        cycle.self = cycle
        local shared = { callback = callback }
        local original = {
          title = callback,
          backend = { env = cycle },
          keys = shared,
          frame = shared,
        }

        local projected, opaque = settings_model.project(original)
        settings_model.restore(projected, original, opaque)
        projected.backend.env.self.cycle_observation = "through-cycle"
        projected.keys.keys_observation = "through-keys"
        projected.frame.frame_observation = "through-frame"
        return {
          callback_result = projected.title(),
          cycle_observation = original.backend.env.cycle_observation,
          keys_observation = original.frame.keys_observation,
          frame_observation = original.keys.frame_observation,
        }
        """
    )

    assert result == {
        "callback_result": "called",
        "cycle_observation": "through-cycle",
        "keys_observation": "through-keys",
        "frame_observation": "through-frame",
    }
