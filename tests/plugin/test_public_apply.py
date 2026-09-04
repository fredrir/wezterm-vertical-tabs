import pytest

from .harness import LuaHarness


def test_apply_to_config_projects_documented_boot_defaults(lua: LuaHarness) -> None:
    result = lua.run(
        """
        local plugin = require "init"
        local host = {}
        plugin.apply_to_config(host, {
          settings = false,
          backend = { path = "/missing/wez-vtabs" },
          poll_ms = 175,
          keys = false,
        })
        return host
        """
    )

    assert result["enable_tab_bar"] is False
    assert result["status_update_interval"] == 175
    assert result["window_padding"] == {
        "left": 0,
        "right": "1cell",
        "top": "0.5cell",
        "bottom": "0.5cell",
    }
    assert result["pane_focus_follows_mouse"] is True
    assert result["inactive_pane_hsb"] == {"brightness": 1, "saturation": 1, "hue": 1}
    assert result["skip_close_confirmation_for_processes_named"].count("wez-vtabs") == 1


def test_repeated_apply_preserves_caller_fields_and_does_not_duplicate_entries(
    lua: LuaHarness,
) -> None:
    result = lua.run(
        """
        local plugin = require "init"
        local caller_padding = { left = 9, right = 8, top = 7, bottom = 6 }
        local caller_hsb = { brightness = 0.4, saturation = 0.5, hue = 0.6 }
        local caller_action = { owner = "caller" }
        local processes = { "fish" }
        local host = {
          enable_tab_bar = true,
          status_update_interval = 300,
          window_padding = caller_padding,
          pane_focus_follows_mouse = false,
          inactive_pane_hsb = caller_hsb,
          colors = { split = "#123456", caller_value = "kept" },
          skip_close_confirmation_for_processes_named = processes,
          keys = { { key = "t", mods = "CTRL|SHIFT", action = caller_action } },
          caller_value = "kept",
        }
        local opts = {
          settings = false,
          backend = { path = "/missing/wez-vtabs" },
          hide_native_tab_bar = false,
          poll_ms = 600,
          keys = { toggle_sidebar = { key = "q", mods = "ALT" } },
        }

        plugin.apply_to_config(host, opts)
        plugin.apply_to_config(host, opts)
        local key_fields = {}
        for _, binding in ipairs(host.keys) do
          key_fields[#key_fields + 1] = {
            key = binding.key,
            mods = binding.mods,
            owner = binding.action.owner,
          }
        end
        host.keys = key_fields
        return host
        """
    )

    assert result["enable_tab_bar"] is True
    assert result["status_update_interval"] == 300
    assert result["pane_focus_follows_mouse"] is False
    assert result["window_padding"] == {"left": 9, "right": 8, "top": 7, "bottom": 6}
    assert result["inactive_pane_hsb"] == {
        "brightness": 0.4,
        "saturation": 0.5,
        "hue": 0.6,
    }
    assert result["colors"] == {"split": "#123456", "caller_value": "kept"}
    assert result["caller_value"] == "kept"
    assert result["skip_close_confirmation_for_processes_named"].count("wez-vtabs") == 1

    caller_chords = [
        binding
        for binding in result["keys"]
        if binding["key"] == "t" and binding["mods"] == "CTRL|SHIFT"
    ]
    assert caller_chords == [{"key": "t", "mods": "CTRL|SHIFT", "owner": "caller"}]
    assert (
        sum(binding["key"] == "q" and binding["mods"] == "ALT" for binding in result["keys"]) == 1
    )


@pytest.mark.parametrize(
    ("decorations", "options", "expected"),
    [
        pytest.param("nil", "{}", "INTEGRATED_BUTTONS|RESIZE", id="unset-left"),
        pytest.param('"RESIZE"', "{}", "INTEGRATED_BUTTONS|RESIZE", id="resize-left"),
        pytest.param('"RESIZE"', '{ titlebar = "plain" }', "RESIZE", id="plain-titlebar"),
        pytest.param('"RESIZE"', '{ position = "right" }', "RESIZE", id="right-sidebar"),
        pytest.param('"TITLE|RESIZE"', "{}", "TITLE|RESIZE", id="caller-owned"),
    ],
)
def test_apply_to_config_only_integrates_macos_buttons_when_the_sidebar_reserves_them(
    lua: LuaHarness,
    decorations: str,
    options: str,
    expected: str,
) -> None:
    result = lua.run(
        f"""
        local wezterm = require "wezterm"
        wezterm.target_triple = "aarch64-apple-darwin"
        local plugin = require "init"
        local host = {{ window_decorations = {decorations} }}
        local options = {options}
        options.settings = false
        options.backend = {{ path = "/missing/wez-vtabs" }}
        options.keys = false
        plugin.apply_to_config(host, options)
        return {{ window_decorations = host.window_decorations }}
        """
    )

    assert result["window_decorations"] == expected
