from pathlib import Path

from .harness import LuaHarness


def test_backend_path_accepts_strings_lists_and_ignores_empty_entries(lua: LuaHarness) -> None:
    result = lua.run(
        """
        local backend = require "vtabs.backend"
        local config = require "vtabs.config"

        local function candidates(path)
          local cfg = config.setup { backend = { path = path } }
          return backend.candidates(cfg, "local", nil)
        end

        local plain = candidates "/opt/wez-vtabs"
        local listed = candidates { "/first", "", "/second", "/first" }
        local absent = candidates(nil)
        local empty = candidates ""
        return {
          plain = table.concat(plain, "|"),
          listed = table.concat(listed, "|"),
          absent_count = #absent,
          empty_count = #empty,
        }
        """
    )

    assert result == {
        "plain": "/opt/wez-vtabs",
        "listed": "/first|/second",
        "absent_count": 0,
        "empty_count": 0,
    }


def test_backend_path_prioritizes_host_then_domain_and_deduplicates(lua: LuaHarness) -> None:
    result = lua.run(
        """
        local backend = require "vtabs.backend"
        local config = require "vtabs.config"
        local cfg = config.setup {
          backend = {
            path = {
              "/fallback",
              archie = "/host",
              ["archie-tls"] = { "/domain", "/host" },
              z = "/tail",
            },
          },
        }
        return {
          candidates = table.concat(backend.candidates(cfg, "archie-tls", "archie"), "|"),
          names_host = backend.names(cfg, "archie-tls", "archie"),
          names_unknown = backend.names(cfg, "other", "nobody"),
        }
        """
    )

    assert result == {
        "candidates": "/host|/domain|/fallback|/tail",
        "names_host": True,
        "names_unknown": False,
    }


def test_backend_path_callback_contains_failures_and_distinguishes_nil_from_empty(
    lua: LuaHarness,
) -> None:
    result = lua.run(
        """
        local backend = require "vtabs.backend"
        local config = require "vtabs.config"
        local cfg = config.setup {
          backend = {
            path = function(domain, host)
              if host == "archie" then
                return { "/host", "", "/shared" }
              elseif domain == "empty" then
                return ""
              elseif domain == "broken" then
                error "callback failed"
              end
              return nil
            end,
          },
        }
        local function inspect(domain, host)
          local found = backend.candidates(cfg, domain, host)
          return {
            count = #found,
            joined = table.concat(found, "|"),
            named = backend.names(cfg, domain, host),
          }
        end
        return {
          host = inspect("localmux", "archie"),
          absent = inspect("other", nil),
          empty = inspect("empty", nil),
          broken = inspect("broken", nil),
        }
        """
    )

    assert result == {
        "host": {"count": 2, "joined": "/host|/shared", "named": True},
        "absent": {"count": 0, "joined": "", "named": False},
        "empty": {"count": 0, "joined": "", "named": True},
        "broken": {"count": 0, "joined": "", "named": False},
    }


def test_backend_launch_contract_carries_candidates_role_and_machine_hints(
    lua: LuaHarness, tmp_path: Path
) -> None:
    result = lua.run(
        """
        local backend = require "vtabs.backend"
        local config = require "vtabs.config"
        local version = require "vtabs.version"
        backend.root = "plugin"
        backend.register_local_domains { unix_domains = { { name = "localmux" } } }

        local cfg = config.setup {
          backend = { path = { "/first", "/second" } },
        }
        local without_path = config.setup { backend = { path = nil } }
        return {
          spawn = backend.spawn_args("archie-tls", "settings"),
          env = backend.env(cfg, "localmux", nil, "#112233"),
          absent_env = backend.env(without_path, "local", nil),
          plugin_version = version,
          machine_domains = {
            local_domain = backend.machine_domain "local",
            unix_domain = backend.machine_domain "localmux",
            remote_domain = backend.machine_domain "archie-tls",
          },
          plain_path_names_remote = backend.names(cfg, "archie-tls", "archie"),
        }
        """
    )

    assert result["spawn"][:2] == ["sh", "-c"]
    assert "VTABS_BIN" in result["spawn"][2]
    assert result["spawn"][3:] == ["wez-vtabs", "--role", "settings"]
    backend_env = result["env"].copy()
    assert backend_env.pop("VTABS_INBOX_ROOT") == str(tmp_path / "runtime" / "wez-vtabs")
    assert backend_env == {
        "VTABS_BG": "#112233",
        "VTABS_BIN": "/first\n/second",
        "VTABS_BUILD": "1",
        "VTABS_REPO": "fredrir/wezterm-vertical-tabs",
        "VTABS_SRC": "plugin/../backend",
        "VTABS_TARGET": "x86_64-unknown-linux-gnu",
        "VTABS_USERVAR": "vtabs",
        "VTABS_VERSION": result["plugin_version"],
    }
    assert "VTABS_BIN" not in result["absent_env"]
    assert result["machine_domains"] == {
        "local_domain": True,
        "unix_domain": True,
        "remote_domain": False,
    }
    assert result["plain_path_names_remote"] is False


