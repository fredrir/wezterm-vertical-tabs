local config = require "vtabs.config"
local gate = require "vtabs.gate"
local state = require "vtabs.state"
local store = require "vtabs.store"
local sidebar = require "vtabs.sidebar"
local model = require "vtabs.model"
local actions = require "vtabs.actions"
local keys = require "vtabs.keys"
local spaces = require "vtabs.spaces"
local util = require "vtabs.util"

local M = {}

local MAX_HINT_COLS = 8

local MODS = { CTRL = "^", SHIFT = "⇧", ALT = "⌥", OPT = "⌥", CMD = "⌘", SUPER = "❖" }

local ITEMS = {
  { id = "activate", label = "Switch to tab", action = "activate_tab" },
  { id = "pin", label = "Pin tab", action = "pin_tab", key = "pin_tab" },
  { id = "rename", label = "Rename…", level = "rename" },
  { id = "space", label = "Move to space", hint = "▸", level = "spaces" },
  { id = "tear_off", label = "Move to new window", action = "tear_off" },
  { id = "duplicate", label = "Duplicate tab" },
  { id = "open_settings", label = "Settings…", action = "open_settings", key = "open_settings" },
  { id = "close_others", label = "Close other tabs", action = "close_others_now", confirm = "close_others" },
  { id = "close", label = "Close tab", action = "close_tab_now", key = "close_tab", confirm = "close" },
}

-- Cancel is second so the confirm level can select it and a stray Enter does nothing.
local CONFIRM_ITEMS = {
  { id = "confirm_close", label = "Close" },
  { id = "confirm_cancel", label = "Cancel" },
}

