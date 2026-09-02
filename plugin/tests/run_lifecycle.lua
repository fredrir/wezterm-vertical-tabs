---The pane lifecycle under interleaving: handlers as tasks, a mux that answers late, two windows.
local H = require "support.helpers"
local async = require "support.async"
local fake = require "fake_mux"
local wezterm = require "wezterm"
local rescue = require "vtabs.sidebar_rescue"
local sidebar = require "vtabs.sidebar"
local util = require "vtabs.util"
local test, eq = H.test, H.eq

test("a deferred split leaves the tree alone until the task resumes", function()
  local win = H.window(1)
  local tab = win.tab_list[1]
  fake.deferred = true
  local task = async.spawn(function()
    return tab.pane_list[1]:split { direction = "Left", size = 28, args = { "wez-vtabs" } }
  end)
  eq(#tab:panes(), 1)
  eq(task.tag, "split")
  async.run()
  fake.deferred = false
  eq(#tab:panes(), 2)
  eq(task.results[1], tab.pane_list[1])
end)

test("the fake cli answers only on the GUI's own socket", function()
  local win = H.window(1, { attach = true, ready = true })
  local tab = win.tab_list[1]
  local sb = sidebar.find(tab)
  eq(rescue.cli_kill(sb:pane_id()), false)
  eq(#tab:panes(), 2)
  H.with_cli(function()
    eq(rescue.cli_kill(sb:pane_id()), true)
  end)
  eq(#tab:panes(), 1)
  local last = wezterm.spawned[#wezterm.spawned]
  eq(last[4], "kill-pane")
  eq(last[6], tostring(sb:pane_id()))
end)

test("the clock only moves by hand", function()
  local clock = H.clock()
  local t0 = util.now_ms()
  eq(clock.advance(1500), t0 + 1500)
  eq(util.now_ms(), t0 + 1500)
  clock.restore()
end)

test("a moved tab lives in the other window and both windows are listed", function()
  local win = H.window(2)
  local dest = fake.window()
  local tab = win.tab_list[2]
  fake.move_tab(win, tab, dest)
  eq(#win.tab_list, 1)
  eq(#dest.tab_list, 1)
  eq(tab:window(), dest)
  local listed = 0
  for _, w in ipairs(wezterm.mux.all_windows()) do
    if w == dest or w == win then
      listed = listed + 1
    end
  end
  eq(listed, 2)
  fake.close_window(dest)
end)
