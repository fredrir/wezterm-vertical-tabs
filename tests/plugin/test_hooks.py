import json


def _control_record(line: str) -> tuple[str, str, object]:
    prefix, token, payload = line.rstrip("\n").split(" ", 2)
    return prefix, token, json.loads(payload)


def test_theme_boundaries_normalize_typed_values_and_preserve_raw_policy(lua) -> None:
    result = lua.run(
        """
        local theme = require "vtabs.theme_bridge"

        local typed = theme.overrides {
          bg = "#AbCdEf",
          accent = { 0, 17, 255 },
          elevation = "0.25",
          scrim = 2,
          hover_bg = { 1, -1, 3 },
          unknown = "kept nowhere",
        }
        local raw = theme.raw_overrides {
          accent = { 1, 2, 255 },
          bg = false,
          scrim = 2,
          unknown = { enabled = true, label = "rust validates this" },
          callback = function() end,
        }
        return { typed = typed, raw = raw }
        """
    )

    assert result == {
        "typed": {
            "accent": "#0011ff",
            "bg": "#abcdef",
            "elevation": 0.25,
        },
        "raw": {
            "accent": "#0102ff",
            "bg": False,
            "scrim": 2,
            "unknown": {"enabled": True, "label": "rust validates this"},
        },
    }


def test_wire_encoder_lowers_hostile_lua_values_without_poisoning_the_message(lua) -> None:
    encoded = lua.run(
        """
        local wire = require "vtabs.wire"
        local cycle = {}
        cycle.self = cycle
        local sparse = { [1] = "first", [3] = "third" }
        local mixed = { "first", named = true }

        return wire.encode {
          cycle = cycle,
          empty = wire.array(),
          infinite = math.huge,
          mixed = mixed,
          nan = 0 / 0,
          negative_infinite = -math.huge,
          sparse = sparse,
        }
        """
    )

    assert encoded == (
        '{"cycle":{"self":null},"empty":[],"infinite":null,"mixed":null,'
        '"nan":null,"negative_infinite":null,"sparse":null}'
    )
    assert json.loads(encoded) == {
        "cycle": {"self": None},
        "empty": [],
        "infinite": None,
        "mixed": None,
        "nan": None,
        "negative_infinite": None,
        "sparse": None,
    }


def test_malformed_theme_hook_request_is_inert(lua) -> None:
    result = lua.run(
        """
        local config = require "vtabs.config"
        local theme = require "vtabs.theme_bridge"
        local calls, pane_touches = 0, 0

        config.setup {
          hooks = {
            theme = function()
              calls = calls + 1
              return { accent = "#ffffff" }
            end,
          },
        }
        local pane = setmetatable({}, {
          __index = function()
            pane_touches = pane_touches + 1
          end,
        })
        local accepted = theme.answer_hook({}, pane, {})
        return { accepted = accepted, calls = calls, pane_touches = pane_touches }
        """
    )

    assert result == {"accepted": False, "calls": 0, "pane_touches": 0}


def test_theme_hook_success_and_failures_always_resume_the_pending_commit(lua) -> None:
    result = lua.run(
        """
        local config = require "vtabs.config"
        local state = require "vtabs.state"
        local theme = require "vtabs.theme_bridge"
        local calls, received_windows, lines = 0, {}, {}
        local window = { identity = "gui-window" }
        local pane = {}

        function pane:pane_id()
          return 41
        end
        function pane:send_text(text)
          lines[#lines + 1] = text
        end

        config.setup {
          hooks = {
            theme = function(received_window, base)
              calls = calls + 1
              received_windows[#received_windows + 1] = received_window.identity
              if base.case == "error" then
                error "user theme failed"
              end
              if base.case == "invalid" then
                return "not an override table"
              end
              return { accent = base.accent, elevation = 0.2, ignored = true }
            end,
          },
        }
        state.set_token(pane:pane_id(), "hook-token")

        local success = theme.answer_hook(window, pane, {
          theme = { case = "success", accent = { 4, 5, 6 } },
        })
        local failed = theme.answer_hook(window, pane, { theme = { case = "error" } })
        local invalid = theme.answer_hook(window, pane, { theme = { case = "invalid" } })
        return {
          calls = calls,
          results = { success, failed, invalid },
          received_windows = received_windows,
          lines = lines,
        }
        """
    )

    records = [_control_record(line) for line in result["lines"]]
    assert result["calls"] == 3
    assert result["results"] == [True, True, True]
    assert result["received_windows"] == ["gui-window", "gui-window", "gui-window"]
    assert [(prefix, token) for prefix, token, _ in records] == [
        ("\x1eVTABS", "hook-token"),
        ("\x1eVTABS", "hook-token"),
        ("\x1eVTABS", "hook-token"),
    ]
    assert [message for _, _, message in records] == [
        {
            "t": "theme_hook_result",
            "overrides": {"accent": "#040506", "elevation": 0.2},
        },
        {"t": "theme_hook_result", "overrides": {}},
        {"t": "theme_hook_result", "overrides": {}},
    ]


