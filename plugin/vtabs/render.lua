local ansi = require "vtabs.ansi"
local util = require "vtabs.util"

---@class VtabsItem
---@field tab_id integer
---@field index integer
---@field is_active boolean
---@field is_pinned boolean
---@field is_private boolean
---@field title string
---@field meta string|nil
---@field icon string
---@field has_unseen boolean

---@class VtabsStripToggle
---@field row integer
---@field x1 integer
---@field x2 integer

---@class VtabsStrip
---@field rows integer
---@field toggle VtabsStripToggle|nil

---@class VtabsHover
---@field x integer
---@field y integer

---@class VtabsDrag
---@field tab_id integer
---@field over_index integer|nil
---@field active boolean
---@field outside boolean|nil

---@class VtabsTheme
---@field bg integer[]
---@field fg integer[]
---@field dim integer[]
---@field accent integer[]
---@field title_idle integer[]
---@field meta_fg integer[]
---@field active_bg integer[]
---@field active_fg integer[]
---@field hover_bg integer[]
---@field hover_fg integer[]
---@field focus_bg integer[]
---@field drag_bg integer[]
---@field drag_fg integer[]
---@field pinned_fg integer[]
---@field separator integer[]
---@field new_tab_fg integer[]
---@field close_fg integer[]
---@field close_hover_fg integer[]
---@field unseen_fg integer[]
---@field private_accent integer[]
---@field border integer[]
---@field border_idle integer[]
---@field scroll_fg integer[]
---@field scroll_idle_fg integer[]

---@class VtabsRenderInput
---@field cols integer
---@field rows integer
---@field items VtabsItem[]
---@field theme VtabsTheme
---@field cfg table
---@field glyphs table
---@field strip VtabsStrip|nil
---@field hover VtabsHover|nil
---@field drag VtabsDrag|nil
---@field scroll integer|nil
---@field focus_index integer|nil
---@field ensure_visible integer|nil
---@field footer (string|table)[]|nil
---@field private boolean|nil
---@field user_scrolled boolean|nil

---@class VtabsSpan
---@field id string
---@field x1 integer
---@field x2 integer

---@class VtabsHit
---@field kind string
---@field id integer|nil
---@field slot integer|nil
---@field part string|nil
---@field x1 integer|nil
---@field x2 integer|nil
---@field pinned boolean|nil
---@field entry table|nil
---@field spans VtabsSpan[]|nil

local M = {}

local function style(fg, bg)
  return ansi.bg(bg) .. ansi.fg(fg)
end