local function compact_mods(mods)
  local out = {}
  for part in tostring(mods or ""):gmatch "[^|]+" do
    out[#out + 1] = MODS[part] or part
  end
  return table.concat(out)
end

---Hints come from the resolved key table, so a rebind shows up without touching this list.
local function hints()
  local out = {}
  for _, binding in ipairs(util.try(keys.build, config.get().keys) or {}) do
    local name = binding.vtabs
    if name and not out[name] then
      local hint = compact_mods(binding.mods) .. tostring(binding.key):upper()
      if util.width(hint) <= MAX_HINT_COLS then
        out[name] = hint
      end
    end
  end
  return out
end

function M.items(gui_window, tab_id, survey)
  local tabs = survey and survey.visible or model.build(gui_window)
  local out, hint = {}, hints()
  for _, spec in ipairs(ITEMS) do
    local entry = {
      id = spec.id,
      label = spec.label,
      level = spec.level,
      action = spec.action,
      hint = spec.hint or (spec.key and hint[spec.key]) or nil,
      confirm = spec.confirm,
      disabled = spec.disabled == true,
    }
    if spec.id == "pin" then
      entry.label = state.is_pinned(tab_id) and "Unpin tab" or "Pin tab"
    elseif spec.id == "close_others" then
      entry.disabled = #tabs <= 1
    elseif spec.id == "space" then
      entry.disabled = not spaces.enabled(config.get())
    end
    out[#out + 1] = entry
  end
  return out
end

---The tabs the confirm level would actually take; `close_others` keeps the one it is anchored on.
local function victims(gui_window, pop, survey)
  if pop.confirm == "close_others" then
    if survey then
      local out = {}
      for _, item in ipairs(model.ordered(survey.visible or {})) do
        if item.tab_id ~= pop.tab_id and not item.is_pinned then
          out[#out + 1] = item.tab_id
        end
      end
      return out
    end
    return actions.others(gui_window, pop.tab_id)
  end
  return { pop.tab_id }
end

function M.open(gui_window, tab_id, anchor_row, anchor_col)
  local wid = gui_window:window_id()
  store.popover[wid] = {
    tab_id = tab_id,
    anchor_row = anchor_row or 0,
    anchor_col = anchor_col,
    level = "root",
    index = 1,
    at = util.now_ms(),
    restore_focus = state.has_focus(wid),
  }
  state.set_focus(wid, true)
  return store.popover[wid]
end

---A popover opened from a key binding has to hold the pane too, or Esc and Enter reach the shell.
function M.grab_focus(gui_window)
  local wid = gui_window:window_id()
  return gate.run(wid, "grab_focus", function()
    local pop = store.popover[wid]
    local sb = M.sidebar_of(gui_window)
    if not pop or not sb then
      return false
    end
    pop.took_pane = true
    sb:activate()
    return true
  end)
end

function M.close(gui_window)
  local wid = gui_window:window_id()
  local pop = store.popover[wid]
  store.popover[wid] = nil
  if pop and pop.took_pane then
    actions.blur_sidebar(gui_window)
  elseif pop and not pop.restore_focus then
    state.set_focus(wid, false)
  end
  return pop ~= nil
end

function M.get(window_id)
  return store.popover[window_id]
end

---Raises the open popover to its confirm level; Cancel is selected, so a stray Enter is harmless.
function M.to_confirm(gui_window, kind, from_root)
  local pop = store.popover[gui_window:window_id()]
  if not pop then
    return false
  end
  pop.level, pop.confirm = "confirm", kind
  pop.count = kind == "close_others" and #actions.others(gui_window, pop.tab_id) or nil
  pop.from_root = from_root or nil
  pop.index = 2
  return true
end

---Runs an item; `rename` changes level instead of acting, disabled items do nothing.
function M.run(gui_window, id)
  local wid = gui_window:window_id()
  local pop = store.popover[wid]
  if not pop then
    return false
  end
  local tab_id = pop.tab_id
  if pop.level == "confirm" then
    if id == "confirm_cancel" then
      return M.close(gui_window)
    end
    if id ~= "confirm_close" then
      return false
    end
    local kind = pop.confirm
    M.close(gui_window)
    actions.run(kind == "close_others" and "close_others_now" or "close_tab_now", gui_window, tab_id)
    return true
  end
  if pop.level == "spaces" then
    if id == "space_auto" then
      M.close(gui_window)
      actions.move_to_space(gui_window, tab_id, nil, false)
      return true
    end
    local space = type(id) == "string" and id:match "^space:(.+)$" or nil
    if not space then
      return false
    end
    M.close(gui_window)
    actions.move_to_space(gui_window, tab_id, space, true)
    return true
  end
  for _, entry in ipairs(M.items(gui_window, tab_id)) do
    if entry.id == id then
      if entry.disabled then
        return false
      end
      if entry.confirm and actions.needs_confirm(gui_window, tab_id, entry.confirm) then
        return M.to_confirm(gui_window, entry.confirm, true)
      end
      if entry.level == "spaces" then
        pop.level = "spaces"
        pop.index = 1
        return true
      end
      if entry.level == "rename" then
        local tab = actions.tab_by_id(gui_window, tab_id)
        pop.level = "rename"
        pop.buffer = tab and util.sanitize(tab:get_title()) or ""
        pop.cursor = utf8.len(pop.buffer) + 1
        return true
      end
      M.close(gui_window)
      if id == "duplicate" then
        local meta = store.tab_meta[tab_id] or {}
        actions.new_tab(gui_window, {
          cwd = meta.cwd,
          domain = meta.domain and { DomainName = meta.domain } or nil,
        })
      elseif entry.action then
        actions.run(entry.action, gui_window, tab_id)
      end
      return true
    end
  end
  return false
end

---Applies one key to the rename buffer; returns "commit", "cancel" or nil when it was consumed.
---The menu message for this window's popover, one level at a time; nil when nothing is open.
---`survey` is the poll's own when the caller has one, so the menu costs no second walk.
function M.wire_body(gui_window, survey)
  local pop = store.popover[gui_window:window_id()]
  if not pop then
    return nil
  end
  -- the whole window, not the visible list: a tab that just left the space still heads its menu
  survey = survey or model.survey(gui_window)
  local body = {
    open = true,
    level = pop.level,
    anchor = { row = pop.anchor_row or 0, col = pop.anchor_col },
    target = pop.tab_id,
    subject = pop.tab_id,
    selected = pop.index,
  }
  if pop.level == "spaces" then
    local current = state.space_of(pop.tab_id)
    local items = {}
    for _, space in ipairs(survey.spaces or {}) do
      local icon = space.icon and space.icon ~= "" and (space.icon .. " ") or ""
      items[#items + 1] = {
        id = "space:" .. space.id,
        label = icon .. space.name,
        hint = space.count > 0 and tostring(space.count) or nil,
        disabled = space.id == current or nil,
      }
    end
    items[#items + 1] = {
      id = "space_auto",
      label = "Auto (follow rules)",
      disabled = not state.space_manual(pop.tab_id) or nil,
    }
    body.items = items
  elseif pop.level == "confirm" then
    local ids = victims(gui_window, pop, survey)
    body.subject = ids[1]
    body.victims = #ids
    local items = {}
    for _, entry in ipairs(CONFIRM_ITEMS) do
      items[#items + 1] = { id = entry.id, label = entry.label, danger = entry.id == "confirm_close" or nil }
    end
    body.items = items
  elseif pop.level == "rename" then
    body.items = { { id = "rename_field", mode = "edit", label = "", value = pop.buffer or "" } }
    body.selected = 1
  else
    local items = {}
    for _, entry in ipairs(M.items(gui_window, pop.tab_id, survey)) do
      items[#items + 1] = {
        id = entry.id,
        label = entry.label,
        hint = entry.hint,
        disabled = entry.disabled or nil,
        danger = entry.confirm and true or nil,
      }
    end
    body.items = items
  end
  return body
end

function M.commit_rename(gui_window)
  local wid = gui_window:window_id()
  local pop = store.popover[wid]
  if not pop or pop.level ~= "rename" then
    return
  end
  local tab = actions.tab_by_id(gui_window, pop.tab_id)
  if tab then
    tab:set_title(util.sanitize(pop.buffer or ""))
  end
  M.close(gui_window)
end

-- A confirm the user reached through the menu has a menu to go back to; one raised by the ✕ does not.
local STEPS_BACK = { rename = true, spaces = true }

---`Esc` steps back a level before it closes.
function M.back(gui_window)
  local pop = store.popover[gui_window:window_id()]
  if pop and (STEPS_BACK[pop.level] or (pop.level == "confirm" and pop.from_root)) then
    pop.level, pop.buffer, pop.cursor = "root", nil, nil
    pop.confirm, pop.count, pop.from_root = nil, nil, nil
    pop.index = 1
    return true
  end
  return M.close(gui_window)
end

function M.sidebar_of(gui_window)
  local tab = util.active_tab(gui_window)
  return tab and sidebar.find(tab) or nil
end

return M