def test_route_hook_failure_is_isolated_to_its_tab(lua) -> None:
    result = lua.run(
        """
        local config = require "vtabs.config"
        local spaces = require "vtabs.spaces"
        local state = require "vtabs.state"
        local calls, lines = 0, {}
        local window, pane = {}, {}

        function window:window_id()
          return 9
        end
        function pane:pane_id()
          return 42
        end
        function pane:send_text(text)
          lines[#lines + 1] = text
        end

        config.setup {
          hooks = {
            route = function(facts)
              calls = calls + 1
              if facts.tab_id == 2 then
                error "one tab cannot be routed"
              end
              if facts.tab_id == 3 then
                return false
              end
              return "work"
            end,
          },
        }
        state.set_token(pane:pane_id(), "route-token")
        local rejected = spaces.answer_hook(window, pane, {
          window_id = 10,
          tabs = { { tab_id = 99, title = "wrong window" } },
        })
        local accepted = spaces.answer_hook(window, pane, {
          window_id = 9,
          tabs = {
            { tab_id = 1, title = "one" },
            { tab_id = 2, title = "two" },
            { tab_id = 3, title = "three" },
          },
        })
        return { rejected = rejected, accepted = accepted, calls = calls, lines = lines }
        """
    )

    prefix, token, message = _control_record(result["lines"][0])
    assert result["rejected"] is False
    assert result["accepted"] is True
    assert result["calls"] == 3
    assert len(result["lines"]) == 1
    assert (prefix, token) == ("\x1eVTABS", "route-token")
    assert message == {
        "t": "space_route_hook_result",
        "routes": [
            {"space": "work", "tab_id": 1},
            {"tab_id": 2},
            {"tab_id": 3},
        ],
    }


def test_split_spawn_command_api_receives_content_pane_when_sidebar_is_active(lua) -> None:
    result = lua.run(
        """
        local actions = require "vtabs.actions"
        local content, sidebar, created, tab, mux_window, gui = {}, {}, {}, {}, {}, {}
        local request, callback_pane_id, callback_window_id, activations = nil, nil, nil, 0

        function content:pane_id() return 11 end
        function content:get_title() return "shell" end
        function content:get_domain_name() return "local" end
        function content:tab() return tab end
        function content:split(value)
          request = value
          return created
        end

        function sidebar:pane_id() return 12 end
        function sidebar:get_title() return "wez-vtabs:abc123" end
        function sidebar:get_domain_name() return "local" end
        function sidebar:tab() return tab end

        function created:activate()
          activations = activations + 1
        end
        function created:pane_id() return 13 end
        function tab:tab_id() return 7 end
        function tab:panes() return { content, sidebar } end
        function tab:active_pane() return sidebar end
        function mux_window:active_tab() return tab end
        function gui:mux_window() return mux_window end
        function gui:window_id() return 5 end

        local returned = actions.split(gui, "Down", function(pane, window)
          callback_pane_id = pane:pane_id()
          callback_window_id = window:window_id()
          return { args = { "sh", "-lc", "printf ready" }, cwd = "/workspace" }
        end)
        return {
          callback_pane_id = callback_pane_id,
          callback_window_id = callback_window_id,
          request = request,
          activations = activations,
          returned_pane_id = returned and returned:pane_id(),
        }
        """
    )

    assert result == {
        "callback_pane_id": 11,
        "callback_window_id": 5,
        "request": {
            "args": ["sh", "-lc", "printf ready"],
            "cwd": "/workspace",
            "direction": "Bottom",
        },
        "activations": 1,
        "returned_pane_id": 13,
    }
