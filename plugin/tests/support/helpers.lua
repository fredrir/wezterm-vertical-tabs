---Shared test harness: the counters, the assertions, and the one builder of each kind the suite
---needs. Everything here is used by at least two `tests/run_*.lua` files; anything used by one
---stays in that file.
local config = require "vtabs.config"
local fake = require "fake_mux"
local input = require "vtabs.input"
local platform = require "vtabs.platform"
local popover = require "vtabs.popover"
local sidebar = require "vtabs.sidebar"
local state = require "vtabs.state"
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

function M.palette(bg, fg)
  return { background = bg, foreground = fg, ansi = {}, brights = {} }
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
    require("vtabs.view").sync(gui)
  end
  return win, gui
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

---The same window with the context menu open for tab `index`, asked for the way a painting
---backend asks: the open_menu verb from the first tab's sidebar.
function M.open_popover(index)
  local win, gui = M.ready_window()
  local sb = sidebar.find(win.tab_list[1])
  local tab = win.tab_list[index or 1]
  input.handle(gui, sb, "vtabs", '{"t":"do","a":"open_menu","id":' .. tab.id .. ',"args":{"row":3,"col":5}}')
  require("vtabs.view").sync(gui)
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
