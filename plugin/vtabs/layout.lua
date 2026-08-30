local util = require "vtabs.util"

local M = {}

local function framed(cfg)
  return type(cfg.frame) == "table" or cfg.frame == true
end

M.framed = framed

---Column grid of §1.1; every landmark derives from `cols` and `padding`.
function M.grid(cfg, cols)
  local pad_l = math.max(cfg.padding.left or 0, 0)
  local pad_r = math.max(cfg.padding.right or 0, 0)
  local g = {}
  g.card_x1 = pad_l + 1
  g.card_x2 = math.max(cols - pad_r - (framed(cfg) and 1 or 0), g.card_x1)
  g.gutter = g.card_x1
  g.icon_x = g.gutter + 2
  g.title_x1 = g.icon_x + 2
  g.close_x = g.card_x2 - 1
  g.title_x2 = g.close_x - 2
  g.meta_x1 = g.title_x1
  g.meta_x2 = g.close_x - 1
  g.close_x1 = g.close_x - 1
  g.close_x2 = g.close_x + 1
  g.title_budget = math.max(g.title_x2 - g.title_x1 + 1, 0)
  g.meta_budget = math.max(g.meta_x2 - g.meta_x1 + 1, 0)
  return g
end

---Collapsed: one icon column, no title, no close, no meta. The nils are the point.
function M.rail_grid(cols)
  return {
    card_x1 = 1,
    card_x2 = cols,
    gutter = 1,
    icon_x = math.ceil(cols / 2),
    title_budget = 0,
    meta_budget = 0,
  }
end

---Reorders items so the dragged one sits at the hovered slot in the pinned-first sequence.
function M.apply_drag(items, drag)
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
  ghost.armed_pinned = dragged.is_pinned
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

---Ghost card height that leaves the list at least 3 rows (2 for the one-row form).
function M.new_tab_rows(cfg, rows, strip_rows, footer_n)
  if not cfg.new_tab_button then
    return 0
  end
  if rows - strip_rows - 3 - footer_n >= 3 then
    return 3
  end
  if rows - strip_rows - 1 - footer_n >= 2 then
    return 1
  end
  return 0
end

local function shows_close(item, cfg, st)
  if cfg.close_button == "never" or st.dragging then
    return false
  end
  if cfg.close_button == "always" or cfg.hover == "press" then
    return true
  end
  return st.hovered or item.is_active
end

---Sub-target on a card row: the close button, or the pin toggle on a dense pinned entry.
local function card_span(item, cfg, st, g, part)
  if g.close_x == nil then
    return nil
  end
  local pin_only = item.is_pinned and cfg.pinned_style ~= "full"
  if part == "title" and pin_only then
    if cfg.pinned_style == "compact" or st.hovered then
      return { { id = "pin", x1 = g.close_x1, x2 = g.close_x2 } }
    end
    return nil
  end
  if pin_only or not shows_close(item, cfg, st) then
    return nil
  end
  if part == "title" or part == "meta" then
    return { { id = "close", x1 = g.close_x1, x2 = g.close_x2 } }
  end
  return nil
end

local function card_rows(item, cfg, rail)
  local dense = item.is_pinned and cfg.pinned_style ~= "full"
  local armed_dense = item.armed_pinned == true and cfg.pinned_style ~= "full"
  local full = cfg.tab_height == "tall" and 3 or 2
  local one_row = dense or rail or cfg.meta == false
  if item.armed_pinned ~= nil then
    one_row = armed_dense or rail or cfg.meta == false
  end
  return one_row and 1 or full, dense
end

