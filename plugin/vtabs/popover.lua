local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local model = require "vtabs.model"
local actions = require "vtabs.actions"
local keys = require "vtabs.keys"
local util = require "vtabs.util"

local M = {}

local session = state.session

local MAX_TITLE_ROWS = 3
local MAX_HINT_COLS = 8
local FRAME_ROWS = 2
local MIN_ITEM_ROWS = 4

local MODS = { CTRL = "^", SHIFT = "⇧", ALT = "⌥", OPT = "⌥", CMD = "⌘", SUPER = "❖" }

---Header lines are dropped smallest-priority first: domain, cwd, extra title lines, separator.
local DROP = { meta = 1, cwd = 2, title_extra = 3, separator = 4, title = 5 }

local ITEMS = {
  { id = "activate", label = "Switch to tab", action = "activate_tab" },
  { id = "pin", label = "Pin tab", action = "toggle_pin", key = "pin_tab" },
  { id = "rename", label = "Rename…", level = "rename" },
  { id = "space", label = "Move to space", hint = "▸", disabled = true },
  { id = "tear_off", label = "Move to new window", action = "tear_off" },
  { id = "duplicate", label = "Duplicate tab" },
  { id = "close_others", label = "Close other tabs", action = "close_others" },
  { id = "close", label = "Close tab", action = "close_tab", key = "close_tab" },
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

---Breaks on a space, else after a `/`, else hard, so a path keeps its component boundaries.
function M.wrap(text, budget, rows)
  local lines = {}
  local rest = util.sanitize(text or "")
  while rest ~= "" and #lines < rows do
    if util.width(rest) <= budget then
      lines[#lines + 1] = rest
      return lines
    end
    local cut = nil
    for i = math.min(budget + 1, #rest), 2, -1 do
      local ch = rest:sub(i - 1, i - 1)
      if ch == " " then
        cut = i - 1
        break
      elseif ch == "/" and not cut then
        cut = i
      end
    end
    cut = cut or budget + 1
    lines[#lines + 1] = (rest:sub(1, cut - 1):gsub("%s+$", ""))
    rest = (rest:sub(cut):gsub("^%s+", ""))
  end
  if rest ~= "" and #lines > 0 then
    lines[#lines] = util.truncate(lines[#lines] .. " " .. rest, budget, config.get().ellipsis)
  end
  return lines
end

function M.items(gui_window, tab_id)
  local tabs = model.build(gui_window)
  local out, hint = {}, hints()
  for _, spec in ipairs(ITEMS) do
    local entry = {
      id = spec.id,
      label = spec.label,
      level = spec.level,
      action = spec.action,
      hint = spec.hint or (spec.key and hint[spec.key]) or nil,
      disabled = spec.disabled == true,
    }
    if spec.id == "pin" then
      entry.label = state.is_pinned(tab_id) and "Unpin tab" or "Pin tab"
    elseif spec.id == "close_others" then
      entry.disabled = #tabs <= 1
    end
    out[#out + 1] = entry
  end
  return out
end

local function header(gui_window, tab_id, budget, level)
  local meta = session.tab_meta[tab_id] or {}
  local item = model.find(model.build(gui_window), tab_id)
  local lines = {}
  for i, line in ipairs(M.wrap(item and item.title or "tab", budget, MAX_TITLE_ROWS)) do
    lines[#lines + 1] = { text = line, tone = "fg", drop = i == 1 and DROP.title or DROP.title_extra }
  end
  if meta.cwd then
    lines[#lines + 1] = { text = util.shorten_path(meta.cwd, budget), tone = "meta", drop = DROP.cwd }
  end
  local process = util.basename(meta.process)
  if meta.domain or process then
    local text = table.concat({ meta.domain, process }, " · ")
    lines[#lines + 1] = { text = util.truncate(text, budget, config.get().ellipsis), tone = "meta", drop = DROP.meta }
  end
  if level == "root" and #lines > 0 then
    lines[#lines + 1] = { text = "", tone = "meta", drop = DROP.separator }
  end
  return lines
end

---Removes the lowest-priority lines until `keep` remain.
local function drop_to(lines, keep)
  local out = {}
  for _, line in ipairs(lines) do
    out[#out + 1] = line
  end
  while #out > keep do
    local worst, at = nil, nil
    for i, line in ipairs(out) do
      if worst == nil or line.drop < worst then
        worst, at = line.drop, i
      end
    end
    table.remove(out, at)
  end
  return out
end

---Rule 1 then rule 2: below the anchor, else above it, else nil.
local function place(anchor, height, rows)
  if anchor + height <= rows then
    return anchor + 1
  end
  if anchor - height >= 1 then
    return anchor - height
  end
  return nil
end

local function scroll_for(index, visible, count)
  if count <= visible then
    return 0
  end
  return math.max(0, math.min(index - visible, count - visible))
end

---Where the popover sits and how much header it can afford (§1.9's five rules, in order).
function M.layout(gui_window, pop, rows, cols)
  local budget = cols - 6
  local items = M.items(gui_window, pop.tab_id)
  local full = header(gui_window, pop.tab_id, budget, pop.level)
  local anchor = math.max(0, math.min(pop.anchor_row or 0, rows))

  for keep = #full, 0, -1 do
    local lines = drop_to(full, keep)
    local height = FRAME_ROWS + #lines + #items
    local a = height <= rows and place(anchor, height, rows) or nil
    if a then
      return { a = a, b = a + height - 1, lines = lines, items = items, visible = #items, scroll = 0 }
    end
  end

  -- Rules 4 and 5: no header, scroll the list, and take the whole pane when even that will not fit.
  local visible = math.max(math.min(#items, rows - FRAME_ROWS), 1)
  local height = FRAME_ROWS + visible
  local a = place(anchor, height, rows) or 1
  return {
    a = a,
    b = math.min(a + height - 1, math.max(rows, 1)),
    lines = {},
    items = items,
    visible = visible,
    scroll = scroll_for(pop.index, visible, #items),
  }
end

M.MIN_ROWS = FRAME_ROWS + MIN_ITEM_ROWS

function M.open(gui_window, tab_id, anchor_row)
  local wid = gui_window:window_id()
  session.popover[wid] = {
    tab_id = tab_id,
    anchor_row = anchor_row or 0,
    level = "root",
    index = 1,
    at = util.now_ms(),
    restore_focus = state.has_focus(wid),
  }
  state.set_focus(wid, true)
  return session.popover[wid]
end

function M.close(gui_window)
  local wid = gui_window:window_id()
  local pop = session.popover[wid]
  session.popover[wid] = nil
  if pop and not pop.restore_focus then
    state.set_focus(wid, false)
  end
  return pop ~= nil
end

function M.get(window_id)
  return session.popover[window_id]
end

---Moves the selection by `delta`, skipping disabled items and stopping at the ends.
function M.move(gui_window, delta)
  local pop = session.popover[gui_window:window_id()]
  if not pop then
    return
  end
  local items = M.items(gui_window, pop.tab_id)
  local i = pop.index
  for _ = 1, #items do
    i = i + delta
    if i < 1 or i > #items then
      return
    end
    if not items[i].disabled then
      pop.index = i
      return
    end
  end
end

---First-letter jump to the next enabled item starting with `ch`.
function M.jump(gui_window, ch)
  local pop = session.popover[gui_window:window_id()]
  if not pop or type(ch) ~= "string" or ch == "" then
    return false
  end
  local items = M.items(gui_window, pop.tab_id)
  local want = ch:lower()
  for step = 1, #items do
    local i = (pop.index + step - 1) % #items + 1
    local item = items[i]
    if not item.disabled and item.label:sub(1, 1):lower() == want then
      pop.index = i
      return true
    end
  end
  return false
end

function M.selected(gui_window)
  local pop = session.popover[gui_window:window_id()]
  if not pop then
    return nil
  end
  return M.items(gui_window, pop.tab_id)[pop.index]
end

---Runs an item; `rename` changes level instead of acting, disabled items do nothing.
function M.run(gui_window, id)
  local wid = gui_window:window_id()
  local pop = session.popover[wid]
  if not pop then
    return false
  end
  local tab_id = pop.tab_id
  for _, entry in ipairs(M.items(gui_window, tab_id)) do
    if entry.id == id then
      if entry.disabled then
        return false
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
        local meta = session.tab_meta[tab_id] or {}
        actions.new_tab(gui_window, {
          cwd = meta.cwd,
          domain = meta.domain and { DomainName = meta.domain } or nil,
        })
      elseif entry.action then
        actions[entry.action](gui_window, tab_id)
      end
      return true
    end
  end
  return false
end

---Applies one key to the rename buffer; returns "commit", "cancel" or nil when it was consumed.
function M.edit(pop, key, mods)
  local ctrl = util.contains(mods, "ctrl")
  local function chars()
    local out = {}
    for _, code in utf8.codes(pop.buffer) do
      out[#out + 1] = utf8.char(code)
    end
    return out
  end
  local text = chars()
  local at = math.max(1, math.min(pop.cursor or #text + 1, #text + 1))
  local function rebuild(list, cursor)
    pop.buffer = table.concat(list)
    pop.cursor = math.max(1, math.min(cursor, #list + 1))
  end
  if key == "enter" then
    return "commit"
  elseif key == "escape" or (ctrl and key == "c") then
    return "cancel"
  elseif ctrl and key == "u" then
    rebuild({}, 1)
  elseif ctrl and key == "a" then
    pop.cursor = 1
  elseif ctrl and key == "e" then
    pop.cursor = #text + 1
  elseif ctrl and key == "k" then
    for i = #text, at, -1 do
      table.remove(text, i)
    end
    rebuild(text, at)
  elseif ctrl and key == "w" then
    local i = at - 1
    while i > 0 and text[i] == " " do
      i = i - 1
    end
    while i > 0 and text[i] ~= " " do
      i = i - 1
    end
    for j = at - 1, i + 1, -1 do
      table.remove(text, j)
    end
    rebuild(text, i + 1)
  elseif key == "backspace" then
    if at > 1 then
      table.remove(text, at - 1)
      rebuild(text, at - 1)
    end
  elseif key == "delete" then
    if at <= #text then
      table.remove(text, at)
      rebuild(text, at)
    end
  elseif key == "left" then
    pop.cursor = math.max(1, at - 1)
  elseif key == "right" then
    pop.cursor = math.min(#text + 1, at + 1)
  elseif key == "home" then
    pop.cursor = 1
  elseif key == "end" then
    pop.cursor = #text + 1
  elseif not ctrl and type(key) == "string" and utf8.len(key) == 1 and utf8.codepoint(key) >= 32 then
    table.insert(text, at, key)
    rebuild(text, at + 1)
  end
  return nil
end

function M.commit_rename(gui_window)
  local wid = gui_window:window_id()
  local pop = session.popover[wid]
  if not pop or pop.level ~= "rename" then
    return
  end
  local tab = actions.tab_by_id(gui_window, pop.tab_id)
  if tab then
    tab:set_title(util.sanitize(pop.buffer or ""))
  end
  M.close(gui_window)
end

---`Esc` steps back a level before it closes.
function M.back(gui_window)
  local pop = session.popover[gui_window:window_id()]
  if pop and pop.level ~= "root" then
    pop.level, pop.buffer, pop.cursor = "root", nil, nil
    return true
  end
  return M.close(gui_window)
end

function M.sidebar_of(gui_window)
  local tab = util.active_tab(gui_window)
  return tab and sidebar.find(tab) or nil
end

return M
