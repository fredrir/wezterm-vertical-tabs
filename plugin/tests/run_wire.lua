local H = require "support.helpers"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local view = require "vtabs.view"
local wire = require "vtabs.wire"

local test, eq = H.test, H.eq
local ready_window = H.ready_window

local function v2_lines(sb, kind)
  local out = {}
  for _, sent in ipairs(sb.sent) do
    local line = sent:match "^({.*})\n$"
    if line and line:find('^{"t":"' .. kind .. '"') then
      out[#out + 1] = line
    end
  end
  return out
end

local function decode(line)
  return require("wezterm").json_parse(line)
end

test("wire: a v2 sidebar receives config, theme and model; a v1 one receives none", function()
  local win, gui = ready_window(2)
  local sb1 = sidebar.find(win.tab_list[1])
  local sb2 = sidebar.find(win.tab_list[2])
  state.session.proto[sb1:pane_id()] = 2
  view.sync(gui, { force = true })
  for _, kind in ipairs { "config", "theme", "model" } do
    eq(#v2_lines(sb1, kind), 1, kind .. " sent once to the v2 pane")
    eq(#v2_lines(sb2, kind), 0, kind .. " never sent to the v1 pane")
  end
  local model = decode(v2_lines(sb1, "model")[1])
  eq(model.rev, 1)
  eq(#model.tabs, 2)
  eq(model.tabs[2].title, win.tab_list[2].title)
  eq(model.screen, "sidebar")
  local config_msg = decode(v2_lines(sb1, "config")[1])
  eq(config_msg.desired_width, 28)
  eq(type(config_msg.render.padding.left), "number")
end)

test("wire: an unchanged window sends nothing again; a change bumps only that message's rev", function()
  local win, gui = ready_window(2)
  local sb = sidebar.find(win.tab_list[1])
  state.session.proto[sb:pane_id()] = 2
  view.sync(gui, { force = true })
  local before = #sb.sent
  view.sync(gui, { force = true })
  eq(#v2_lines(sb, "model"), 1, "identical model deduped")
  eq(#v2_lines(sb, "config"), 1, "identical config deduped")
  win.tab_list[2].title = "renamed"
  view.sync(gui, { force = true })
  local models = v2_lines(sb, "model")
  eq(#models, 2, "a retitle resends the model")
  eq(decode(models[2]).rev, 2, "and bumps its rev")
  eq(#v2_lines(sb, "config"), 1, "config stays deduped")
  assert(#sb.sent > before, "something new was sent")
end)

test("wire: the encoder is deterministic and arrays stay arrays when empty", function()
  local a = wire.encode { b = 1, a = { 2, 3 }, c = { x = true } }
  eq(a, '{"a":[2,3],"b":1,"c":{"x":true}}')
  eq(wire.encode(wire.array {}), "[]")
  eq(wire.encode {}, "{}")
  eq(wire.encode { s = 'quote " and \\ back' }, '{"s":"quote \\" and \\\\ back"}')
end)

test("wire: an open popover rides the model with its row ids", function()
  local win, gui, sb = H.open_popover(3)
  state.session.proto[sb:pane_id()] = 2
  require("vtabs.view").sync(gui, { force = true })
  local models = v2_lines(sb, "model")
  local last = decode(models[#models])
  assert(last.popover, "the bridge is on the wire")
  assert(last.popover.h >= 3)
  local ids = {}
  for _, row in ipairs(last.popover.rows) do
    ids[#ids + 1] = tostring(row.id)
  end
  assert(table.concat(ids, ","):find "close", "row identity crosses with the spans")
end)
