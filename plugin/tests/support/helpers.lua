---Shared test harness: the counters, the assertions, and the one builder of each kind the suite
---needs. Everything here is used by at least two `tests/run_*.lua` files; anything used by one
---stays in that file.
local config = require "vtabs.config"
local fake = require "fake_mux"
local input = require "vtabs.input"
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

function M.eq(a, b, msg)
  if a ~= b then
    error((msg or "") .. string.format(" expected %s got %s", tostring(b), tostring(a)), 2)
  end
end

function M.control_payload(framed)
  local prefix = require("vtabs.gen.protocol").CONTROL_PREFIX
  local line = type(framed) == "string" and framed:match "[^\n]+" or nil
  if not line or line:sub(1, #prefix) ~= prefix then
    return nil
  end
  local separator = line:find(" ", #prefix + 1, true)
  return separator and line:sub(separator + 1) or nil
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

---Runs `fn` as if the GUI sat on its own socket, so `wezterm cli` argvs reach `fake.cli`.
function M.with_cli(fn)
  local wezterm = require "wezterm"
  local getenv = util.getenv
  wezterm.pid, wezterm.cli = 4242, fake.cli
  util.getenv = function(name)
    if name == "WEZTERM_UNIX_SOCKET" then
      return "/tmp/gui-sock-4242"
    end
    return getenv(name)
  end
  local ok, err = pcall(fn)
  wezterm.pid, wezterm.cli, util.getenv = nil, nil, getenv
  if not ok then
    error(err, 0)
  end
end

---Runs `fn` with the inbox transport on the fake filesystem under a fixed runtime directory, and
---`localmux` known as a unix domain of this machine.
function M.with_inbox(fn)
  local transport = require "vtabs.transport"
  require("vtabs.backend").register_local_domains { unix_domains = { { name = "localmux" } } }
  local runtime_dir, fs = util.runtime_dir, transport.fs
  util.runtime_dir = function()
    return "/run/vtabs-test"
  end
  transport.fs = fake.fs
  fake.files, fake.fail, fake.lose_probe = {}, {}, false
  local ok, err = pcall(fn)
  util.runtime_dir, transport.fs = runtime_dir, fs
  require("vtabs.link").reset()
  if not ok then
    error(err, 0)
  end
end

---A ready window whose every pane sits on the `localmux` domain, as a mux client shows it.
function M.mux_window(n)
  local win, gui = M.window(n or 1, { attach = true, ready = true })
  for _, tab in ipairs(win.tab_list) do
    for _, p in ipairs(tab.pane_list) do
      p.domain = "localmux"
    end
    require("vtabs.store").pane_domain[sidebar.find(tab):pane_id()] = "localmux@"
  end
  return win, gui
end

---A clock the test moves by hand; every poll after `advance` sees a later now.
function M.clock()
  local real = util.now_ms
  local now = real()
  util.now_ms = function()
    return now
  end
  return {
    advance = function(ms)
      now = now + ms
      return now
    end,
    restore = function()
      util.now_ms = real
    end,
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
  input.handle(gui, sb, "vtabs", '{"t":"intent","a":"open_menu","tab_id":' .. tab.id .. ',"row":3,"col":5}')
  require("vtabs.view").sync(gui)
  return win, gui, sb, popover.get(gui:window_id())
end

return M