---Concatenates segments into exactly `cols` columns; overflow is cut, underflow padded.
local function paint(segments, cols, bg)
  local out, used = {}, 0
  for _, seg in ipairs(segments) do
    local text = seg.text
    local w = util.width(text)
    if used + w > cols then
      text = util.truncate(text, cols - used, "")
      w = util.width(text)
    end
    if w > 0 then
      out[#out + 1] = style(seg.fg, seg.bg or bg)
        .. (seg.bold and ansi.bold(true) or "")
        .. text
        .. (seg.bold and ansi.bold(false) or "")
      used = used + w
    end
  end
  if used < cols then
    out[#out + 1] = ansi.bg(bg) .. string.rep(" ", cols - used)
  end
  return table.concat(out) .. ansi.RESET
end

local function row_colors(item, theme, hovered, dragging, focused)
  if dragging then
    return theme.drag_fg, theme.drag_bg
  end
  if focused then
    return theme.fg, theme.focus_bg
  end
  if item.is_active then
    return theme.active_fg, theme.active_bg
  end
  if hovered then
    return theme.hover_fg, theme.hover_bg
  end
  return theme.fg, theme.bg
end

local function marker(item, theme, focused, glyphs)
  if item.is_active then
    return glyphs.active, item.is_private and theme.private_accent or theme.accent
  end
  if focused then
    return glyphs.focus, theme.accent
  end
  if item.has_unseen then
    return glyphs.unseen, theme.unseen_fg
  end
  return " ", theme.fg
end

---Lays out one tab row and returns the painted text plus the close-button span.
local function tab_row(item, ctx, hovered, dragging, focused)
  local theme, cfg, cols, glyphs = ctx.theme, ctx.cfg, ctx.cols, ctx.glyphs
  local fg, bg = row_colors(item, theme, hovered, dragging, focused)
  local compact_pinned = item.is_pinned and cfg.pinned_style == "compact"
  local tail_w = 0
  local tail = nil
  if compact_pinned then
    tail = { text = glyphs.pinned, fg = theme.pinned_fg }
  elseif cfg.close_button ~= "never" and not dragging then
    local show = cfg.close_button == "always" or hovered or item.is_active
    local close_fg = (hovered and ctx.hover and ctx.hover.x >= cols - cfg.padding.right - util.width(glyphs.close))
        and theme.close_hover_fg
      or theme.close_fg
    tail = { text = show and glyphs.close or string.rep(" ", util.width(glyphs.close)), fg = close_fg, close = show }
  end
  if tail then
    tail_w = util.width(tail.text) + 1
  end

  local mark, mark_fg = marker(item, theme, focused, glyphs)
  local segments = {
    { text = mark, fg = mark_fg },
    { text = string.rep(" ", cfg.padding.left), fg = fg },
  }
  local prefix_w = util.width(mark) + cfg.padding.left
  if cfg.icons and item.icon ~= "" then
    segments[#segments + 1] = { text = item.icon .. " ", fg = item.is_private and theme.private_accent or fg }
    prefix_w = prefix_w + util.width(item.icon) + 1
  end
  if cfg.show_index then
    local index = string.format("%d ", item.index)
    segments[#segments + 1] = { text = index, fg = theme.dim }
    prefix_w = prefix_w + util.width(index)
  end
  local budget = cols - prefix_w - tail_w - cfg.padding.right
  local title = util.truncate(item.title, budget, cfg.ellipsis)
  segments[#segments + 1] = { text = title, fg = fg, bold = item.is_active }
  local used = prefix_w + util.width(title)
  local close_span = nil
  if tail and budget >= 0 then
    local fill = cols - used - cfg.padding.right - tail_w
    segments[#segments + 1] = { text = string.rep(" ", math.max(fill, 0) + 1), fg = fg }
    local from = used + math.max(fill, 0) + 2
    segments[#segments + 1] = { text = tail.text, fg = tail.fg }
    if tail.close then
      close_span = { from = from, to = from + util.width(tail.text) - 1 }
    end
  end
  return paint(segments, cols, bg), close_span
end

local function text_row(text, fg, bg, ctx)
  local cfg, cols = ctx.cfg, ctx.cols
  local budget = cols - cfg.padding.left - cfg.padding.right
  local body = util.truncate(text, math.max(budget, 0), cfg.ellipsis)
  return paint({ { text = string.rep(" ", cfg.padding.left), fg = fg }, { text = body, fg = fg } }, cols, bg)
end

local function blank_row(bg, cols)
  return ansi.bg(bg) .. string.rep(" ", cols) .. ansi.RESET
end

---Reorders items so the dragged one sits at the hovered slot in the pinned-first sequence.
local function apply_drag(items, drag)
  if not drag or not drag.over_index or not drag.active then
    return items
  end
  local dragged = nil
  local rest = {}
  for _, item in ipairs(items) do
    if item.tab_id == drag.tab_id then
      dragged = item
    else
      rest[#rest + 1] = item
    end
  end
  if not dragged then
    return items
  end
  local pinned, unpinned = util.partition(rest, function(i)
    return i.is_pinned
  end)
  local ghost = {}
  for k, v in pairs(dragged) do
    ghost[k] = v
  end
  ghost.is_pinned = drag.over_index <= #pinned + (dragged.is_pinned and 1 or 0)
  local target = math.max(1, math.min(drag.over_index, #rest + 1))
  local sequence = {}
  for _, i in ipairs(pinned) do
    sequence[#sequence + 1] = i
  end
  for _, i in ipairs(unpinned) do
    sequence[#sequence + 1] = i
  end
  table.insert(sequence, target, ghost)
  return sequence
end

local function footer_entry(entry)
  if type(entry) == "string" then
    return { text = entry }
  end
  return entry or { text = "" }
end

---Renders the sidebar. `view` = { cols, rows, items, theme, cfg, glyphs, hover, drag, scroll, focus_index, footer, ensure_visible }.
---Returns { data, hits = { [row] = hit }, total_rows, scroll }.
function M.render(view)
  local cfg, theme, cols = view.cfg, view.theme, view.cols
  local ctx = { theme = theme, cfg = cfg, cols = cols, glyphs = view.glyphs, hover = view.hover }
  local footer = {}
  for _, entry in ipairs(view.footer or {}) do
    footer[#footer + 1] = footer_entry(entry)
  end
  local list_rows = math.max(view.rows - #footer, 1)

  local ordered = apply_drag(view.items, view.drag)
  local pinned, rest = util.partition(ordered, function(i)
    return i.is_pinned
  end)
  local show_separator = #pinned > 0 and cfg.separator ~= "none"

  local plan = {}
  for _ = 1, cfg.padding.top do
    plan[#plan + 1] = { kind = "space" }
  end
  local slot = 0
  local function add_tab(item)
    slot = slot + 1
    plan[#plan + 1] = { kind = "tab", item = item, slot = slot }
    for _ = 1, cfg.row_gap do
      plan[#plan + 1] = { kind = "space" }
    end
  end
  for _, item in ipairs(pinned) do
    add_tab(item)
  end
  if show_separator then
    plan[#plan + 1] = { kind = "separator" }
  end
  for _, item in ipairs(rest) do
    add_tab(item)
  end
  if cfg.new_tab_button then
    plan[#plan + 1] = { kind = "new_tab" }
  end

  local total = #plan
  local max_scroll = math.max(total - list_rows, 0)
  local scroll = math.max(0, math.min(view.scroll or 0, max_scroll))
  if view.ensure_visible then
    for i, entry in ipairs(plan) do
      if entry.kind == "tab" and entry.item.tab_id == view.ensure_visible then
        if i <= scroll then
          scroll = i - 1
        elseif i > scroll + list_rows then
          scroll = i - list_rows
        end
        break
      end
    end
  end
  local hovered_line = view.hover and (view.hover.y + scroll) or nil

  local out = { ansi.HIDE_CURSOR }
  local hits = {}
  for row = 1, list_rows do
    local entry = plan[row + scroll]
    local text, hit
    if not entry or entry.kind == "space" then
      text, hit = blank_row(theme.bg, cols), { kind = "space" }
    elseif entry.kind == "tab" then
      local item = entry.item
      local hovered = hovered_line == row + scroll
      local dragging = view.drag and view.drag.active and view.drag.tab_id == item.tab_id
      local focused = view.focus_index == entry.slot
      local close
      text, close = tab_row(item, ctx, hovered, dragging, focused)
      hit = { kind = "tab", tab_id = item.tab_id, slot = entry.slot, close = close, pinned = item.is_pinned }
    elseif entry.kind == "separator" then
      hit = { kind = "separator" }
      if cfg.separator == "rule" then
        local width = math.max(cols - cfg.padding.left - cfg.padding.right, 1)
        text = text_row(string.rep("─", width), theme.separator, theme.bg, ctx)
      else
        text = blank_row(theme.bg, cols)
      end
    elseif entry.kind == "new_tab" then
      local hovered = hovered_line == row + scroll
      local fg = hovered and theme.hover_fg or theme.new_tab_fg
      local bg = hovered and theme.hover_bg or theme.bg
      text = text_row(" " .. view.glyphs.new_tab .. " " .. cfg.new_tab_label, fg, bg, ctx)
      hit = { kind = "new_tab" }
    end
    if cfg.scroll_indicator and total > list_rows and cfg.padding.right >= 1 then
      local thumb_len = math.max(1, math.floor(list_rows * list_rows / total))
      local thumb_start = 1 + math.floor(scroll * (list_rows - thumb_len) / math.max(max_scroll, 1) + 0.5)
      if row >= thumb_start and row < thumb_start + thumb_len then
        text = text
          .. ansi.cup(row, cols)
          .. ansi.bg(theme.bg)
          .. ansi.fg(theme.scroll_fg)
          .. view.glyphs.scroll
          .. ansi.RESET
      end
    end
    if view.drag and view.drag.outside then
      local edge = cfg.position == "right" and 1 or cols
      text = text .. ansi.cup(row, edge) .. ansi.bg(theme.accent) .. " " .. ansi.RESET
    end
    out[#out + 1] = ansi.cup(row, 1) .. text
    hits[row] = hit
  end
  for i, entry in ipairs(footer) do
    local row = list_rows + i
    if row <= view.rows then
      local fg = entry.fg or theme.dim
      local bg = entry.bg or theme.bg
      local hovered = view.hover and view.hover.y == row
      if hovered and entry.id then
        fg, bg = theme.hover_fg, theme.hover_bg
      end
      out[#out + 1] = ansi.cup(row, 1) .. text_row(util.sanitize(entry.text or ""), fg, bg, ctx)
      hits[row] = { kind = "footer", entry = entry }
    end
  end
  out[#out + 1] = ansi.RESET

  return { data = table.concat(out), hits = hits, total_rows = total, scroll = scroll }
end

return M
