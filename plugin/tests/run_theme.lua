local H = require "support.helpers"
local config = require "vtabs.config"
local state = require "vtabs.state"
local theme = require "vtabs.theme_bridge"
local wezterm = require "wezterm"

local test, eq = H.test, H.eq

test("the Lua theme boundary only normalizes typed overrides", function()
  local out = theme.overrides {
    bg = "#123456",
    accent = { 1, 2, 255 },
    elevation = 0.25,
    scrim = 2,
    ignored = false,
  }
  eq(out.bg, "#123456")
  eq(out.accent, "#0102ff")
  eq(out.elevation, 0.25)
  eq(out.scrim, nil)
  eq(out.ignored, nil)
end)

test("raw space overrides preserve representable mistakes for Rust diagnostics", function()
  local out = theme.raw_overrides {
    accent = { 1, 2, 255 },
    bg = false,
    scrim = 2,
    mystery = "value",
  }
  eq(out.accent, "#0102ff")
  eq(out.bg, false)
  eq(out.scrim, 2)
  eq(out.mystery, "value")
end)

test("a Rust-resolved generation is cached and mismatched generations are rejected", function()
  local _, gui = H.window(1)
  local first = { bg = { 1, 2, 3 }, content_bg = { 4, 5, 6 } }
  assert(theme.accept(gui, { generation = 4, theme = first }, 4))
  eq(theme.get(gui:window_id()), first)
  assert(not theme.accept(gui, { generation = 3, theme = {} }, 4))
  assert(not theme.accept(gui, { generation = 5, theme = {} }, 4), "future generation")
  eq(theme.get(gui:window_id()), first)
  assert(not theme.accept(gui, { generation = 4, theme = {} }, 4), "duplicate generation")
end)

test("a theme hook request must match the window's current wire generation", function()
  local win, gui = H.window(1, { attach = true, ready = true })
  local pane = win.tab_list[1].pane_list[1]
  local calls = 0
  config.setup {
    backend = { path = "/bin/wez-vtabs" },
    hooks = {
      theme = function()
        calls = calls + 1
        return {}
      end,
    },
  }
  require("vtabs.view").sync(gui)
  local current = assert(require("vtabs.wire").generation(gui:window_id()))
  assert(not theme.answer_hook(gui, pane, { generation = current + 1, theme = {} }))
  eq(calls, 0)
end)

test("the user theme hook runs once per window generation and every requester gets its answer", function()
  local win, gui = H.window(1)
  local pane = win.tab_list[1].pane_list[1]
  state.set_token(pane:pane_id(), "theme-test")
  local calls = 0
  config.setup {
    backend = { path = "/bin/wez-vtabs" },
    hooks = {
      theme = function(window, base)
        calls = calls + 1
        eq(window, gui)
        eq(base.bg[1], 10)
        return { accent = { 4, 5, 6 } }
      end,
    },
  }
  local ev = { generation = 9, theme = { bg = { 10, 20, 30 } } }
  assert(theme.answer_hook(gui, pane, ev))
  assert(theme.answer_hook(gui, pane, ev))
  eq(calls, 1)
  local answer = wezterm.json_parse(H.control_payload(pane.sent[#pane.sent]))
  eq(answer.t, "theme_hook_result")
  eq(answer.generation, 9)
  eq(answer.overrides.accent, "#040506")
  for generation = 10, 13 do
    assert(theme.answer_hook(gui, pane, { generation = generation, theme = ev.theme }))
  end
  assert(theme.answer_hook(gui, pane, ev))
  eq(calls, 6, "the hook cache retains only a bounded tail of generations")
end)
