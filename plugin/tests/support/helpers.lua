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
function M.row_text(data, row)
  local seg = data:match("\27%[" .. row .. ";1H(.-)\27%[" .. (row + 1) .. ";1H")
    or data:match("\27%[" .. row .. ";1H(.*)$")
  seg = seg:gsub("\27%[%d+;%d+H.-\27%[0m$", "")
  return M.strip(seg)
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

function M.dump_frame(name, v)
  return M.dump_lines(name, M.frame_rows(v), v.cols)
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
