local ansi = require "vtabs.ansi"
local layout = require "vtabs.layout"
local theme_mod = require "vtabs.theme"
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

local function faded(rgb, page_bg, fade)
  if not fade or fade <= 0 or type(theme_mod.mix) ~= "function" then
    return rgb
  end
  return theme_mod.mix(rgb, page_bg, fade)
end

---One cell per column; a wide glyph owns two cells, so a row is always exactly `cols` wide.
local function new_line(cols, bg, fg)
  local cells = {}
  for i = 1, cols do
    cells[i] = { ch = " ", fg = fg, bg = bg }
  end
  return cells
end

local function fill(cells, x1, x2, bg)
  for x = math.max(x1, 1), math.min(x2, #cells) do
    cells[x].bg = bg
  end
end

---Writes `text` from column `x`, never past `limit`.
local function put(cells, x, text, st, limit)
  limit = math.min(limit or #cells, #cells)
  local col = x
  for _, code in utf8.codes(text or "") do
    local ch = utf8.char(code)
    local w = util.width(ch)
    if w > 0 then
      if col < 1 or col + w - 1 > limit then
        break
      end
      cells[col] = { ch = ch, fg = st.fg, bg = st.bg or cells[col].bg, bold = st.bold }
      for k = 1, w - 1 do
        cells[col + k] = { ch = "", fg = st.fg, bg = st.bg or cells[col + k].bg }
      end
      col = col + w
    end
  end
  return col
end

local function same(a, b)
  return (a.scrim or 0) == (b.scrim or 0)
    and a.fg[1] == b.fg[1]
    and a.fg[2] == b.fg[2]
    and a.fg[3] == b.fg[3]
    and a.bg[1] == b.bg[1]
    and a.bg[2] == b.bg[2]
    and a.bg[3] == b.bg[3]
    and (a.bold or false) == (b.bold or false)
end

local function emit(cells, page_bg, fade)
  local out, run, text = {}, nil, {}
  local function flush()
    if run and #text > 0 then
      local body = table.concat(text)
      local blank = body:match "^ *$" ~= nil
      out[#out + 1] = ansi.bg(faded(run.bg, page_bg, run.scrim))
        .. (blank and "" or ansi.fg(faded(run.fg, page_bg, fade or run.scrim)))
        .. (run.bold and ansi.bold(true) or "")
        .. body
        .. (run.bold and ansi.bold(false) or "")
    end
    text = {}
  end
  for _, cell in ipairs(cells) do
    if cell.ch ~= "" then
      if not run or not same(run, cell) then
        flush()
        run = cell
      end
      text[#text + 1] = cell.ch
    end
  end
  flush()
  return table.concat(out) .. ansi.RESET
end

---Column grid of §1.1; every landmark derives from `cols` and `padding`.
local function row_colors(item, theme, st)
  if st.dragging then
    return theme.drag_fg, theme.drag_bg
  end
  if st.focused then
    return theme.fg, theme.focus_bg
  end
  if item.is_active then
    return theme.active_fg, theme.active_bg
  end
  if st.hovered then
    return theme.hover_fg, theme.hover_bg
  end
  return theme.title_idle or theme.fg, theme.bg
end

local BAR_MIN_HUE = 24

---A title that ran out of room and came back as `fg` scores well on contrast and shows no hue at
---all, so the bar is gated on distance from `fg`, not on contrast against the card.
local function needs_bar(theme)
  local title, fg = theme.title_active, theme.fg
  if not title or not fg then
    return true
  end
  local delta = 0
  for i = 1, 3 do
    delta = math.max(delta, math.abs((title[i] or 0) - (fg[i] or 0)))
  end
  return delta < BAR_MIN_HUE
end

local function marker(item, theme, st, glyphs)
  local accent = item.is_private and theme.private_accent or theme.accent
  if (item.is_active or st.dragging) and needs_bar(theme) then
    return glyphs.active, accent
  end
  if st.focused then
    return glyphs.focus, accent
  end
  if item.has_unseen then
    return glyphs.unseen, theme.unseen_fg
  end
  return " ", theme.fg
end

---Path-shaped meta keeps its tail: two siblings must not both collapse to their shared parent.
local function fit_meta(text, budget, glyphs, sep)
  if util.width(text) <= budget then
    return text
  end
  sep = sep ~= nil and sep or glyphs.meta_sep
  local head, tail, at, from = "", text, nil, 1
  while true do
    local i = text:find(sep, from, true)
    if not i then
      break
    end
    at, from = i, i + 1
  end
  if at then
    head, tail = text:sub(1, at + #sep - 1), text:sub(at + #sep)
    -- the separator may or may not carry its own spacing; the path starts at the first real byte
    local lead = tail:match "^%s+"
    if lead then
      head, tail = head .. lead, tail:sub(#lead + 1)
    end
  end
  local room = budget - util.width(head)
  local path_like = tail:sub(1, 1) == "~"
    or tail:sub(1, 1) == "/"
    or tail:match "^%a:[\\/]" ~= nil
    or tail:sub(1, 2) == "\\\\"
  if not path_like or room < 3 then
    return util.truncate(text, budget, glyphs.ellipsis)
  end
  return head .. util.shorten_path(tail, room, glyphs.ellipsis)
end

---Paints one row of a card; which sub-targets exist was decided in layout.
local function card_row(item, ctx, st, part, rows_in_card, spans)
  local theme, cfg, glyphs, g, cols = ctx.theme, ctx.cfg, ctx.glyphs, ctx.grid, ctx.cols
  local cells = new_line(cols, theme.bg, theme.fg)
  if part == "gap" then
    return cells
  end
  local fg, bg = row_colors(item, theme, st)
  local icon_fg = st.dragging and fg or theme.meta_fg or theme.dim
  fill(cells, g.card_x1, g.card_x2, bg)

  local rail = ctx.rail == true
  local carries_icon = part == (rows_in_card >= 3 and "icon" or "title")
  local mark, mark_fg = marker(item, theme, st, glyphs)
  -- in the rail the marker rides the icon so the pair reads as one row of the same card
  if rail and carries_icon or not rail and part == "title" then
    put(cells, g.gutter, mark, { fg = mark_fg }, g.gutter)
  elseif (item.is_active or st.dragging) and needs_bar(theme) then
    put(cells, g.gutter, glyphs.active, { fg = item.is_private and theme.private_accent or theme.accent }, g.gutter)
  end

  local function paint_icon()
    put(
      cells,
      g.icon_x,
      util.sanitize(item.icon),
      { fg = item.is_private and theme.private_accent or icon_fg },
      g.icon_x
    )
  end

  if rail then
    if carries_icon and item.icon ~= "" then
      paint_icon()
    end
    return cells
  end

  if part == "icon" then
    if cfg.icons and item.icon ~= "" then
      paint_icon()
    end
  elseif part == "title" then
    if cfg.icons and item.icon ~= "" and carries_icon then
      paint_icon()
    end
    local title = util.truncate(util.sanitize(item.title), g.title_budget, glyphs.ellipsis)
    local title_fg = item.is_active and not st.dragging and (theme.title_active or fg) or fg
    put(cells, g.title_x1, title, { fg = title_fg, bold = item.is_active }, g.title_x2)
    local span = spans and spans[1]
    if span then
      local glyph = span.id == "pin" and glyphs.pinned or glyphs.close
      local glyph_fg = theme.close_fg
      if span.id == "pin" then
        glyph_fg = theme.pinned_fg
      elseif st.hovered and st.pointer_x ~= nil and st.pointer_x >= span.x1 and st.pointer_x <= span.x2 then
        glyph_fg = theme.close_hover_fg
      end
      put(cells, g.close_x, glyph, { fg = glyph_fg }, g.close_x)
    end
  else
    local meta = util.sanitize(item.meta or "")
    local sep = cfg.meta_sep ~= nil and cfg.meta_sep or glyphs.meta_sep
    if cfg.show_index then
      meta = string.format("%d%s%s", item.index, sep, meta)
    end
    local meta_fg = st.dragging and theme.drag_fg or theme.meta_fg or theme.dim
    put(cells, g.meta_x1, fit_meta(meta, g.meta_budget, glyphs, sep), { fg = meta_fg }, g.meta_x2)
  end

  -- inverted from P1: the active card is square, so the chamfer is what marks a hover
  local chamfered = (st.hovered or st.focused) and not item.is_active and not st.dragging
  if chamfered and rows_in_card >= 2 and glyphs.corners == "chamfer" and part ~= "icon" then
    local ch = part == "title" and glyphs.chamfer_top or glyphs.chamfer_bottom
    cells[g.card_x2] = { ch = ch, fg = bg, bg = theme.bg }
  end
  return cells
end

local function chrome_row(ctx, glyph, glyph_x, text, text_fg, glyph_fg, bg)
  local theme, g, cols = ctx.theme, ctx.grid, ctx.cols
  local cells = new_line(cols, bg or theme.bg, theme.fg)
  if glyph and glyph ~= "" then
    put(cells, glyph_x, glyph, { fg = glyph_fg or text_fg }, glyph_x)
  end
  -- the rail has no title column at all, so its chrome rows are the glyph and nothing else
  if text and text ~= "" and g.title_x1 ~= nil then
    put(cells, g.title_x1, util.truncate(text, math.max(g.card_x2 - g.title_x1 + 1, 0), ctx.glyphs.ellipsis), {
      fg = text_fg,
    }, g.card_x2)
  end
  return cells
end

local BORDER_STEP_MIN = 10

---Hover is one step of border colour and nothing else. Where the two steps are indistinguishable
---the accent stands in, so the affordance never sinks into the idle border.
local function ghost_border(theme, hovered)
  local idle = theme.border_idle or theme.separator
  if not hovered then
    return idle
  end
  local step = theme.border
  if not step or not idle then
    return theme.accent
  end
  local delta = 0
  for i = 1, 3 do
    delta = math.max(delta, math.abs((step[i] or 0) - (idle[i] or 0)))
  end
  return delta >= BORDER_STEP_MIN and step or theme.accent
end

---The only outlined element in the sidebar: that is what makes it read as "not a tab".
local function ghost_rows(ctx, hovered)
  local theme, glyphs, g, cols = ctx.theme, ctx.glyphs, ctx.grid, ctx.cols
  local border_fg = ghost_border(theme, hovered)
  local rows = {}
  for i = 1, 3 do
    local cells = new_line(cols, theme.bg, theme.fg)
    if i == 2 then
      put(cells, g.card_x1, glyphs.frame_v, { fg = border_fg }, g.card_x1)
      put(cells, g.icon_x, glyphs.new_tab, { fg = theme.accent }, g.icon_x)
      local label = util.truncate(ctx.cfg.new_tab_label, math.max(g.card_x2 - 1 - g.title_x1, 0), glyphs.ellipsis)
      -- the label keeps the page behind it in both states: an inline band only shows up on hover
      put(cells, g.title_x1, label, { fg = hovered and theme.fg or theme.new_tab_fg }, g.card_x2 - 1)
      put(cells, g.card_x2, glyphs.frame_v, { fg = border_fg }, g.card_x2)
    else
      put(cells, g.card_x1, i == 1 and glyphs.frame_tl or glyphs.frame_bl, { fg = border_fg }, g.card_x1)
      local first, last = g.card_x1 + 1, g.card_x2 - 1
      for x = first, last do
        -- the cells either side of a corner never gap: on Latte closure carries the card, not contrast
        local corner = x <= first + 1 or x >= last - 1
        if corner or (x - first) % 2 == 1 then
          put(cells, x, glyphs.frame_dash, { fg = border_fg }, x)
        end
      end
      put(cells, g.card_x2, i == 1 and glyphs.frame_tr or glyphs.frame_br, { fg = border_fg }, g.card_x2)
    end
    rows[i] = cells
  end
  return rows
end

---Paints the inner edge in the content's own colour and chamfers the page away from it.
function M.frame_edge(painted, cols, theme, glyphs, rows)
  local content = theme.content_bg or theme.bg
  local first, last
  for row = 1, rows do
    if painted[row] then
      first = first or row
      last = row
    end
  end
  for row = 1, rows do
    local cells = painted[row]
    if cells and cells[cols] then
      cells[cols] = { ch = " ", fg = theme.fg, bg = content }
    end
    if cells and cells[cols - 1] and glyphs.corners == "chamfer" and (row == first or row == last) then
      local ch = row == first and glyphs.chamfer_top or glyphs.chamfer_bottom
      cells[cols - 1] = { ch = ch, fg = theme.bg, bg = content }
    end
  end
end

---Overlays a popover rect on a laid-out frame and scrims every row it does not own.
---@param frame table `render.render`'s internal frame: cells, hits, cols, rows, theme
---@param rect table `{ x, y, w, h, scrim, bg, rows = { { bg, fg, spans, hit } }, outside_hit }`
function M.composite(frame, rect)
  local theme, cols = frame.theme, frame.cols
  local scrim = rect.scrim or 0
  local x1 = math.max(rect.x or 1, 1)
  local x2 = math.min(x1 + (rect.w or cols) - 1, cols)
  local y1 = math.max(rect.y or 1, 1)
  local y2 = math.min(y1 + (rect.h or 0) - 1, frame.rows)
  if x1 > cols or y1 > frame.rows or x2 < x1 or y2 < y1 then
    return frame
  end
  for row, cells in pairs(frame.cells) do
    if row < y1 or row > y2 then
      if scrim > 0 then
        for _, cell in ipairs(cells) do
          cell.scrim = scrim
        end
      end
      frame.hits[row] = rect.outside_hit or { kind = "scrim" }
    end
  end
  for i = 1, y2 - y1 + 1 do
    local row = y1 + i - 1
    local cells = frame.cells[row]
    local spec = (rect.rows or {})[i] or {}
    if cells then
      local bg = spec.bg or rect.bg or theme.active_bg
      local fg = spec.fg or theme.fg
      for x = x1, x2 do
        cells[x] = { ch = " ", fg = fg, bg = bg }
      end
      for _, span in ipairs(spec.spans or {}) do
        put(cells, x1 + (span.x or 1) - 1, span.text, { fg = span.fg or fg, bg = bg, bold = span.bold }, x2)
      end
      frame.hits[row] = spec.hit or { kind = "popover" }
    end
  end
  return frame
end

---Encodes a laid-out frame: one self-contained string per row, plus the joined frame.
function M.paint(frame)
  local out = { ansi.HIDE_CURSOR }
  local rows_text, rows_n = {}, 0
  for row = 1, frame.rows do
    local cells = frame.cells[row]
    if cells then
      local body = emit(cells, frame.theme.bg, frame.fades[row])
      rows_text[row] = body
      rows_n = math.max(rows_n, row)
      out[#out + 1] = ansi.cup(row, 1) .. body
    end
  end
  out[#out + 1] = ansi.RESET
  return {
    data = table.concat(out),
    rows = rows_text,
    rows_n = rows_n,
    hits = frame.hits,
    total_rows = frame.total_rows,
    scroll = frame.scroll,
  }
end

---Renders the sidebar: layout decides every row and hit, this paints them.
---@param view VtabsRenderInput
---@return table `{ data, rows, rows_n, hits, total_rows, scroll }`
function M.render(view)
  local cfg, theme, cols = view.cfg, view.theme, view.cols
  local glyphs = view.glyphs
  local plan = layout.plan(view)
  local g = plan.grid
  local ctx = { theme = theme, cfg = cfg, cols = cols, glyphs = glyphs, grid = g, rail = plan.rail }
  local painted, fades = {}, {}

  for row = 1, view.rows do
    local spec = plan.rows[row]
    local cells = nil
    if spec == nil then
      cells = nil
    elseif spec.kind == "strip" then
      cells = new_line(cols, theme.bg, theme.fg)
      if spec.lit then
        fill(cells, spec.toggle.x1, spec.toggle.x2, theme.hover_bg)
      end
      if spec.glyph then
        local glyph = cfg.position == "right" and glyphs.toggle_right or glyphs.toggle_left
        local on = plan.rows[spec.toggle.row] and plan.rows[spec.toggle.row].lit
        put(cells, spec.toggle.x or spec.toggle.x1 + 1, glyph, { fg = on and theme.accent or theme.dim }, cols)
      end
    elseif spec.kind == "space" then
      cells = new_line(cols, theme.bg, theme.fg)
    elseif spec.kind == "header" then
      cells = chrome_row(ctx, glyphs.private, g.icon_x, "Private", theme.private_accent, theme.private_accent)
    elseif spec.kind == "separator" then
      cells = new_line(cols, theme.bg, theme.fg)
      for x = g.card_x1, g.card_x2 do
        put(cells, x, glyphs.rule, { fg = theme.separator }, x)
      end
    elseif spec.kind == "card" then
      cells = card_row(spec.item, ctx, spec.st, spec.part, spec.rows_in_card, spec.spans)
    elseif spec.kind == "ghost" then
      if spec.shape == "rail" then
        cells = new_line(cols, spec.hovered and theme.hover_bg or theme.bg, theme.fg)
        put(cells, g.icon_x, glyphs.new_tab, { fg = theme.accent }, cols)
      elseif spec.shape == "card" then
        cells = ghost_rows(ctx, spec.hovered)[spec.index]
      else
        cells = chrome_row(
          ctx,
          glyphs.new_tab,
          g.icon_x,
          cfg.new_tab_label,
          spec.hovered and theme.fg or theme.new_tab_fg,
          theme.accent,
          spec.hovered and theme.hover_bg or theme.bg
        )
      end
    elseif spec.kind == "footer" then
      local entry = spec.entry
      local fg = entry.fg or theme.meta_fg or theme.dim
      local bg = entry.bg or theme.bg
      if spec.hovered then
        fg, bg = theme.hover_fg, theme.hover_bg
      end
      cells = chrome_row(ctx, entry.icon, g.icon_x, util.sanitize(entry.text or ""), fg, entry.icon_fg or fg, bg)
    end
    if cells then
      if spec.thumb then
        cells[cols] = {
          ch = glyphs.scroll,
          fg = spec.thumb_lit and theme.scroll_fg or (theme.scroll_idle_fg or theme.scroll_fg),
          bg = theme.bg,
        }
      end
      if view.drag and view.drag.outside and spec.kind ~= "strip" and spec.kind ~= "footer" then
        local edge = cfg.position == "right" and 1 or cols
        cells[edge] = { ch = " ", fg = theme.fg, bg = theme.accent }
      end
      painted[row] = cells
      fades[row] = spec.fade
    end
  end

  if layout.framed(cfg) then
    M.frame_edge(painted, cols, theme, glyphs, view.rows)
  end
  local frame = {
    cells = painted,
    fades = fades,
    hits = plan.hits,
    cols = cols,
    rows = view.rows,
    theme = theme,
    grid = g,
    total_rows = plan.total,
    scroll = plan.scroll,
  }
  if view.popover then
    M.composite(frame, view.popover)
  end
  return M.paint(frame)
end

return M
