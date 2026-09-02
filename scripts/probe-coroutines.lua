-- Proves what vtabs/gate.lua relies on: every emission runs on its own Lua thread, sleep_ms yields
-- to the others, and a thread keeps its identity across its own awaits. Runs, logs, quits.
--   HOME=$(mktemp -d) WEZTERM_LOG=info wezterm --config-file scripts/probe-coroutines.lua start --always-new-process
local wezterm = require "wezterm"
local config = wezterm.config_builder()
config.default_prog = { "/bin/sh" }
config.window_close_confirmation = "NeverPrompt"
config.exit_behavior = "Close"

local function id()
  local co, ismain = coroutine.running()
  return string.format("%s main=%s yieldable=%s", tostring(co), tostring(ismain), tostring(coroutine.isyieldable()))
end

local function log(s)
  wezterm.log_info("probe " .. s .. " " .. id())
end

local function chain_a(window, pane, n)
  wezterm.time.call_after(0.3, function()
    log("A#" .. n)
    if n == 2 then
      log "split before"
      pcall(function()
        return pane:split { direction = "Right", size = 0.3 }
      end)
      log "split after"
    end
    if n == 3 then
      log "sleep start"
      wezterm.sleep_ms(1500)
      log "sleep end"
    end
    if n == 5 then
      window:perform_action(
        wezterm.action_callback(function()
          log "callback"
        end),
        pane
      )
    end
    if n < 10 then
      chain_a(window, pane, n + 1)
    else
      log "done"
      window:perform_action(wezterm.action.QuitApplication, pane)
    end
  end)
end

local function chain_b(n)
  wezterm.time.call_after(0.3, function()
    log("B#" .. n)
    if n < 14 then
      chain_b(n + 1)
    end
  end)
end

local started = false
wezterm.on("update-status", function(window, pane)
  if started then
    return
  end
  started = true
  log "status"
  chain_a(window, pane, 1)
  chain_b(1)
end)

return config
