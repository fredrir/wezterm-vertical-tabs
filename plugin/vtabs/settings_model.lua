local config = require "vtabs.config"
local page = require "vtabs.page"
local util = require "vtabs.util"
local version = require "vtabs.version"

local M = {}

---One field row as the wire carries it: what the widget shows is rendered here, so WIDGETS stays
---the single place a value learns to read, and Rust never re-implements a control.
local function field_body(row, st)
  row.armed = st.armed == row.key
  local option = row.option or {}
  local words = option.label or row.label:gsub("^%s+", "")
  if option.help then
    words = words .. " — " .. option.help
  end
  return {
    key = row.key,
    label = row.label,
    group = row.group,
    widget = row.widget,
    value_text = page.value_text(row),
    changed = row.changed,
    locked = row.locked and { text = row.locked } or nil,
    depth = row.depth,
    help = words,
    editing = st.editing and st.editing.key == row.key and { buffer = st.editing.buffer } or nil,
    armed = row.armed or nil,
  }
end

---The settings screen's model, one message: fields in form order, nav groups, caveat.
function M.body(cfg, st)
  cfg = cfg or config.get()
  st = st or {}
  local fields, groups = {}, {}
  local rows = page.fields(cfg)
  for _, row in ipairs(rows) do
    fields[#fields + 1] = field_body(row, st)
  end
  for _, group in ipairs(page.groups(rows)) do
    groups[#groups + 1] = { id = group, label = page.group_label(group) }
  end
  local caveat = nil
  if st.armed and page.caveat_applies() then
    caveat = page.CAVEAT
  end
  return {
    screen = "settings",
    version = version,
    groups = groups,
    fields = fields,
    caveat = caveat,
  }
end

local function row_for(cfg, key)
  for _, row in ipairs(page.fields(cfg)) do
    if row.key == key then
      return row
    end
  end
  return nil
end

---One-line text buffer, as page.lua's type_into edits it: the same three keys, byte-wise.
local function type_into(buffer, key)
  if key == "backspace" then
    return buffer:sub(1, -2)
  end
  if type(key) == "string" and utf8.len(key) == 1 then
    return buffer .. key
  end
  return buffer
end

---The settings verbs a painting backend sends; navigation never crosses, only what commits.
local ACT = {}

function ACT.activate_option(gui_window, st, args)
  local row = row_for(config.get(), args.key)
  if not row or row.locked then
    return
  end
  local widget = page.WIDGETS[row.widget]
  if not widget or not widget.activate then
    return
  end
  local what, value = widget.activate(row)
  if what == "commit" then
    page.commit(gui_window, row, value)
  elseif what == "edit" then
    st.editing = { key = row.key, buffer = value }
  elseif what == "record" then
    st.armed = row.key
  end
end

function ACT.nudge_option(gui_window, _, args)
  local row = row_for(config.get(), args.key)
  if row and not row.locked then
    page.commit(gui_window, row, page.step(row, args.delta or 1))
  end
end

function ACT.reset_option(gui_window, st, args)
  local row = row_for(config.get(), args.key)
  if row and not row.locked then
    st.armed = nil
    page.commit(gui_window, row, row.default)
  end
end

function ACT.edit_key(gui_window, st, args)
  if not st.editing then
    return
  end
  if args.key == "escape" then
    st.editing = nil
  elseif args.key == "enter" then
    local row = row_for(config.get(), st.editing.key)
    if row then
      page.commit(gui_window, row, st.editing.buffer)
    end
    st.editing = nil
  else
    st.editing.buffer = type_into(st.editing.buffer, args.key)
  end
end

function ACT.record_chord(gui_window, st, args)
  local key = st.armed
  st.armed = nil
  if not key or args.key == "escape" then
    return
  end
  local row = row_for(config.get(), key)
  if row and not row.locked then
    local mods = type(args.mods) == "table" and table.concat(args.mods, "|") or args.mods
    page.commit(gui_window, row, { key = args.key, mods = (mods ~= "" and mods) or nil })
  end
end

function ACT.settings_copy(gui_window)
  util.try(function()
    gui_window:copy_to_clipboard(page.as_lua())
  end)
end

function ACT.close_settings(gui_window)
  require("vtabs.settings").close(gui_window)
end

M.ACT = ACT

---Runs one settings verb against the window's page state; true when it was one.
function M.act(gui_window, st, name, args)
  local handler = ACT[name]
  if not handler then
    return false
  end
  handler(gui_window, st, args or {})
  return true
end

return M
