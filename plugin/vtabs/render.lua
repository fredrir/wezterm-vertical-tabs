local ansi = require "vtabs.ansi"
local util = require "vtabs.util"

local M = {}

local function style(fg, bg, extra)
  return ansi.bg(bg) .. ansi.fg(fg) .. (extra or "")
end

local function row_colors(item, theme, hovered, dragging, focused)
  if dragging then
    return theme.drag_fg, theme.drag_bg
  end
  if item.is_active then
    return theme.active_fg, theme.active_bg
  end
  if hovered or focused then
    return theme.hover_fg, theme.hover_bg
  end
  return theme.fg, theme.bg
end

local function accent_for(item, theme)
  return item.is_private and theme.private_accent or theme.accent
end

---Builds one tab row: returns the escaped text and the close-button column span.
local function tab_row(item, ctx, hovered, dragging, focused)
  local theme, cfg, cols = ctx.theme, ctx.cfg, ctx.cols
  local fg, bg = row_colors(item, theme, hovered, dragging, focused)
  local base = style(fg, bg)
  local left_pad = cfg.padding.left
  local right_pad = cfg.padding.right

  local show_close = cfg.close_button == "always" or (cfg.close_button == "hover" and hovered)
  if item.is_pinned and cfg.pinned_style == "compact" then
    show_close = false
  end
  local close_glyph = ctx.icons.close
  local close_w = show_close and (util.width(close_glyph) + 1) or 0

  local marker = item.is_active and "▎" or " "
  local marker_fg = item.is_active and accent_for(item, theme) or bg
  local icon = (cfg.icons and item.icon ~= "" and item.icon) or ""
  local icon_w = icon ~= "" and (util.width(icon) + 1) or 0
  local prefix_w = left_pad + 1 + icon_w
  local index = cfg.show_index and string.format("%d ", item.index) or ""

  local unseen = item.has_unseen and not item.is_active and ctx.icons.unseen or nil
  local unseen_w = (unseen and not show_close) and (util.width(unseen) + 1) or 0
  local title_budget = cols - prefix_w - close_w - unseen_w - right_pad - util.width(index)
  local title = util.truncate(item.title, math.max(title_budget, 0), cfg.ellipsis)

  local parts = {
    base,
    string.rep(" ", left_pad),
    ansi.fg(marker_fg) .. marker .. ansi.fg(fg),
  }
  if icon ~= "" then
    local icon_fg = item.is_private and theme.private_accent or fg
    parts[#parts + 1] = ansi.fg(icon_fg) .. icon .. " " .. ansi.fg(fg)
  end
  parts[#parts + 1] = index
  parts[#parts + 1] = (item.is_active and ansi.bold(true) or "") .. title .. ansi.bold(false)
  local used = prefix_w + util.width(index) + util.width(title)
  local close_span = nil
  if unseen_w > 0 then
    local fill = math.max(cols - used - right_pad - unseen_w, 0)
    parts[#parts + 1] = string.rep(" ", fill + 1) .. ansi.fg(theme.unseen_fg) .. unseen .. ansi.fg(fg)
    used = used + fill + unseen_w
  end
  if show_close then
    local fill = cols - used - right_pad - close_w
    local from = used + math.max(fill, 0) + 2
    parts[#parts + 1] = string.rep(" ", math.max(fill, 0)) .. " "
    local close_fg = (hovered and ctx.hover and ctx.hover.x >= from) and theme.close_hover_fg or theme.close_fg
    parts[#parts + 1] = ansi.fg(close_fg) .. close_glyph .. ansi.fg(fg)
    used = from + util.width(close_glyph) - 1
    close_span = { from = from, to = from + util.width(close_glyph) - 1 }
  end
  parts[#parts + 1] = string.rep(" ", math.max(cols - used, 0)) .. ansi.RESET
  return table.concat(parts), close_span
end

local function text_row(text, fg, bg, cols, left_pad)
  local body = string.rep(" ", left_pad) .. text
  return style(fg, bg) .. util.pad_right(util.truncate(body, cols, ""), cols) .. ansi.RESET
end

local function blank_row(bg, cols)
  return ansi.bg(bg) .. string.rep(" ", cols) .. ansi.RESET
end

---Reorders items so the dragged one sits at the hovered slot.
local function apply_drag(items, drag)
  if not drag or not drag.over_index then
    return items
  end
  local out = {}
  local dragged = nil
  for _, item in ipairs(items) do
    if item.tab_id == drag.tab_id then
      dragged = item
    else
      out[#out + 1] = item
    end
  end
  if not dragged then
    return items
  end
  local target = math.max(1, math.min(drag.over_index, #out + 1))
  table.insert(out, target, dragged)
  return out
end

local function partition(items)
  local pinned, rest = {}, {}
  for _, item in ipairs(items) do
    if item.is_pinned then
      pinned[#pinned + 1] = item
    else
      rest[#rest + 1] = item
    end
  end
  return pinned, rest
end

---Renders the sidebar. `view` = { cols, rows, items, theme, cfg, icons, hover, drag, scroll, focus_index, footer }.
---Returns { data = ansi string, hits = { [row] = hit }, total_rows = n, scroll = clamped offset }.
function M.render(view)
  local cfg, theme, cols = view.cfg, view.theme, view.cols
  local lines = {}
  local function push(text, hit)
    lines[#lines + 1] = { text = text, hit = hit }
  end

  for _ = 1, cfg.padding.top do
    push(blank_row(theme.bg, cols), { kind = "space" })
  end

  local items = apply_drag(view.items, view.drag)
  local pinned, rest = partition(items)
  local hovered_row = view.hover and view.hover.y or nil
  local slot = 0

  local function emit_tab(item)
    slot = slot + 1
    item.slot = slot
    local row_index = #lines + 1 - (view.scroll or 0)
    local hovered = hovered_row == row_index
    local dragging = view.drag and view.drag.tab_id == item.tab_id and view.drag.active
    local focused = view.focus_index == slot
    local ctx = { theme = theme, cfg = cfg, cols = cols, icons = view.icons, hover = hovered and view.hover or nil }
    local text, close = tab_row(item, ctx, hovered, dragging, focused)
    push(text, { kind = "tab", tab_id = item.tab_id, slot = slot, close = close, pinned = item.is_pinned })
    for _ = 1, cfg.row_gap do
      push(blank_row(theme.bg, cols), { kind = "space" })
    end
  end

  for _, item in ipairs(pinned) do
    emit_tab(item)
  end
  if #pinned > 0 and #rest > 0 then
    local sep = string.rep("─", math.max(cols - cfg.padding.left - cfg.padding.right, 1))
    push(text_row(sep, theme.separator, theme.bg, cols, cfg.padding.left), { kind = "separator" })
  end
  for _, item in ipairs(rest) do
    emit_tab(item)
  end

  if cfg.new_tab_button then
    local hovered = hovered_row == #lines + 1 - (view.scroll or 0)
    local fg = hovered and theme.hover_fg or theme.new_tab_fg
    local bg = hovered and theme.hover_bg or theme.bg
    local label = " " .. view.icons.new_tab .. " " .. cfg.new_tab_label
    push(text_row(label, fg, bg, cols, cfg.padding.left), { kind = "new_tab" })
  end

  for _, text in ipairs(view.footer or {}) do
    push(text_row(text, theme.dim, theme.bg, cols, cfg.padding.left), { kind = "footer" })
  end

  local total = #lines
  local max_scroll = math.max(total - view.rows, 0)
  local scroll = math.max(0, math.min(view.scroll or 0, max_scroll))
  if view.ensure_visible then
    for i, line in ipairs(lines) do
      if line.hit.kind == "tab" and line.hit.tab_id == view.ensure_visible then
        if i <= scroll then
          scroll = i - 1
        elseif i > scroll + view.rows then
          scroll = i - view.rows
        end
        break
      end
    end
  end

  local out = { ansi.HIDE_CURSOR }
  local hits = {}
  for row = 1, view.rows do
    local line = lines[row + scroll]
    out[#out + 1] = ansi.cup(row, 1)
    if line then
      out[#out + 1] = line.text
      hits[row] = line.hit
    else
      out[#out + 1] = blank_row(theme.bg, cols)
      hits[row] = { kind = "space" }
    end
  end
  out[#out + 1] = ansi.RESET

  return { data = table.concat(out), hits = hits, total_rows = total, scroll = scroll, slots = slot }
end

return M
