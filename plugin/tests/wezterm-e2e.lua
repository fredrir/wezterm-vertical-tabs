-- Standalone config for driving the plugin in a throwaway WezTerm instance.
local wezterm = require "wezterm"
local root = os.getenv "VTABS_ROOT"
package.path = root .. "/plugin/?.lua;" .. root .. "/plugin/?/init.lua;" .. package.path

local vtabs = dofile(root .. "/plugin/init.lua")
local config = wezterm.config_builder()

config.default_prog = { "/bin/sh" }
config.initial_cols = 100
config.initial_rows = 30
config.window_close_confirmation = "NeverPrompt"
config.exit_behavior = "Close"

local mux_socket = os.getenv "VTABS_E2E_MUX"
if mux_socket then
  config.unix_domains = { { name = "e2emux", socket_path = mux_socket } }
  config.default_domain = "e2emux"
end

config.unix_domains = config.unix_domains or {}

local function sidebar_cols(window)
  local out = {}
  for _, info in ipairs(window:mux_window():tabs_with_info()) do
    for _, pane in ipairs(info.tab:panes()) do
      local title = pcall(pane.get_title, pane) and pane:get_title() or ""
      if title:match "^wez%-vtabs:" then
        out[#out + 1] = tostring(pane:get_dimensions().cols)
      end
    end
  end
  return table.concat(out, ",")
end

-- Registered before the plugin, so it reports the width WezTerm dealt out before geometry corrects it.
wezterm.on("window-resized", function(window)
  wezterm.log_info("e2e: sidebar cols on resize " .. sidebar_cols(window))
end)

-- The traffic-light reserve and the rail's own strip geometry are keyed off the target triple,
-- so they can only be exercised anywhere else by lying to `platform` about the platform.
-- `titlebar = "macos"` claims the reserve for the strip; `platform.is_mac` is what the titlebar
-- band reads, and `integrated_title_button_style = "MacOsNative"` is rejected off macOS.
if os.getenv "VTABS_E2E_MACOS" then
  require("vtabs.platform").is_mac = true
  config.window_decorations = "INTEGRATED_BUTTONS|RESIZE"
end

vtabs.apply_to_config(config, {
  titlebar = os.getenv "VTABS_E2E_MACOS" and "macos" or nil,
  poll_ms = 200,
  debug = true,
  confirm_close = false,
  domain = os.getenv "VTABS_E2E_DOMAIN" or "CurrentPaneDomain",
  backend = { path = os.getenv "VTABS_BIN" },
  icons = false,
  collapsed = os.getenv "VTABS_E2E_COLLAPSED" or nil,
})

local probes = {
  toggle = function(window)
    vtabs.toggle_sidebar(window)
  end,
  toggle_ms = function(window)
    local util = require "vtabs.util"
    local before = util.now_ms()
    vtabs.toggle_sidebar(window)
    wezterm.log_info("e2e: toggle ms " .. tostring(util.now_ms() - before))
  end,
  attach_ms = function(window)
    local util = require "vtabs.util"
    local sidebar = require "vtabs.sidebar"
    local n, worst = 0, 0
    for _, info in ipairs(window:mux_window():tabs_with_info()) do
      if not sidebar.find(info.tab) then
        local before = util.now_ms()
        sidebar.attach(info.tab)
        local took = util.now_ms() - before
        n, worst = n + 1, math.max(worst, took)
      end
    end
    wezterm.log_info(string.format("e2e: attach n %d worst %d", n, worst))
  end,
  grow = function(window)
    local dims = window:get_dimensions()
    window:set_inner_size(dims.pixel_width + 300, dims.pixel_height)
  end,
  shrink = function(window)
    local dims = window:get_dimensions()
    window:set_inner_size(dims.pixel_width - 300, dims.pixel_height)
  end,
  -- A drag: ten resizes 100 ms apart, the way `window-resized` really arrives.
  drag_shrink = function(window)
    local n, step = 0, nil
    step = function()
      n = n + 1
      local ok = pcall(function()
        local d = window:get_dimensions()
        window:set_inner_size(d.pixel_width - 30, d.pixel_height)
      end)
      if ok and n < 10 then
        wezterm.time.call_after(0.1, step)
      end
    end
    step()
  end,
  drag_grow = function(window)
    local n, step = 0, nil
    step = function()
      n = n + 1
      local ok = pcall(function()
        local d = window:get_dimensions()
        window:set_inner_size(d.pixel_width + 30, d.pixel_height)
      end)
      if ok and n < 10 then
        wezterm.time.call_after(0.1, step)
      end
    end
    step()
  end,
  new_tab = function(window)
    require("vtabs.actions").new_tab(window)
  end,
  tear_off = function(window)
    require("vtabs.actions").tear_off(window, window:mux_window():active_tab():tab_id())
  end,
  -- `actions.new_tab` attaches after an awaiting `spawn_tab`; a poll landing in that await has
  -- already attached and cleared `session.attaching`, so the second call sees exactly this state.
  double_attach = function(window)
    local sidebar = require "vtabs.sidebar"
    local tab = window:mux_window():active_tab()
    local before = sidebar.find(tab)
    local again = sidebar.attach(tab)
    wezterm.log_info(
      "e2e: double attach had "
        .. tostring(before and before:pane_id())
        .. " got "
        .. tostring(again and again:pane_id())
    )
  end,
  -- Two polls landing inside one mux lag: the second reads the same stale `cols` as the first.
  double_correct = function(window)
    local geometry = require "vtabs.geometry"
    local a = geometry.correct(window)
    local b = geometry.correct(window)
    wezterm.log_info("e2e: double correct " .. tostring(a) .. " " .. tostring(b))
  end,
  reload = function()
    wezterm.reload_configuration()
  end,
  rail_mode = function()
    require("vtabs.config").get().collapsed = "rail"
  end,
  popover_level = function(window)
    local pop = require("vtabs.popover").get(window:window_id())
    wezterm.log_info("e2e: popover level " .. (pop and (tostring(pop.level) .. ":" .. tostring(pop.confirm)) or "none"))
  end,
  confirm_on = function()
    require("vtabs.config").get().confirm_close = true
  end,
  confirm_off = function()
    require("vtabs.config").get().confirm_close = false
  end,
  -- Whether the active tab would ask before closing, and every input to that answer.
  probe_confirm = function(window)
    local actions = require "vtabs.actions"
    local sidebar = require "vtabs.sidebar"
    local util = require "vtabs.util"
    local cfg = require("vtabs.config").get()
    local tab = window:mux_window():active_tab()
    local content = sidebar.classify(tab)
    local procs = {}
    for _, p in ipairs(content) do
      procs[#procs + 1] = tostring(util.basename(util.try(function()
        return p:get_foreground_process_name()
      end)))
    end
    wezterm.log_info(
      string.format(
        "e2e: confirm cfg %s needs %s procs %s skip %s",
        tostring(cfg.confirm_close),
        tostring(actions.needs_confirm(window, tab:tab_id(), "close")),
        table.concat(procs, ","),
        table.concat(util.try(function()
          return window:effective_config().skip_close_confirmation_for_processes_named
        end) or {}, ",")
      )
    )
  end,
  footer_hook = function()
    require("vtabs.config").get().hooks.footer = function()
      return { { id = "e2e_footer", text = "e2e footer" } }
    end
  end,
  no_footer_hook = function()
    require("vtabs.config").get().hooks.footer = nil
  end,
  private_window = function(window)
    require("vtabs.actions").new_window(window, true)
  end,
  -- The traffic-light reserve the active sidebar's own dimensions imply, next to the pane it got.
  probe_reserve = function(window)
    local platform = require "vtabs.platform"
    local sidebar = require "vtabs.sidebar"
    local cfg = require("vtabs.config").get()
    local sb = sidebar.find(window:mux_window():active_tab())
    local d = sb and sb:get_dimensions()
    local g = d
      and platform.strip_geometry(d, {
        is_mac = true,
        integrated_buttons = true,
        native_button_style = true,
        position = cfg.position,
        padding_top = cfg.padding.top,
        toggle_button = cfg.toggle_button,
        card_x1 = cfg.padding.left + 1,
      })
    wezterm.log_info(
      string.format(
        "e2e: reserve %s toggle_x %s pane %s",
        tostring(g and g.cols),
        tostring(g and g.toggle_x),
        tostring(d and d.cols)
      )
    )
  end,
  -- The hit map is the only source for the columns a click has to land on; labels move, spans do not.
  probe_hits = function(window)
    local state = require "vtabs.state"
    local sidebar = require "vtabs.sidebar"
    local sb = sidebar.find(window:mux_window():active_tab())
    local out = {}
    for row, h in pairs(sb and state.session.hits[sb:pane_id()] or {}) do
      local parts = { string.format("%s/%s/%d/%s-%s", h.kind, tostring(h.id), row, tostring(h.x1), tostring(h.x2)) }
      for _, span in ipairs(h.spans or {}) do
        parts[#parts + 1] = string.format("%s@%d-%d", tostring(span.id), span.x1, span.x2)
      end
      out[#out + 1] = table.concat(parts, ",")
    end
    table.sort(out)
    wezterm.log_info("e2e: hits " .. table.concat(out, " "))
  end,
  hidden_mode = function()
    require("vtabs.config").get().collapsed = "hidden"
  end,
  -- geometry's caches are module-locals; upvalues are the only way to trace them from outside.
  probe_geom = function(window)
    local geometry = require "vtabs.geometry"
    local wid = window:window_id()
    local out = {}
    for i = 1, 40 do
      local name, value = debug.getupvalue(geometry.correct, i)
      if not name then
        break
      end
      if type(value) == "table" and value[wid] ~= nil then
        local v = value[wid]
        if type(v) == "table" then
          local parts = {}
          for _, k in ipairs { "tab_id", "cols", "tab_cols", "target", "stuck", "collapsed", "px" } do
            if v[k] ~= nil then
              parts[#parts + 1] = k .. "=" .. tostring(v[k])
            end
          end
          out[#out + 1] = name .. "{" .. table.concat(parts, ",") .. "}"
        else
          out[#out + 1] = name .. "=" .. tostring(v)
        end
      end
    end
    wezterm.log_info("e2e: geom " .. tostring(geometry.desired(wid)) .. " | " .. table.concat(out, " "))
  end,
  -- One line per tab: the sidebar panes the plugin itself sees, so a duplicate is visible plugin-side.
  probe_panes = function(window)
    local sidebar = require "vtabs.sidebar"
    local out = {}
    for _, info in ipairs(window:mux_window():tabs_with_info()) do
      local marked, total = 0, 0
      for _, p in ipairs(info.tab:panes()) do
        total = total + 1
        if sidebar.has_marker(p) then
          marked = marked + 1
        end
      end
      local sb = sidebar.find(info.tab)
      out[#out + 1] = string.format("%d:%d/%d/%s", info.tab:tab_id(), marked, total, tostring(sb and sb:pane_id()))
    end
    wezterm.log_info("e2e: panes " .. table.concat(out, " "))
  end,
  probe_desired = function(window)
    local geometry = require "vtabs.geometry"
    local state = require "vtabs.state"
    wezterm.log_info(
      "e2e: desired width "
        .. tostring(geometry.desired(window:window_id()))
        .. " collapsed "
        .. tostring(state.is_collapsed(window:window_id()))
    )
  end,
  -- A tab overlay (the tab menu) replaces the tab's panes, so this reports the overlay's pane id.
  probe_active_title = function(window)
    wezterm.log_info("e2e: active title " .. tostring(window:active_tab():get_title()))
  end,
  probe_active = function(window)
    local pane = window:active_pane()
    wezterm.log_info("e2e: active pane " .. tostring(pane and pane:pane_id()))
  end,
}

-- Test-only hook: a pane printing SetUserVar=vtabs_test=<base64 name> triggers a probe.
wezterm.on("user-var-changed", function(window, _, name, value)
  local probe = name == "vtabs_test" and probes[value]
  if not probe then
    return
  end
  local ok, err = pcall(probe, window)
  if not ok then
    wezterm.log_error("e2e: probe " .. value .. " failed: " .. tostring(err))
  end
end)

return config
