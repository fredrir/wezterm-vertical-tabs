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
  }
end

---Whether the grid has a text column at all. Every painter asks the grid this one way, so a rail
---never has to be recognised twice; a nil budget means the glyph is all there is room for.
function M.has_text(g)
  return g.title_budget ~= nil
end

local ACTION_STRIDE = 3

---Columns between one action glyph and the next. The lights are 20 pt apart whatever the font is,
---so a fixed cell count drifts out of step with them; two is the floor a hit span can live in.
local function action_stride(strip)
  local cell_w = strip and strip.cell_w
  if not cell_w or cell_w <= 0 then
    return ACTION_STRIDE
  end
  return math.max(2, math.floor(require("vtabs.platform").BUTTON_PITCH_PT / cell_w + 0.5))
end
local ACTION_DEFAULT = { "toggle", "new_tab", "settings" }
local ACTION_BUILTIN = { toggle = true, new_tab = true, settings = true, search = true }
-- `search` has no built-in behaviour, so it is drawn only when a hook answers it; `settings` opens
-- the page, and a hook only overrides where it goes.
local ACTION_HOOKED = { search = true }

local function resolved_actions(cfg)
  local wanted = cfg.strip_actions
  if type(wanted) ~= "table" or #wanted == 0 then
    wanted = ACTION_DEFAULT
  end
  local hooks = cfg.hooks or {}
  local out = {}
  for _, entry in ipairs(wanted) do
    if type(entry) == "table" and entry.on_click then
      out[#out + 1] = { id = entry.id or "custom", icon = entry.icon }
    elseif entry == "toggle" then
      if cfg.toggle_button then
        out[#out + 1] = { id = "toggle" }
      end
    elseif ACTION_HOOKED[entry] then
      if hooks[entry] then
        out[#out + 1] = { id = entry }
      end
    elseif ACTION_BUILTIN[entry] then
      out[#out + 1] = { id = entry }
    end
  end
  return out
end

---Action glyphs and their 3-column hit spans, left to right on the lights' own row. They sit beside
---the traffic lights where there are any, and otherwise on the sidebar's trailing edge, which is the
---only place a toolbar fits without eating the list's margin.
function M.strip_actions(cfg, strip, g, rail, cols)
  local list = resolved_actions(cfg)
  local row = strip and (strip.toggle_row or (strip.toggle and strip.toggle.row))
  if #list == 0 or row == nil then
    return {}, nil
  end
  local reserve = strip.cols or 0
  local stride = action_stride(strip)
  local base
  if rail then
    local width = g.card_x2
    if reserve == 0 and width >= 9 and 2 + stride * (#list - 1) + 1 <= width - 1 then
      base = 2
    else
      list = { list[1] }
      base = math.ceil(width / 2)
    end
  elseif reserve > 0 then
    -- two clear of the last light, whatever the cell width made the reserve
    base = reserve + 2
  elseif cfg.position == "right" then
    base = g.card_x1
  else
    -- no lights to sit beside: the trailing edge is the toolbar convention, and it leaves the
    -- list's left margin clean. The cluster keeps its order, so the toggle is still its first glyph.
    base = g.card_x2 - 1 - stride * (#list - 1)
  end
  local out = {}
  for i, action in ipairs(list) do
    local x = base + stride * (i - 1)
    -- the span is exactly one stride wide, so neighbours stay contiguous with no dead cell between
    local x1 = x - math.floor((stride - 1) / 2)
    local x2 = x1 + stride - 1
    if x1 >= 1 and x2 <= cols then
      out[#out + 1] = { id = action.id, icon = action.icon, x = x, x1 = x1, x2 = x2 }
    end
  end
  return out, row
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

---Blank card rows above and below the content block, by `tab_height`.
local PADS = { row = 0, card = 1, tall = 2 }

---`tab_height` decides the pads, `meta` decides the content lines; a rail slot is the expanded slot
---with the columns taken away, so a tab keeps its rows across the toggle.
local function card_rows(item, cfg)
  local dense = item.is_pinned and cfg.pinned_style ~= "full"
  local armed_dense = item.armed_pinned == true and cfg.pinned_style ~= "full"
  local one_row = dense
  if item.armed_pinned ~= nil then
    one_row = armed_dense
  end
  if one_row then
    return 1, dense, 0, 1
  end
  local pads = PADS[cfg.tab_height] or 1
  local content = cfg.meta == false and 1 or 2
  return 2 * pads + content, dense, pads, content
end

---Rows an unpinned tab owns, gap included: the shortest travel that can land on another slot.
function M.slot_rows(cfg)
  local pads = PADS[cfg.tab_height] or 1
  local content = cfg.meta == false and 1 or 2
  return 2 * pads + content + math.max(cfg.row_gap or 0, 0)
end

---The card row the title and icon sit on: the middle of the pad/content/pad block.
function M.icon_row(rows_in_card)
  return math.ceil(math.max(rows_in_card, 1) / 2)
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
  -- new_tab_rows has to see the shortened pane too, or the ghost claims the rows reserved below and
  -- above it: one page row, so the outlined card is as far from the last title as the cards are
  -- from each other. The one-row form has no border and needs none.
  local pad_b = math.min(math.max(cfg.padding.bottom or 0, 0), math.max(view.rows - strip_rows, 0))
  local ghost_h = M.new_tab_rows(cfg, view.rows - pad_b - 1, strip_rows, #footer)
  local ghost_gap = ghost_h == 3 and 1 or 0
  local list_rows = math.max(view.rows - strip_rows - ghost_gap - ghost_h - #footer - pad_b, 0)

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
    local rows_in_card, dense, pads, content = card_rows(item, cfg)
    local n = 0
    local function row(part)
      n = n + 1
      plan[#plan + 1] = {
        kind = "tab",
        item = item,
        slot = slot,
        part = part,
        rows_in_card = rows_in_card,
        row_in_card = n,
      }
    end
    for _ = 1, pads do
      row "pad"
    end
    row "title"
    if content >= 2 then
      row "meta"
    end
    for _ = 1, pads do
      row "pad"
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
  local actions, action_row = M.strip_actions(cfg, view.strip, g, rail, cols)
  -- two rows tall, so the target stays comfortable wherever the lights' centre lands
  local band_last = action_row and math.min(action_row + 1, strip_rows) or 0
  local lit_id = nil
  if view.hover and action_row and view.hover.y >= action_row and view.hover.y <= band_last then
    for _, action in ipairs(actions) do
      if view.hover.x >= action.x1 and view.hover.x <= action.x2 then
        lit_id = action.id
      end
    end
  end
  for row = 1, strip_rows do
    local in_band = #actions > 0 and action_row ~= nil and row >= action_row and row <= band_last
    rows[row] = {
      kind = "strip",
      actions = in_band and actions or nil,
      lit_id = in_band and lit_id or nil,
      glyph = action_row ~= nil and row == action_row,
    }
    if in_band then
      local spans = {}
      for _, action in ipairs(actions) do
        spans[#spans + 1] = { id = action.id, x1 = action.x1, x2 = action.x2 }
      end
      hits[row] = { kind = "action", x1 = actions[1].x1, x2 = actions[#actions].x2, spans = spans }
    else
      hits[row] = { kind = "strip" }
    end
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
        row_in_card = entry.row_in_card or 1,
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

  if ghost_gap > 0 then
    local row = strip_rows + list_rows + 1
    rows[row] = { kind = "space" }
    hits[row] = { kind = "space" }
  end

  if ghost_h > 0 then
    local base = strip_rows + list_rows + ghost_gap
    local hovered = view.hover ~= nil and view.hover.y > base and view.hover.y <= base + ghost_h
    -- the rail draws the same outlined card; only a window too short for it falls back to a bare glyph
    local shape = ghost_h == 3 and "card" or (rail and "rail" or "row")
    for i = 1, ghost_h do
      local row = base + i
      if row <= view.rows then
        rows[row] = { kind = "ghost", shape = shape, index = i, hovered = hovered }
        hits[row] = { kind = "new_tab", x1 = g.card_x1, x2 = g.card_x2 }
      end
    end
  end

  -- painted, not skipped: an unpainted row keeps whatever the pane had there before
  for i = 1, pad_b do
    local row = view.rows - pad_b + i
    if row >= 1 then
      rows[row] = { kind = "space" }
      hits[row] = { kind = "space" }
    end
  end

  for i, entry in ipairs(footer) do
    local row = view.rows - pad_b - #footer + i
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
    actions = actions,
    rows = rows,
    hits = hits,
  }
end

return M