---Everything about a frame that does not depend on colour: the grid, the plan, the rows, the hits.
---@param view VtabsRenderInput
function M.plan(view)
  local cfg, cols = view.cfg, view.cols
  local rail = view.rail == true
  local g = rail and M.rail_grid(cols) or M.grid(cfg, cols)
  local strip_rows = view.strip and math.max(view.strip.rows or 0, 0) or math.max(cfg.padding.top or 0, 0)
  strip_rows = math.min(strip_rows, view.rows)
  local footer = {}
  for _, entry in ipairs(view.footer or {}) do
    footer[#footer + 1] = footer_entry(entry)
  end
  local ghost_h = M.new_tab_rows(cfg, view.rows, strip_rows, #footer)
  if rail and ghost_h > 0 then
    ghost_h = 1
  end
  local list_rows = math.max(view.rows - strip_rows - ghost_h - #footer, 0)

  local ordered = M.apply_drag(view.items, view.drag)
  local pinned, rest = util.partition(ordered, function(i)
    return i.is_pinned
  end)
  local plan = {}
  if view.private then
    plan[#plan + 1] = { kind = "header" }
    plan[#plan + 1] = { kind = "space" }
  end
  local slot = 0
  local function push(item)
    slot = slot + 1
    local rows_in_card, dense = card_rows(item, cfg, rail)
    local function row(part)
      plan[#plan + 1] = { kind = "tab", item = item, slot = slot, part = part, rows_in_card = rows_in_card }
    end
    row "title"
    if rows_in_card == 3 then
      row "icon"
    end
    if rows_in_card >= 2 then
      row "meta"
    end
    if not dense or item.armed_pinned ~= nil then
      for _ = 1, cfg.row_gap do
        row "gap"
      end
    end
  end
  for _, item in ipairs(pinned) do
    push(item)
  end
  if #pinned > 0 and cfg.separator ~= "none" then
    plan[#plan + 1] = { kind = cfg.separator == "rule" and "separator" or "space" }
  end
  for _, item in ipairs(rest) do
    push(item)
  end

  local total = #plan
  local max_scroll = math.max(total - list_rows, 0)
  local scroll = math.max(0, math.min(view.scroll or 0, max_scroll))
  if view.ensure_visible then
    for i, entry in ipairs(plan) do
      if entry.kind == "tab" and entry.item.tab_id == view.ensure_visible and entry.part == "title" then
        local last = i + (entry.rows_in_card or 1) - 1
        if i <= scroll then
          scroll = i - 1
        elseif last > scroll + list_rows then
          scroll = math.min(last - list_rows, total - 1)
        end
        break
      end
    end
  end
  scroll = math.max(0, math.min(scroll, max_scroll))

  local hovered_entry = view.hover and plan[view.hover.y - strip_rows + scroll] or nil
  local hovered_id = hovered_entry and hovered_entry.kind == "tab" and hovered_entry.item.tab_id or nil

  -- a gap row paints nothing, so fading it would leave the cut edge looking solid
  local function paints(entry)
    return entry ~= nil and entry.kind ~= "space" and not (entry.kind == "tab" and entry.part == "gap")
  end
  local fade_first, fade_last
  for i = 1, list_rows do
    if paints(plan[i + scroll]) then
      fade_first = i
      break
    end
  end
  for i = list_rows, 1, -1 do
    if paints(plan[i + scroll]) then
      fade_last = i
      break
    end
  end

  local thumb = nil
  if
    cfg.scroll_indicator ~= "never"
    and cfg.scroll_indicator ~= false
    and total > list_rows
    and (not rail or cols >= 7)
  then
    local len = math.max(1, math.floor(list_rows * list_rows / total))
    thumb = {
      first = 1 + math.floor(scroll * (list_rows - len) / math.max(max_scroll, 1) + 0.5),
      len = len,
      lit = cfg.scroll_indicator == "always"
        or view.hover ~= nil
        or (view.drag and view.drag.active or false)
        or view.user_scrolled == true,
    }
  end

  local rows, hits = {}, {}
  local toggle = view.strip and view.strip.toggle
  local toggle_last = toggle and math.min(toggle.row + 1, strip_rows) or 0
  local toggle_on = toggle
      and view.hover ~= nil
      and view.hover.y >= toggle.row
      and view.hover.y <= toggle_last
      and view.hover.x >= toggle.x1
      and view.hover.x <= toggle.x2
    or false
  for row = 1, strip_rows do
    local in_toggle = toggle ~= nil and row >= toggle.row and row <= toggle_last
    rows[row] = {
      kind = "strip",
      toggle = toggle,
      lit = toggle_on and in_toggle,
      glyph = toggle ~= nil and row == toggle.row,
    }
    hits[row] = in_toggle and { kind = "toggle", x1 = toggle.x1, x2 = toggle.x2 } or { kind = "strip" }
  end

  for i = 1, list_rows do
    local row = strip_rows + i
    local entry = plan[i + scroll]
    local fade = nil
    if (i == fade_first and scroll > 0) or (i == fade_last and scroll < max_scroll) then
      fade = 0.5
    end
    local thumb_here = thumb ~= nil and i >= thumb.first and i < thumb.first + thumb.len
    local thumb_lit = thumb_here and thumb.lit
    if not entry or entry.kind == "space" then
      rows[row] = { kind = "space", fade = fade, thumb = thumb_here, thumb_lit = thumb_lit }
      hits[row] = { kind = "space" }
    elseif entry.kind == "header" then
      rows[row] = { kind = "header", fade = fade, thumb = thumb_here, thumb_lit = thumb_lit }
      hits[row] = { kind = "space" }
    elseif entry.kind == "separator" then
      rows[row] = { kind = "separator", fade = fade, thumb = thumb_here, thumb_lit = thumb_lit }
      hits[row] = { kind = "separator" }
    else
      local item = entry.item
      local st = {
        hovered = hovered_id == item.tab_id,
        dragging = view.drag ~= nil and view.drag.active == true and view.drag.tab_id == item.tab_id,
        focused = view.focus_index == entry.slot,
        pointer_x = view.hover and view.hover.x or nil,
      }
      local spans = card_span(item, cfg, st, g, entry.part)
      rows[row] = {
        kind = "card",
        item = item,
        part = entry.part,
        rows_in_card = entry.rows_in_card or 1,
        st = st,
        spans = spans,
        fade = fade,
        thumb = thumb_here,
        thumb_lit = thumb_lit,
      }
      hits[row] = {
        kind = "tab",
        id = item.tab_id,
        slot = entry.slot,
        part = entry.part,
        x1 = g.card_x1,
        x2 = g.card_x2,
        pinned = item.is_pinned,
        spans = spans,
      }
    end
  end

  if ghost_h > 0 then
    local base = strip_rows + list_rows
    local hovered = view.hover ~= nil and view.hover.y > base and view.hover.y <= base + ghost_h
    local shape = rail and "rail" or (ghost_h == 3 and "card" or "row")
    for i = 1, ghost_h do
      local row = base + i
      if row <= view.rows then
        rows[row] = { kind = "ghost", shape = shape, index = i, hovered = hovered }
        hits[row] = { kind = "new_tab", x1 = g.card_x1, x2 = g.card_x2 }
      end
    end
  end

  for i, entry in ipairs(footer) do
    local row = view.rows - #footer + i
    if row >= 1 then
      local hovered = view.hover ~= nil and view.hover.y == row and entry.id ~= nil
      rows[row] = { kind = "footer", entry = entry, hovered = hovered }
      hits[row] = { kind = "footer", id = entry.id, entry = entry, x1 = g.card_x1, x2 = g.card_x2 }
    end
  end

  return {
    grid = g,
    rail = rail,
    strip_rows = strip_rows,
    list_rows = list_rows,
    ghost_h = ghost_h,
    footer = footer,
    plan = plan,
    total = total,
    scroll = scroll,
    max_scroll = max_scroll,
    hovered_id = hovered_id,
    rows = rows,
    hits = hits,
  }
end

return M
