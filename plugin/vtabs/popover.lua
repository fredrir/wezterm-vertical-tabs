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
local MIN_W = 16
local MIN_RENDER_W = 4
-- Interior columns a row spends on anything but its label: borders, marker, both margins, and the
-- gap a hint needs on top of that.
local LABEL_PAD = 5
local HINT_PAD = 6
-- Every row's text starts here, so header lines and item labels share one left margin.
local TEXT_REL = 4

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
  { id = "close_others", label = "Close other tabs", action = "close_others", confirm = "close_others" },
  { id = "close", label = "Close tab", action = "close_tab", key = "close_tab", confirm = "close" },
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
      confirm = spec.confirm,
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

---The tabs the confirm level would actually take; `close_others` keeps the one it is anchored on.
local function victims(gui_window, pop)
  if pop.confirm == "close_others" then
    return actions.others(gui_window, pop.tab_id)
  end
  return { pop.tab_id }
end

---What the confirm level asks: `Close <title>?`, then `and N others` when more than one would go.
---The title names the first tab that would close, never the one `close_others` keeps.
local function question(gui_window, pop)
  if pop.level ~= "confirm" then
    return nil
  end
  local ids = victims(gui_window, pop)
  local first = ids[1] and model.find(model.build(gui_window), ids[1])
  local title = first and first.title or ""
  local lines = { string.format("Close %s?", title ~= "" and title or "tab") }
  if #ids > 1 then
    lines[2] = string.format("and %d other%s", #ids - 1, #ids == 2 and "" or "s")
  end
  return lines
end

local function items_for(gui_window, pop)
  if pop.level == "confirm" then
    return CONFIRM_ITEMS
  end
  return M.items(gui_window, pop.tab_id)
end

local function header(gui_window, pop, budget)
  local tab_id, level = pop.tab_id, pop.level
  local meta = session.tab_meta[tab_id] or {}
  local item = model.find(model.build(gui_window), tab_id)
  local lines = {}
  if level == "confirm" then
    -- The qualifier goes before the question does: "and 3 others" alone asks nothing.
    for n, ask in ipairs(question(gui_window, pop)) do
      for i, line in ipairs(M.wrap(ask, budget, MAX_TITLE_ROWS)) do
        local first = n == 1 and i == 1
        lines[#lines + 1] = { text = line, tone = "fg", drop = first and DROP.title or DROP.title_extra }
      end
    end
    lines[#lines + 1] = { text = "", tone = "meta", drop = DROP.separator }
    return lines
  end
  for i, line in ipairs(M.wrap(item and item.title or "tab", budget, MAX_TITLE_ROWS)) do
    lines[#lines + 1] = { text = line, tone = "fg", drop = i == 1 and DROP.title or DROP.title_extra }
  end
  if meta.cwd then
    lines[#lines + 1] = { text = util.shorten_path(meta.cwd, budget), tone = "meta", drop = DROP.cwd }
  end
  local process = util.basename(meta.process)
  if meta.domain or process then
    local text = table.concat({ meta.domain, process }, config.get().meta_sep)
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

local function label_width(entry)
  local w = util.width(entry.label or "")
  if entry.hint then
    return w + util.width(entry.hint) + HINT_PAD
  end
  return w + LABEL_PAD
end

---§6.3: as wide as its widest row wants, clamped to the columns the sidebar can spare.
function M.width_for(cfg, cols, items, head)
  local avail = cols - (cfg.padding.left or 0) - (cfg.padding.right or 0)
  local natural = MIN_W
  for _, entry in ipairs(items) do
    natural = math.max(natural, label_width(entry))
  end
  for _, line in ipairs(head or {}) do
    natural = math.max(natural, util.width(line) + LABEL_PAD)
  end
  local want = cfg.popover.width
  if type(want) ~= "number" then
    want = natural
  end
  return math.max(math.min(want, avail), math.min(MIN_W, avail))
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
function M.layout(gui_window, pop, rows, w)
  local budget = w - LABEL_PAD
  local items = items_for(gui_window, pop)
  local full = header(gui_window, pop, budget)
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

function M.open(gui_window, tab_id, anchor_row, anchor_col)
  local wid = gui_window:window_id()
  session.popover[wid] = {
    tab_id = tab_id,
    anchor_row = anchor_row or 0,
    anchor_col = anchor_col,
    level = "root",
    index = 1,
    at = util.now_ms(),
    restore_focus = state.has_focus(wid),
  }
  state.set_focus(wid, true)
  return session.popover[wid]
end

---A popover opened from a key binding has to hold the pane too, or Esc and Enter reach the shell.
function M.grab_focus(gui_window)
  local pop = session.popover[gui_window:window_id()]
  local sb = M.sidebar_of(gui_window)
  if not pop or not sb then
    return false
  end
  pop.took_pane = true
  sb:activate()
  return true
end

function M.close(gui_window)
  local wid = gui_window:window_id()
  local pop = session.popover[wid]
  session.popover[wid] = nil
  if pop and pop.took_pane then
    actions.blur_sidebar(gui_window)
  elseif pop and not pop.restore_focus then
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
  local items = items_for(gui_window, pop)
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

---§6.6: the pointer selects the row it is over, but only inside the menu — a pointer that wandered
---onto the scrim must not erase a keyboard selection.
---@return boolean true when the selection moved and the frame has to be repainted
function M.point_at(gui_window, record, x)
  local pop = session.popover[gui_window:window_id()]
  if not pop or not config.get().popover.follow_pointer then
    return false
  end
  -- Cancel stays selected at the confirm level: a pointer that happens to rest over Close must not
  -- arm a destructive answer the user never chose.
  if pop.level == "confirm" then
    return false
  end
  if not record or record.kind ~= "popover" or record.id == nil then
    return false
  end
  if record.x1 and (x < record.x1 or x > record.x2) then
    return false
  end
  for i, entry in ipairs(items_for(gui_window, pop)) do
    if entry.id == record.id then
      if entry.disabled or pop.index == i then
        return false
      end
      pop.index = i
      return true
    end
  end
  return false
end

---First-letter jump to the next enabled item starting with `ch`.
function M.jump(gui_window, ch)
  local pop = session.popover[gui_window:window_id()]
  if not pop or type(ch) ~= "string" or ch == "" then
    return false
  end
  local items = items_for(gui_window, pop)
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
  return items_for(gui_window, pop)[pop.index]
end

---Raises the open popover to its confirm level; Cancel is selected, so a stray Enter is harmless.
function M.to_confirm(gui_window, kind, from_root)
  local pop = session.popover[gui_window:window_id()]
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
  local pop = session.popover[wid]
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
    actions[kind == "close_others" and "close_others" or "close_tab"](gui_window, tab_id)
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
  elseif not ctrl and type(key) == "string" and utf8.len(key) == 1 and utf8.codepoint(key) >= 32 and #text < 256 then
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

-- A confirm the user reached through the menu has a menu to go back to; one raised by the ✕ does not.
local STEPS_BACK = { rename = true }

---`Esc` steps back a level before it closes.
function M.back(gui_window)
  local pop = session.popover[gui_window:window_id()]
  if pop and (STEPS_BACK[pop.level] or (pop.level == "confirm" and pop.from_root)) then
    pop.level, pop.buffer, pop.cursor = "root", nil, nil
    pop.confirm, pop.count, pop.from_root = nil, nil, nil
    pop.index = 1
    return true
  end
  return M.close(gui_window)
end

local function frame_row(w, left, fill, right, theme)
  return { spans = { { x = 1, text = left .. string.rep(fill, math.max(w - 2, 0)) .. right, fg = theme.border } } }
end

---Right-aligns `hint` so it ends at the interior's last column.
---`x` is the rect's absolute left column: hit records are read against pane columns, and the rect no
---longer starts where the sidebar's padding does.
local function item_row(entry, w, selected, theme, x)
  local txt_x2 = w - 2
  local fg, hint_fg = theme.fg, theme.disabled_fg
  -- The selected row is an accent fill: `hover_bg` on `surface_raised` scored 1.01-1.10 against the
  -- panel it sat on. Its ink covers the destructive tint too — red on accent is unreadable.
  if selected then
    fg, hint_fg = theme.popover_sel_fg, theme.popover_sel_hint
  elseif entry.disabled then
    fg = theme.disabled_fg
  end
  local spans = {
    { x = 1, text = "│", fg = theme.border },
    { x = w, text = "│", fg = theme.border },
    { x = TEXT_REL, text = util.truncate(util.sanitize(entry.label), txt_x2 - TEXT_REL + 1), fg = fg },
  }
  if selected then
    -- An accent bar on an accent field would be invisible, so the marker takes the ink colour.
    spans[#spans + 1] = { x = 2, text = "▎", fg = fg }
  end
  if entry.hint then
    local hint = util.sanitize(entry.hint)
    spans[#spans + 1] = { x = txt_x2 - util.width(hint) + 1, text = hint, fg = hint_fg }
  end
  return {
    bg = selected and theme.popover_sel_bg or nil,
    spans = spans,
    hit = {
      kind = "popover",
      id = entry.id,
      x1 = (x or 1) + 1,
      x2 = (x or 1) + w - 2,
      disabled = entry.disabled or nil,
    },
  }
end

local function text_row(text, tone, w, theme)
  return {
    spans = {
      { x = 1, text = "│", fg = theme.border },
      { x = w, text = "│", fg = theme.border },
      { x = 3, text = util.truncate(util.sanitize(text or ""), w - 4, config.get().ellipsis), fg = tone },
    },
  }
end

---The rename field with a block cursor; the buffer scrolls horizontally inside the interior.
local function rename_rows(pop, w, theme)
  local budget = w - 4
  local text = util.sanitize(pop.buffer or "")
  local chars = {}
  for _, code in utf8.codes(text) do
    chars[#chars + 1] = utf8.char(code)
  end
  local at = math.max(1, math.min(pop.cursor or #chars + 1, #chars + 1))
  local from = math.max(1, at - budget + 1)
  local shown = table.concat(chars, "", from, math.min(#chars, from + budget - 1))
  local under = chars[at] or " "
  return {
    text_row("Rename tab", theme.fg, w, theme),
    text_row("", theme.meta_fg, w, theme),
    {
      spans = {
        { x = 1, text = "│", fg = theme.border },
        { x = w, text = "│", fg = theme.border },
        { x = 3, text = shown, fg = theme.fg },
        { x = 3 + (at - from), text = under, fg = theme.bg, bg = theme.fg },
      },
    },
    text_row("", theme.meta_fg, w, theme),
    text_row("⏎ save   esc cancel", theme.meta_fg, w, theme),
  }
end

---The rect `render.composite` overlays: absolute placement plus one descriptor per row.
function M.rect(gui_window, rows, cols, theme, cfg)
  local pop = session.popover[gui_window:window_id()]
  if not pop then
    return nil
  end
  local first_col = cfg.padding.left + 1
  local w = M.width_for(cfg, cols, items_for(gui_window, pop), question(gui_window, pop))
  -- A width that cannot hold two borders and a cell has nothing to draw. Anything above that does
  -- render, however cramped: a level that is open but unpainted swallows every click in the pane.
  if w < MIN_RENDER_W or rows < FRAME_ROWS + 1 then
    return nil
  end
  -- §6.4: the menu opens at the column that asked for it and slides back inside the sidebar's own.
  local anchor_col = pop.anchor_col or first_col
  local x = math.max(first_col, math.min(anchor_col, cols - cfg.padding.right - w + 1))
  local body = {}
  local placed
  if pop.level == "rename" then
    local content = rename_rows(pop, w, theme)
    placed = { a = math.max(1, math.min((pop.anchor_row or 0) + 1, rows - #content - 1)), items = {} }
    placed.a = math.max(1, math.min(placed.a, math.max(rows - #content - 1, 1)))
    for _, row in ipairs(content) do
      body[#body + 1] = row
    end
  else
    placed = M.layout(gui_window, pop, rows, w)
    for _, line in ipairs(placed.lines) do
      body[#body + 1] = text_row(line.text, line.tone == "fg" and theme.fg or theme.meta_fg, w, theme)
    end
    pop.index = math.max(1, math.min(pop.index, #placed.items))
    for i = 1, placed.visible do
      local entry = placed.items[i + (placed.scroll or 0)]
      if entry then
        body[#body + 1] = item_row(entry, w, i + (placed.scroll or 0) == pop.index, theme, x)
      end
    end
  end
  local out = { frame_row(w, "╭", "─", "╮", theme) }
  for _, row in ipairs(body) do
    out[#out + 1] = row
  end
  out[#out + 1] = frame_row(w, "╰", "─", "╯", theme)
  -- `composite` gives a whole pane row to the rect's hit, so every row carries the rect's own
  -- columns: without them a click level with an item but beside the menu would run it.
  for _, row in ipairs(out) do
    row.hit = row.hit or { kind = "popover" }
    row.hit.x1, row.hit.x2 = x, x + w - 1
  end
  local y = math.max(1, math.min(placed.a, math.max(rows - #out + 1, 1)))
  return {
    x = x,
    y = y,
    w = w,
    h = #out,
    bg = theme.surface_raised,
    scrim = theme.scrim,
    rows = out,
    outside_hit = { kind = "scrim" },
  }
end

function M.sidebar_of(gui_window)
  local tab = util.active_tab(gui_window)
  return tab and sidebar.find(tab) or nil
end

return M
