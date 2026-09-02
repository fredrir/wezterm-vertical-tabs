local config = require "vtabs.config"
local mux = require "vtabs.mux"
local sidebar = require "vtabs.sidebar"
local util = require "vtabs.util"

local M = {}

local function pane_id(pane)
  return mux.call(pane, "pane_id")
end

local function panes_from(tab)
  local infos = mux.panes_with_info(tab) or {}
  local collections = 1
  local panes = {}
  for _, info in ipairs(infos) do
    if info.pane then
      panes[#panes + 1] = info.pane
    end
  end
  -- Older mux domains may not expose pane layout records. Preserve their panes, while keeping the
  -- normal path to one enumeration that also supplies geometry.
  if #panes == 0 then
    panes = mux.panes(tab) or {}
    collections = collections + 1
  end
  return panes, infos, collections
end

local function facts_for(pane)
  return {
    pane = pane,
    id = pane_id(pane),
    title = sidebar.title(pane),
    dimensions = mux.dims(pane),
    domain = mux.domain(pane),
    foreground = mux.foreground(pane),
    cwd = mux.cwd(pane),
    unseen = mux.unseen(pane) == true,
    user_vars = mux.user_vars(pane) or {},
  }
end

---Takes one immutable observation of the mux tree for a publish. Consumers may derive values from
---the referenced handles, but must not enumerate the window or its tabs again during this pass.
function M.capture(gui_window, opts)
  opts = opts or {}
  local mux_window = mux.call(gui_window, "mux_window")
  local wid = mux.window_id(gui_window)
  local cfg = opts.cfg or config.get()
  local effective = opts.effective or mux.effective_config(gui_window) or {}
  local observed = {
    gui_window = gui_window,
    mux_window = mux_window,
    window_id = wid,
    cfg = cfg,
    now = opts.now or util.now_ms(),
    effective = effective,
    palette = effective.resolved_palette or {},
    window_dims = mux.dims(gui_window) or {},
    tabs = {},
    tabs_by_id = {},
    panes = {},
    stats = { mux_collections = 1, pane_collections = 0 },
  }
  observed.metrics = observed.window_dims

  for _, info in ipairs(mux.tabs_with_info(mux_window) or {}) do
    local tab = info.tab
    local tab_id = mux.tab_id(tab)
    if tab and tab_id ~= nil then
      local panes, pane_infos, collections = panes_from(tab)
      observed.stats.mux_collections = observed.stats.mux_collections + collections
      observed.stats.pane_collections = observed.stats.pane_collections + collections
      local active_pane = nil
      for _, pane_info in ipairs(pane_infos) do
        if pane_info.is_active and pane_info.pane then
          active_pane = pane_info.pane
          break
        end
      end
      active_pane = active_pane or mux.active_pane(tab)
      local content, painting = sidebar.classify(tab, panes)
      local entry = {
        info = info,
        tab = tab,
        tab_id = tab_id,
        index = info.index,
        is_active = info.is_active == true,
        title = mux.tab_title(tab),
        panes = panes,
        pane_infos = pane_infos,
        active_pane = active_pane,
        content = content,
        sidebar = painting,
        content_pane = sidebar.content_pane(tab, panes, active_pane),
      }
      entry.sidebar_ready = painting ~= nil and sidebar.is_ready(painting, panes) or false
      for _, pane in ipairs(panes) do
        local facts = facts_for(pane)
        if facts.id ~= nil then
          observed.panes[facts.id] = facts
        end
        if sidebar.is_settings(pane) then
          observed.settings = { tab = tab, tab_id = tab_id, pane = pane, entry = entry }
        end
      end
      observed.tabs[#observed.tabs + 1] = entry
      observed.tabs_by_id[tab_id] = entry
      if entry.is_active then
        observed.active = entry
      end
    end
  end
  if not observed.active then
    local active = mux.active_tab(mux_window)
    observed.active = active and observed.tabs_by_id[mux.tab_id(active)] or nil
  end
  observed.active_tab = observed.active and observed.active.tab or nil
  observed.active_tab_id = observed.active and observed.active.tab_id or nil
  return observed
end

return M
