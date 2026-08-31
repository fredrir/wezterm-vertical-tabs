---Shared test harness: the counters, the assertions, and the one builder of each kind the suite
---needs. Everything here is used by at least two `tests/run_*.lua` files; anything used by one
---stays in that file.
local config = require "vtabs.config"
local fake = require "fake_mux"
local glyphs = require "vtabs.glyphs"
local hit = require "vtabs.hit"
local input = require "vtabs.input"
local platform = require "vtabs.platform"
local popover = require "vtabs.popover"
local render = require "vtabs.render"
local sidebar = require "vtabs.sidebar"
local state = require "vtabs.state"
local theme = require "vtabs.theme"
local util = require "vtabs.util"

local M = {}

M.here = arg[0]:match "^(.*)[/\\]" or "."

local passed, failed = 0, 0

function M.test(name, fn)
  local ok, err = pcall(fn)
  if ok then
    passed = passed + 1
  else
    failed = failed + 1
    print("FAIL " .. name .. ": " .. tostring(err))
  end
end

function M.report()
  return passed, failed
end

function M.strip(s)
  return (s:gsub("\27%[[%d;?]*[A-Za-z]", ""))
end

function M.eq(a, b, msg)
  if a ~= b then
    error((msg or "") .. string.format(" expected %s got %s", tostring(b), tostring(a)), 2)
  end
end

