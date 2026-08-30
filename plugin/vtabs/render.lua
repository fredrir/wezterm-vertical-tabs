local ansi = require "vtabs.ansi"
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
  return a.fg[1] == b.fg[1]
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
      out[#out + 1] = ansi.bg(run.bg)
        .. (blank and "" or ansi.fg(faded(run.fg, page_bg, fade)))
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
local function grid(cfg, cols)
  local pad_l = math.max(cfg.padding.left or 0, 0)
  local pad_r = math.max(cfg.padding.right or 0, 0)
  local g = {}
  g.card_x1 = pad_l + 1
  g.card_x2 = math.max(cols - pad_r, g.card_x1)
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

local function marker(item, theme, st, glyphs)
  local accent = item.is_private and theme.private_accent or theme.accent
  if item.is_active or st.dragging then
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

local function on_surface(item, st)
  return st.dragging or st.focused or item.is_active or st.hovered
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

---Path-shaped meta keeps its tail: two siblings must not both collapse to their shared parent.
local function fit_meta(text, budget, glyphs)
  if util.width(text) <= budget then
    return text
  end
  local sep = " " .. glyphs.meta_sep .. " "
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

---Renders one row of a card and the sub-targets on it.
local function card_row(item, ctx, st, part, rows_in_card)
  local theme, cfg, glyphs, g, cols = ctx.theme, ctx.cfg, ctx.glyphs, ctx.grid, ctx.cols
  local cells = new_line(cols, theme.bg, theme.fg)
  if part == "gap" then
    return cells, nil
  end
  local fg, bg = row_colors(item, theme, st)
  local surface = on_surface(item, st)
  fill(cells, g.card_x1, g.card_x2, bg)

  local mark, mark_fg = marker(item, theme, st, glyphs)
  if part == "title" then
    put(cells, g.gutter, mark, { fg = mark_fg }, g.gutter)
  elseif item.is_active or st.dragging then
    put(cells, g.gutter, glyphs.active, { fg = mark_fg }, g.gutter)
  end

  local spans = nil
  if part == "title" then
    if cfg.icons and item.icon ~= "" then
      put(cells, g.icon_x, item.icon, { fg = item.is_private and theme.private_accent or fg }, g.icon_x)
    end
    local title = util.truncate(item.title, g.title_budget, glyphs.ellipsis)
    put(cells, g.title_x1, title, { fg = fg, bold = item.is_active }, g.title_x2)
    local pin_only = item.is_pinned and cfg.pinned_style ~= "full"
    local glyph, glyph_fg, span_id
    if pin_only then
      if cfg.pinned_style == "compact" or st.hovered then
        glyph, glyph_fg, span_id = glyphs.pinned, theme.pinned_fg, "pin"
      end
    elseif shows_close(item, cfg, st) then
      local on_close = st.pointer_x ~= nil and st.pointer_x >= g.close_x1 and st.pointer_x <= g.close_x2
      glyph = glyphs.close
      glyph_fg = (st.hovered and on_close) and theme.close_hover_fg or theme.close_fg
      span_id = "close"
    end
    if glyph then
      put(cells, g.close_x, glyph, { fg = glyph_fg }, g.close_x)
      spans = { { id = span_id, x1 = g.close_x1, x2 = g.close_x2 } }
    end
  else
    local meta = item.meta or ""
    if cfg.show_index then
      meta = string.format("%d %s %s", item.index, glyphs.meta_sep, meta)
    end
    -- meta_fg is gated against page and card only, never against drag_bg
    local meta_fg = st.dragging and theme.drag_fg or theme.meta_fg or theme.dim
    put(cells, g.meta_x1, fit_meta(meta, g.meta_budget, glyphs), { fg = meta_fg }, g.meta_x2)
    if shows_close(item, cfg, st) and not item.is_pinned then
      spans = { { id = "close", x1 = g.close_x1, x2 = g.close_x2 } }
    end
  end

  if surface and rows_in_card >= 2 and glyphs.corners == "chamfer" then
    local ch = part == "title" and glyphs.chamfer_top or glyphs.chamfer_bottom
    cells[g.card_x2] = { ch = ch, fg = bg, bg = theme.bg }
  end
  return cells, spans
end

local function chrome_row(ctx, glyph, glyph_x, text, text_fg, glyph_fg, bg)
  local theme, g, cols = ctx.theme, ctx.grid, ctx.cols
  local cells = new_line(cols, bg or theme.bg, theme.fg)
  if glyph and glyph ~= "" then
    put(cells, glyph_x, glyph, { fg = glyph_fg or text_fg }, glyph_x)
  end
  if text and text ~= "" then
    put(cells, g.title_x1, util.truncate(text, math.max(g.card_x2 - g.title_x1 + 1, 0), ctx.glyphs.ellipsis), {
      fg = text_fg,
    }, g.card_x2)
  end
  return cells
end

---The only outlined element in the sidebar: that is what makes it read as "not a tab".
local function ghost_rows(ctx, hovered)
  local theme, glyphs, g, cols = ctx.theme, ctx.glyphs, ctx.grid, ctx.cols
  local border_fg = hovered and theme.accent or (theme.border_idle or theme.separator)
  local line = hovered and glyphs.frame_h or glyphs.frame_dash
  local inner_bg = hovered and theme.hover_bg or theme.bg
  local rows = {}
  for i = 1, 3 do
    local cells = new_line(cols, theme.bg, theme.fg)
    if i == 2 then
      fill(cells, g.card_x1 + 1, g.card_x2 - 1, inner_bg)
      put(cells, g.card_x1, glyphs.frame_v, { fg = border_fg }, g.card_x1)
      put(cells, g.icon_x, glyphs.new_tab, { fg = theme.accent }, g.icon_x)
      local label = util.truncate(ctx.cfg.new_tab_label, math.max(g.card_x2 - 1 - g.title_x1, 0), glyphs.ellipsis)
      put(cells, g.title_x1, label, { fg = hovered and theme.fg or theme.new_tab_fg }, g.card_x2 - 1)
      put(cells, g.card_x2, glyphs.frame_v, { fg = border_fg }, g.card_x2)
    else
      put(cells, g.card_x1, i == 1 and glyphs.frame_tl or glyphs.frame_bl, { fg = border_fg }, g.card_x1)
      for x = g.card_x1 + 1, g.card_x2 - 1 do
        put(cells, x, line, { fg = border_fg }, x)
      end
      put(cells, g.card_x2, i == 1 and glyphs.frame_tr or glyphs.frame_br, { fg = border_fg }, g.card_x2)
    end
    rows[i] = cells
  end
  return rows
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
local function new_tab_rows(cfg, rows, strip_rows, footer_n)
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

---Renders the sidebar.
---@param view VtabsRenderInput
---@return table `{ data, hits = { [row] = VtabsHit }, total_rows, scroll }`
function M.render(view)
  local cfg, theme, cols = view.cfg, view.theme, view.cols
  local glyphs = view.glyphs
  local g = grid(cfg, cols)
  local ctx = { theme = theme, cfg = cfg, cols = cols, glyphs = glyphs, grid = g }
  local strip_rows = view.strip and math.max(view.strip.rows or 0, 0) or math.max(cfg.padding.top or 0, 0)
  strip_rows = math.min(strip_rows, view.rows)
  local footer = {}
  for _, entry in ipairs(view.footer or {}) do
    footer[#footer + 1] = footer_entry(entry)
  end
  local ghost_h = new_tab_rows(cfg, view.rows, strip_rows, #footer)
  local list_rows = math.max(view.rows - strip_rows - ghost_h - #footer, 0)

  local ordered = apply_drag(view.items, view.drag)
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
    local dense = item.is_pinned and cfg.pinned_style ~= "full"
    local armed_dense = item.armed_pinned ~= nil and item.armed_pinned and cfg.pinned_style ~= "full"
    local rows_in_card = (dense or (cfg.meta == false)) and 1 or 2
    if item.armed_pinned ~= nil then
      rows_in_card = (armed_dense or (cfg.meta == false)) and 1 or 2
    end
    plan[#plan + 1] = { kind = "tab", item = item, slot = slot, part = "title", rows_in_card = rows_in_card }
    if rows_in_card == 2 then
      plan[#plan + 1] = { kind = "tab", item = item, slot = slot, part = "meta", rows_in_card = rows_in_card }
    end
    if not dense or item.armed_pinned ~= nil then
      for _ = 1, cfg.row_gap do
        plan[#plan + 1] = { kind = "tab", item = item, slot = slot, part = "gap", rows_in_card = rows_in_card }
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

  local out = { ansi.HIDE_CURSOR }
  local hits = {}
  local rows_text, rows_n = {}, 0
  ---Each row is self-contained -- SGR, cells, reset -- so it can be re-sent on its own.
  local function line(row, cells, fade)
    local body = emit(cells, theme.bg, fade)
    rows_text[row] = body
    rows_n = math.max(rows_n, row)
    out[#out + 1] = ansi.cup(row, 1) .. body
  end

  for row = 1, strip_rows do
    local cells = new_line(cols, theme.bg, theme.fg)
    local toggle = view.strip and view.strip.toggle
    if toggle and row == toggle.row then
      local glyph = cfg.position == "right" and glyphs.toggle_right or glyphs.toggle_left
      local on = view.hover and view.hover.y >= toggle.row and view.hover.y <= math.min(toggle.row + 1, strip_rows)
      on = on and view.hover.x >= toggle.x1 and view.hover.x <= toggle.x2
      put(cells, toggle.x or toggle.x1 + 1, glyph, { fg = on and theme.accent or theme.dim }, cols)
    end
    if toggle and row >= toggle.row and row <= math.min(toggle.row + 1, strip_rows) then
      hits[row] = { kind = "toggle", x1 = toggle.x1, x2 = toggle.x2 }
    else
      hits[row] = { kind = "strip" }
    end
    line(row, cells)
  end

  for i = 1, list_rows do
    local row = strip_rows + i
    local entry = plan[i + scroll]
    local cells, hit
    if not entry or entry.kind == "space" then
      cells, hit = new_line(cols, theme.bg, theme.fg), { kind = "space" }
    elseif entry.kind == "header" then
      cells = chrome_row(ctx, glyphs.private, g.icon_x, "Private", theme.private_accent, theme.private_accent)
      hit = { kind = "space" }
    elseif entry.kind == "separator" then
      cells = new_line(cols, theme.bg, theme.fg)
      for x = g.card_x1, g.card_x2 do
        put(cells, x, glyphs.rule, { fg = theme.separator }, x)
      end
      hit = { kind = "separator" }
    else
      local item = entry.item
      local st = {
        hovered = hovered_id == item.tab_id,
        dragging = view.drag and view.drag.active and view.drag.tab_id == item.tab_id or false,
        focused = view.focus_index == entry.slot,
        pointer_x = view.hover and view.hover.x or nil,
      }
      local spans
      cells, spans = card_row(item, ctx, st, entry.part, entry.rows_in_card or 1)
      hit = {
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
    local fade = nil
    if (i == fade_first and scroll > 0) or (i == fade_last and scroll < max_scroll) then
      fade = 0.5
    end
    if view.drag and view.drag.outside then
      local edge = cfg.position == "right" and 1 or cols
      cells[edge] = { ch = " ", fg = theme.fg, bg = theme.accent }
    end
    local thumb = cfg.scroll_indicator ~= "never" and cfg.scroll_indicator ~= false and total > list_rows
    if thumb then
      local len = math.max(1, math.floor(list_rows * list_rows / total))
      local start = 1 + math.floor(scroll * (list_rows - len) / math.max(max_scroll, 1) + 0.5)
      if i >= start and i < start + len then
        local lit = cfg.scroll_indicator == "always"
          or view.hover ~= nil
          or (view.drag and view.drag.active)
          or view.user_scrolled
        cells[cols] = {
          ch = glyphs.scroll,
          fg = lit and theme.scroll_fg or (theme.scroll_idle_fg or theme.scroll_fg),
          bg = theme.bg,
        }
      end
    end
    line(row, cells, fade)
    hits[row] = hit
  end

  if ghost_h > 0 then
    local base = strip_rows + list_rows
    local hovered = view.hover and view.hover.y > base and view.hover.y <= base + ghost_h or false
    if ghost_h == 3 then
      local rows = ghost_rows(ctx, hovered)
      for i = 1, 3 do
        if base + i <= view.rows then
          line(base + i, rows[i])
          hits[base + i] = { kind = "new_tab", x1 = g.card_x1, x2 = g.card_x2 }
        end
      end
    else
      local cells = chrome_row(
        ctx,
        glyphs.new_tab,
        g.icon_x,
        cfg.new_tab_label,
        hovered and theme.fg or theme.new_tab_fg,
        theme.accent,
        hovered and theme.hover_bg or theme.bg
      )
      if base + 1 <= view.rows then
        line(base + 1, cells)
        hits[base + 1] = { kind = "new_tab", x1 = g.card_x1, x2 = g.card_x2 }
      end
    end
  end

  for i, entry in ipairs(footer) do
    local row = view.rows - #footer + i
    if row >= 1 then
      local fg = entry.fg or theme.meta_fg or theme.dim
      local bg = entry.bg or theme.bg
      if view.hover and view.hover.y == row and entry.id then
        fg, bg = theme.hover_fg, theme.hover_bg
      end
      local cells = chrome_row(ctx, entry.icon, g.icon_x, util.sanitize(entry.text or ""), fg, entry.icon_fg or fg, bg)
      line(row, cells)
      hits[row] = { kind = "footer", id = entry.id, entry = entry, x1 = g.card_x1, x2 = g.card_x2 }
    end
  end
  out[#out + 1] = ansi.RESET

  return {
    data = table.concat(out),
    rows = rows_text,
    rows_n = rows_n,
    hits = hits,
    total_rows = total,
    scroll = scroll,
  }
end

return M
