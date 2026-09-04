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

test("a structurally valid Rust-resolved theme replaces the cached theme", function()
  local _, gui = H.window(1)
  local first = { bg = { 1, 2, 3 }, content_bg = { 4, 5, 6 } }
  assert(theme.accept(gui, { theme = first }))
  eq(theme.get(gui:window_id()), first)
  assert(not theme.accept(gui, { theme = false }))
  eq(theme.get(gui:window_id()), first, "an invalid answer is inert")
  local second = { bg = { 7, 8, 9 } }
  assert(theme.accept(gui, { theme = second }))
  eq(theme.get(gui:window_id()), second)
end)

test("a theme hook request must contain a theme", function()
  local win, gui = H.window(1)
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
  assert(not theme.answer_hook(gui, pane, {}))
  eq(calls, 0)
end)

test("each pending theme hook request runs the user hook and returns a fieldless answer", function()
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
  local ev = { theme = { bg = { 10, 20, 30 } } }
  assert(theme.answer_hook(gui, pane, ev))
  assert(theme.answer_hook(gui, pane, ev))
  eq(calls, 2)
  local answer = wezterm.json_parse(H.control_payload(pane.sent[#pane.sent]))
  eq(answer.t, "theme_hook_result")
  eq(answer.overrides.accent, "#040506")
  local fields = 0
  for _ in pairs(answer) do
    fields = fields + 1
  end
  eq(fields, 2, "the reply contains only its tag and overrides")
end)
