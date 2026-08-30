local wezterm = require "wezterm" ---@type Wezterm
local util = require "vtabs.util"

local M = {}

---The one place a WezTerm handle is touched unguarded. A pane, tab or window the mux has dropped
---throws on every method, and every caller in the plugin wants the same answer: nothing. Reaching
---for a handle outside this module means writing that guard again, which is how the plugin ended up
---with seventy of them.
local function get(obj, method)
  if obj == nil then
    return nil
  end
  return util.try(function()
    return obj[method](obj)
  end)
end

---Escape hatch for the calls that are not zero-argument getters. Returns the callee's own results,
---or nothing at all when the handle is dead -- so a caller that must tell "threw" from "returned
---nil" wants a raw `pcall` instead.
function M.call(obj, method, ...)
  if obj == nil then
    return
  end
  local args = table.pack(...)
  local res = table.pack(pcall(function()
    return obj[method](obj, table.unpack(args, 1, args.n))
  end))
  if not res[1] then
    return
  end
  return table.unpack(res, 2, res.n)
end

function M.tab_id(tab)
  return get(tab, "tab_id")
end

function M.panes(tab)
  return get(tab, "panes")
end

function M.panes_with_info(tab)
  return get(tab, "panes_with_info")
end

function M.active_pane(tab)
  return get(tab, "active_pane")
end

function M.tab_of(pane)
  return get(pane, "tab")
end

function M.title(pane)
  return get(pane, "get_title")
end

---Panes and GUI windows both answer `get_dimensions`, in their own shapes: cells for a pane, device
---pixels for a window.
function M.dims(obj)
  return get(obj, "get_dimensions")
end

function M.domain(pane)
  return get(pane, "get_domain_name")
end

function M.foreground(pane)
  return get(pane, "get_foreground_process_name")
end

function M.cwd(pane)
  return get(pane, "get_current_working_dir")
end

function M.user_vars(pane)
  return get(pane, "get_user_vars")
end

function M.unseen(pane)
  return get(pane, "has_unseen_output")
end

function M.tabs_with_info(win)
  return get(win, "tabs_with_info")
end

function M.active_tab(win)
  return get(win, "active_tab")
end

function M.window_id(win)
  return get(win, "window_id")
end

function M.effective_config(win)
  return get(win, "effective_config")
end

function M.overrides(gui_window)
  return get(gui_window, "get_config_overrides")
end

function M.pane_by_id(pane_id)
  return util.try(function()
    return wezterm.mux.get_pane(pane_id)
  end)
end

function M.all_windows()
  return util.try(function()
    return wezterm.mux.all_windows()
  end)
end

return M