function M.usub(s, i, j)
  local from = utf8.offset(s, i)
  local to = utf8.offset(s, j + 1)
  return from and s:sub(from, (to or #s + 1) - 1) or ""
end

function M.rgb(c)
  return table.concat(c, ",")
end

function M.hex(h)
  local r, g, b = h:match "^#(%x%x)(%x%x)(%x%x)$"
  return table.concat({ tonumber(r, 16), tonumber(g, 16), tonumber(b, 16) }, ",")
end

---The one row extractor: everything painted arrives as one CUP-addressed blob, so a row is the
---slice between its own cursor address and the next one.
local function row_segment(data, row)
  local seg = data:match("\27%[" .. row .. ";1H(.-)\27%[" .. (row + 1) .. ";1H")
    or data:match("\27%[" .. row .. ";1H(.*)$")
  return (seg:gsub("\27%[%d+;%d+H.-\27%[0m$", ""))
end

function M.row_text(data, row)
  return M.strip(row_segment(data, row))
end

---Extends `M.strip`: instead of discarding SGR, replays it into run-length `#fg/#bg[*]:width`
---spans (`*` = bold), in the order the renderer emitted them. One row's worth at a time.
local function styled_spans(seg)
  local spans = {}
  local fg, bg, bold = { 0, 0, 0 }, { 0, 0, 0 }, false
  local function push(text)
    local w = util.width(text)
    if w <= 0 then
      return
    end
    local key =
      string.format("#%02x%02x%02x/#%02x%02x%02x%s", fg[1], fg[2], fg[3], bg[1], bg[2], bg[3], bold and "*" or "")
    local last = spans[#spans]
    if last and last.key == key then
      last.w = last.w + w
    else
      spans[#spans + 1] = { key = key, w = w }
    end
  end
  local pos, len = 1, #seg
  while pos <= len do
    if seg:sub(pos, pos) == "\27" then
      local s, e, params = seg:find("^\27%[([%d;]*)m", pos)
      if s then
        if params == "0" then
          fg, bg, bold = { 0, 0, 0 }, { 0, 0, 0 }, false
        elseif params == "1" then
          bold = true
        elseif params == "22" then
          bold = false
        else
          local kind, r, g, b = params:match "^(%d+);2;(%d+);(%d+);(%d+)$"
          if kind == "38" then
            fg = { tonumber(r), tonumber(g), tonumber(b) }
          elseif kind == "48" then
            bg = { tonumber(r), tonumber(g), tonumber(b) }
          end
        end
        pos = e + 1
      else
        pos = pos + 1
      end
    else
      local next_esc = seg:find("\27", pos, true)
      push(seg:sub(pos, (next_esc or len + 1) - 1))
      pos = next_esc or (len + 1)
    end
  end
  local out = {}
  for _, span in ipairs(spans) do
    out[#out + 1] = span.key .. ":" .. span.w
  end
  return table.concat(out, " ")
end

local function rows_of(data, n)
  local rows = {}
  for row = 1, n do
    rows[row] = M.row_text(data, row)
  end
  return rows
end

function M.frame_rows(v)
  local r = render.render(v)
  return rows_of(r.data, v.rows), r
end

function M.page_rows(v)
  local out = require("vtabs.page").paint(v)
  return rows_of(out.data, v.rows), out
end

---First row whose hit record matches; `part` is optional. Reads the map the renderer published
---rather than assuming where a card landed.
function M.hit_row(sb, kind, id, part)
  for row, h in pairs(state.session.hits[sb:pane_id()] or {}) do
    if h.kind == kind and h.id == id and (part == nil or h.part == part) then
      return row
    end
  end
end

function M.title_row(sb, tab_id)
  return M.hit_row(sb, hit.KIND.TAB, tab_id, "title")
end

function M.popover_row(sb, id)
  return M.hit_row(sb, hit.KIND.POPOVER, id)
end

---Design frames are written only when asked for, so `just check` touches nothing outside the repo.
local FRAME_DIR = os.getenv "VTABS_DUMP_FRAMES"
M.FRAME_DIR = FRAME_DIR

function M.dump_lines(name, lines, cols)
  if not FRAME_DIR or FRAME_DIR == "" then
    return lines
  end
  os.execute("mkdir -p " .. FRAME_DIR)
  local f = io.open(FRAME_DIR .. "/" .. name .. ".txt", "w")
  if not f then
    return lines
  end
  local tens, ones = {}, {}
  for x = 1, cols do
    tens[x] = x % 10 == 0 and tostring(x // 10) or " "
    ones[x] = tostring(x % 10)
  end
  f:write(table.concat(tens), "\n", table.concat(ones), "\n")
  for _, line in ipairs(lines) do
    f:write(line, "\n")
  end
  f:close()
  return lines
end

---The styled twin of `dump_lines`: one `#fg/#bg[*]:width` run-list per row, from the same raw
---ANSI the text dump strips. See `plugin/tests/golden/README.md` for the format.
function M.dump_styled(name, data, rows_n)
  if not FRAME_DIR or FRAME_DIR == "" then
    return
  end
  os.execute("mkdir -p " .. FRAME_DIR)
  local f = io.open(FRAME_DIR .. "/" .. name .. ".styled.txt", "w")
  if not f then
    return
  end
  for row = 1, rows_n do
    f:write(string.format("%2d: %s\n", row, styled_spans(row_segment(data, row))))
  end
  f:close()
end

local SCENE_DIR = os.getenv "VTABS_DUMP_SCENES"

local function scene_json(v, indent)
  local pad = string.rep("  ", indent)
  if type(v) == "table" then
    if #v > 0 or next(v) == nil then
      local parts = {}
      for _, item in ipairs(v) do
        parts[#parts + 1] = scene_json(item, indent + 1)
      end
      return "[" .. table.concat(parts, ", ") .. "]"
    end
    local keys = {}
    for k in pairs(v) do
      if type(v[k]) ~= "function" then
        keys[#keys + 1] = k
      end
    end
    table.sort(keys)
    local parts = {}
    for _, k in ipairs(keys) do
      parts[#parts + 1] = pad .. '  "' .. k .. '": ' .. scene_json(v[k], indent + 1)
    end
    return "{\n" .. table.concat(parts, ",\n") .. "\n" .. pad .. "}"
  elseif type(v) == "number" then
    if v == math.floor(v) then
      return string.format("%d", v)
    end
    return string.format("%.17g", v)
  elseif type(v) == "boolean" then
    return tostring(v)
  end
  return '"' .. tostring(v):gsub("\\", "\\\\"):gsub('"', '\\"') .. '"'
end

---Render-relevant config only; hooks reduce to their names, frame to its boolean.
local CFG_KEYS = {
  "position",
  "new_tab_label",
  "row_gap",
  "separator",
  "tab_height",
  "meta_sep",
  "show_index",
  "icons",
  "close_button",
  "hover",
  "pinned_style",
}

---The Rust renderer's input: the same view, with Lua-owned resolution already applied.
function M.dump_scene(name, v)
  if not SCENE_DIR or SCENE_DIR == "" then
    return
  end
  local layout_mod = require "vtabs.layout"
  local cfg = {}
  for _, k in ipairs(CFG_KEYS) do
    cfg[k] = v.cfg[k]
  end
  cfg.padding = {
    left = v.cfg.padding.left,
    right = v.cfg.padding.right,
    top = v.cfg.padding.top,
    bottom = v.cfg.padding.bottom,
  }
  cfg.frame = layout_mod.framed(v.cfg)
  cfg.new_tab_button = not not v.cfg.new_tab_button
  cfg.meta = v.cfg.meta ~= false
  cfg.scroll_indicator = v.cfg.scroll_indicator == false and "never" or v.cfg.scroll_indicator
  local footer = {}
  for _, entry in ipairs(v.footer or {}) do
    footer[#footer + 1] = type(entry) == "string" and { text = entry } or entry
  end
  local pop = nil
  if v.popover then
    pop = { x = v.popover.x, y = v.popover.y, w = v.popover.w, h = v.popover.h, scrim = v.popover.scrim }
    pop.bg = v.popover.bg
    pop.rows = {}
    for i, row in ipairs(v.popover.rows or {}) do
      pop.rows[i] = { bg = row.bg, fg = row.fg, spans = row.spans }
    end
  end
  local plain_glyphs = {}
  for k, val in pairs(v.glyphs) do
    if type(val) == "string" then
      plain_glyphs[k] = val
    end
  end
  local scene = {
    cols = v.cols,
    rows = v.rows,
    rail = v.rail or false,
    items = v.items,
    theme = v.theme,
    cfg = cfg,
    glyphs = plain_glyphs,
    strip = v.strip,
    strip_buttons = layout_mod.resolved_actions(v.cfg),
    hover = v.hover,
    drag = v.drag,
    scroll = v.scroll,
    focus_index = v.focus_index,
    ensure_visible = v.ensure_visible,
    footer = footer,
    private = v.private or false,
    user_scrolled = v.user_scrolled or false,
    popover = pop,
  }
  os.execute("mkdir -p " .. SCENE_DIR)
  local f = io.open(SCENE_DIR .. "/" .. name .. ".json", "w")
  if not f then
    return
  end
  f:write(scene_json(scene, 0), "\n")
  f:close()
end

function M.dump_frame(name, v)
  local rows, r = M.frame_rows(v)
  M.dump_lines(name, rows, v.cols)
  M.dump_styled(name, r.data, v.rows)
  M.dump_scene(name, v)
  return rows
end

function M.palette(bg, fg)
  return { background = bg, foreground = fg, ansi = {}, brights = {} }
end

function M.items()
  return {
    {
      tab_id = 1,
      index = 1,
      is_active = false,
      is_pinned = true,
      title = "pinned one",
      icon = "P",
      has_unseen = false,
    },
    {
      tab_id = 2,
      index = 2,
      is_active = true,
      is_pinned = false,
      title = "active tab with a really long title",
      icon = "A",
      has_unseen = false,
    },
    { tab_id = 3, index = 3, is_active = false, is_pinned = false, title = "third", icon = "T", has_unseen = true },
  }
end

function M.p1_items()
  return {
    {
      tab_id = 1,
      index = 1,
      is_active = false,
      is_pinned = true,
      title = "dotfiles",
      meta = "~/dotfiles",
      icon = "~",
      has_unseen = false,
    },
    {
      tab_id = 2,
      index = 2,
      is_active = true,
      is_pinned = false,
      title = "wezterm-vertical-tabs",
      meta = "~/projects/wez-plugins",
      icon = "v",
      has_unseen = false,
    },
    {
      tab_id = 3,
      index = 3,
      is_active = false,
      is_pinned = false,
      title = "claude",
      meta = "~/projects/api",
      icon = "*",
      has_unseen = true,
    },
  }
end

-- These assert renderer mechanics at a fixed layout, so they state it rather than tracking defaults.
local LEGACY_LAYOUT = {
  row_gap = 0,
  padding = { top = 1, left = 1, right = 1 },
  separator = "rule",
  pinned_style = "compact",
  new_tab_button = "row",
  meta = false,
  toggle_button = false,
}

function M.legacy(opts)
  return util.merge(LEGACY_LAYOUT, opts or {})
end

---Runs `fn` with the clock moved on, so a width can be observed as having sat still since.
function M.later(ms, fn)
  local real = util.now_ms
  local base = real()
  util.now_ms = function()
    return base + ms
  end
  local ok, err = pcall(fn)
  util.now_ms = real
  if not ok then
    error(err, 0)
  end
end

---The one render-input builder. The fixture pins the geometry the positional tests count on, so
---schema default changes cannot silently move every row index. `base` picks the item set.
function M.view(over, base)
  local opts = over and over.opts or {}
  opts.row_gap = opts.row_gap or 0
  opts.separator = opts.separator or "rule"
  -- pinned to the shipped defaults after addendum 2: pad / title / pad, no gap row
  if opts.meta == nil then
    opts.meta = false
  end
  local cfg = config.setup(opts)
  local v = {
    cols = 28,
    -- one more row than the original fixture: the ghost's page row takes it, so the list keeps the
    -- seven rows every positional test in this file counts on
    rows = 11,
    items = base or M.items(),
    theme = theme.resolve({}, M.palette("#1e1e2e", "#cdd6f4")),
    cfg = cfg,
    glyphs = glyphs.resolve(cfg.glyphs, {}),
    scroll = 0,
    strip = { rows = 0 },
  }
  for k, val in pairs(over or {}) do
    if k ~= "opts" then
      v[k] = val
    end
  end
  return v
end

function M.p1_view(over)
  return M.view(over, M.p1_items())
end

---The settings page takes its own input shape: state in, frame out, no sidebar around it.
function M.page_view(over)
  over = over or {}
  local cfg = over.cfg or config.setup { backend = { path = "/bin/wez-vtabs" } }
  return {
    cols = over.cols or 100,
    rows = over.rows or 21,
    cfg = cfg,
    theme = theme.resolve({}, M.palette("#1e1e2e", "#cdd6f4")),
    glyphs = glyphs.resolve(cfg.glyphs, {}),
    st = over.st or { group = 1, focus = 1 },
    pending = over.pending,
  }
end

function M.sidebars_in(tab)
  local n = 0
  for _, p in ipairs(tab:panes()) do
    if sidebar.is_backend(p) then
      n = n + 1
    end
  end
  return n
end

---Sidebars attach on activation, so a test that wants them all has to visit every tab.
function M.attach_all(win, gui)
  local was = win.active_tab_ref
  for _, tab in ipairs(win.tab_list) do
    win.active_tab_ref = tab
    sidebar.ensure(gui)
  end
  win.active_tab_ref = was
  sidebar.ensure(gui)
end

function M.mark_ready(tab)
  local sb = sidebar.find(tab)
  sb.vars.vtabs_token = state.token_for(sb:pane_id())
  return sb
end

---The one window fixture. `attach` visits every tab so each gets a sidebar, `ready` echoes the
---tokens back, and `sync` runs one forced view pass with the first tab active.
function M.window(n, opts)
  opts = opts or {}
  config.setup { meta = "auto", backend = { path = "/bin/wez-vtabs" } }
  local win = fake.window()
  for i = 1, n or 2 do
    win:add_tab { title = "t" .. i }
  end
  local gui = win.gui
  if opts.attach then
    M.attach_all(win, gui)
  end
  if opts.ready then
    for _, tab in ipairs(win.tab_list) do
      M.mark_ready(tab)
    end
  end
  win.active_tab_ref = win.tab_list[1]
  if opts.sync then
    require("vtabs.view").sync(gui, { force = true })
  end
  return win, gui
end

function M.mouse(gui, sb, kind, button, x, y)
  input.handle(gui, sb, "vtabs", string.format('{"t":"mouse","k":"%s","b":"%s","x":%d,"y":%d}', kind, button, x, y))
end

---Presses row `y` and reports the drag, with the dwell already elapsed unless `hold` says otherwise.
function M.press_row(gui, sb, y, hold)
  M.mouse(gui, sb, "down", "left", 5, y)
  local drag = state.session.drag[gui:window_id()]
  if drag and not hold then
    drag.began = drag.began - 200
  end
  return drag
end

---The fixture most interaction tests want: every sidebar attached, ready and painted once.
function M.ready_window(n)
  return M.window(n or 3, { attach = true, ready = true, sync = true })
end

---The same window, focused on tab `index`, with its sidebar and content pane to hand.
function M.key_window(index)
  local win, gui = M.ready_window()
  win.active_tab_ref = win.tab_list[index or 1]
  local tab = win.active_tab_ref
  return win, gui, tab, sidebar.find(tab), sidebar.content_pane(tab)
end

---The same window with the context menu already open over row `row`.
function M.open_popover(row)
  local win, gui = M.ready_window()
  local sb = sidebar.find(win.tab_list[1])
  M.mouse(gui, sb, "down", "right", 5, row or 3)
  M.mouse(gui, sb, "up", "right", 5, row or 3)
  require("vtabs.view").sync(gui, { force = true })
  return win, gui, sb, popover.get(gui:window_id())
end

-- 8.4 pt cells across, 19 pt down, so 70/8.4 -> 9 cols and 28/19 -> 2 rows. No `dpi`, so 1x.
M.RETINA = { cols = 28, viewport_rows = 30, pixel_width = 235, pixel_height = 570 }

function M.strip_geom(dims, over)
  local opts = {
    is_mac = true,
    integrated_buttons = true,
    native_button_style = true,
    is_full_screen = false,
    position = "left",
    padding_top = 1,
    toggle_button = true,
    card_x1 = 2,
  }
  for k, v in pairs(over or {}) do
    opts[k] = v
  end
  return platform.strip_geometry(dims, opts)
end

return M
