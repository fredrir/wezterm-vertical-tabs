import json

import pytest

from .harness import LuaHarness


def test_persisted_state_contains_only_public_restorable_facts(lua: LuaHarness) -> None:
    lua.run(
        """
        local state = require "vtabs.state"

        state.set_sidebar(31, 91, "capability-secret")
        state.set_private(7, true)
        state.set_collapsed(7, true)
        state.set_focus(7, true)
        state.set_active_space(7, "private-window-space")
        state.set_pinned(31, true)
        state.set_space(31, "work", true)
        state.push_closed {
          cwd = "/tmp/project",
          domain = "local",
          title = "documentation",
          space = "work",
          space_manual = true,
        }
        return {}
        """
    )

    state_file = lua.scratch / "state" / "wez-vtabs" / "state.json"
    body = state_file.read_text()
    assert json.loads(body) == {
        "closed": [
            {
                "cwd": "/tmp/project",
                "domain": "local",
                "space": "work",
                "space_manual": True,
                "title": "documentation",
            }
        ],
        "pinned": {"31": True},
        "space_manual": {"31": True},
        "space_of": {"31": "work"},
    }
    assert "capability-secret" not in body

    restored = lua.run(
        """
        local state = require "vtabs.state"
        local before = {
          pending = state.pins_pending(),
          pinned = state.is_pinned(31),
        }
        state.restore_pins()
        return {
          before = before,
          after = {
            pending = state.pins_pending(),
            pinned = state.is_pinned(31),
            space = state.space_of(31),
            space_manual = state.space_manual(31),
            closed = state.pop_closed(),
          },
        }
        """
    )

    assert restored == {
        "before": {"pending": True, "pinned": False},
        "after": {
            "pending": False,
            "pinned": True,
            "space": "work",
            "space_manual": True,
            "closed": {
                "cwd": "/tmp/project",
                "domain": "local",
                "space": "work",
                "space_manual": True,
                "title": "documentation",
            },
        },
    }


@pytest.mark.parametrize(
    ("body", "warning"),
    [
        pytest.param("not json", "state file unreadable", id="corrupt"),
        pytest.param(
            '{"closed":[],"pinned":{"31":true},"tokens":{"91":"injected-secret"}}',
            "incompatible shape",
            id="incompatible-shape",
        ),
    ],
)
def test_invalid_persisted_state_starts_empty(
    lua: LuaHarness,
    body: str,
    warning: str,
) -> None:
    state_file = lua.scratch / "state" / "wez-vtabs" / "state.json"
    state_file.parent.mkdir(parents=True)
    state_file.write_text(body)

    observed = lua.run(
        """
        local state = require "vtabs.state"
        local wezterm = require "wezterm"
        return {
          pending = state.pins_pending(),
          pinned = state.is_pinned(31),
          closed = state.pop_closed(),
          token = state.token_for(91),
          sidebar = state.sidebar_pane_id(31),
          log = wezterm.log,
        }
        """
    )

    assert observed["pending"] is False
    assert observed["pinned"] is False
    assert "closed" not in observed
    assert "token" not in observed
    assert "sidebar" not in observed
    assert any(warning in message for message in observed["log"])


def test_sidebar_identity_rejects_a_title_or_session_owned_by_a_live_sibling(
    lua: LuaHarness,
) -> None:
    observed = lua.run(
        """
        local identity = require "vtabs.sidebar_identity"
        local state = require "vtabs.state"

        local tab = {}
        local candidate = { vars = {} }
        local foreign_owner = {}

        function tab:tab_id() return 40 end
        function candidate:pane_id() return 1 end
        function candidate:tab() return tab end
        function candidate:get_title() return "wez-vtabs:deadbeef" end
        function candidate:get_domain_name() return "local" end
        function candidate:get_user_vars() return self.vars end
        function foreign_owner:pane_id() return 2 end

        local panes = { candidate, foreign_owner }
        state.set_sidebar(tab:tab_id(), candidate:pane_id(), "candidate-session")
        state.set_token(foreign_owner:pane_id(), "foreign-session")

        local marker = identity.has_marker(candidate)
        local title_only = identity.is_ready(candidate, panes)
        candidate.vars.vtabs_token = "unknown-session"
        local wrong = identity.is_ready(candidate, panes)
        candidate.vars.vtabs_token = "foreign-session"
        local foreign = identity.is_ready(candidate, panes)
        candidate.vars.vtabs_token = "candidate-session"
        local own = identity.is_ready(candidate, panes)

        return {
          marker = marker,
          mapped_pane = state.sidebar_pane_id(tab:tab_id()),
          ready = {
            title_only = title_only,
            unknown = wrong,
            live_sibling = foreign,
            own = own,
          },
        }
        """
    )

    assert observed == {
        "marker": True,
        "mapped_pane": 1,
        "ready": {
            "title_only": False,
            "unknown": False,
            "live_sibling": False,
            "own": True,
        },
    }