def test_unknown_keys_are_pruned_with_paths_in_diagnostics(lua: LuaHarness) -> None:
    result = lua.run(
        """
        local wezterm = require "wezterm"
        local config = require "vtabs.config"
        local cfg = config.setup {
          widht = 31,
          backend = { path = "/bin/wez-vtabs", pth = "/wrong" },
        }
        return {
          unknown = { widht = cfg.widht, backend_pth = cfg.backend.pth },
          warnings = wezterm.log,
        }
        """
    )

    warnings = "\n".join(result["warnings"])
    assert result["unknown"] == {}
    assert "unknown option widht" in warnings, warnings
    assert "unknown option backend.pth" in warnings, warnings


def test_retired_values_and_action_names_are_not_silently_translated(lua: LuaHarness) -> None:
    result = lua.run(
        """
        local wezterm = require "wezterm"
        local config = require "vtabs.config"
        local actions = require "vtabs.actions"
        local cfg = config.setup {
          tab_height = true,
          meta = true,
          new_tab_button = true,
          backend = { path = "/bin/wez-vtabs" },
        }
        return {
          retired = {
            tab_height = cfg.tab_height,
            meta = cfg.meta,
            new_tab_button = cfg.new_tab_button,
          },
          action_types = {
            toggle = type(actions.resolve "toggle"),
            settings = type(actions.resolve "settings"),
          },
          warnings = wezterm.log,
        }
        """
    )

    assert result["retired"] == {
        "tab_height": "card",
        "meta": False,
        "new_tab_button": "ghost",
    }
    assert result["action_types"] == {"toggle": "nil", "settings": "nil"}
    warnings = "\n".join(result["warnings"])
    for option in ("tab_height", "meta", "new_tab_button"):
        assert option in warnings, warnings


def test_numeric_configuration_accepts_exact_limits_and_rejects_neighbours(
    lua: LuaHarness,
) -> None:
    result = lua.run(
        """
        local config = require "vtabs.config"
        local function setup(opts)
          opts.backend = { path = "/bin/wez-vtabs" }
          return config.setup(opts)
        end
        return {
          width_min = setup { width = 8 }.width,
          width_below = setup { width = 7 }.width,
          width_fraction = setup { width = 8.5 }.width,
          width_non_finite = setup { width = math.huge }.width,
          poll_min = setup { poll_ms = 50 }.poll_ms,
          poll_below = setup { poll_ms = 49 }.poll_ms,
          fade_max = setup { popover = { fade_ms = 400 } }.popover.fade_ms,
          fade_above = setup { popover = { fade_ms = 401 } }.popover.fade_ms,
          elevation_min = setup { theme = { elevation = 0 } }.theme.elevation,
          elevation_max = setup { theme = { elevation = 0.3 } }.theme.elevation,
          elevation_above = setup { theme = { elevation = 0.3001 } }.theme.elevation,
        }
        """
    )

    assert result == {
        "width_min": 8,
        "width_below": 28,
        "width_fraction": 28,
        "width_non_finite": 28,
        "poll_min": 50,
        "poll_below": 500,
        "fade_max": 400,
        "fade_above": 90,
        "elevation_min": 0,
        "elevation_max": 0.3,
        "elevation_above": 0.06,
    }


def test_modifier_alias_leaves_the_users_binding_in_control(lua: LuaHarness) -> None:
    result = lua.run(
        """
        local wezterm = require "wezterm"
        wezterm.target_triple = "aarch64-apple-darwin"
        local keys = require "vtabs.keys"
        local own = { owner = "user" }
        local host = { keys = { { key = "t", mods = "SUPER", action = own } } }
        keys.apply(host, { keys = {} })

        local observed = {}
        for _, binding in ipairs(host.keys) do
          if binding.key == "t" then
            observed[#observed + 1] = {
              mods = binding.mods,
              user_tag = binding.action.owner,
            }
          end
        end
        return observed
        """
    )

    assert result == [
        {"mods": "SUPER", "user_tag": "user"},
        {"mods": "CMD|SHIFT"},
    ]
