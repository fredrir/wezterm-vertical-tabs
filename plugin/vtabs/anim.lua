local ansi = require "vtabs.ansi"

local M = {}

M.MAX_DATA = 24 * 1024
M.MAX_ROWS = 128

---Spec §4.4. `stagger` is per row, capped; collapse runs bottom-to-top, expand top-to-bottom.
M.PHASES = {
  collapse_out = { ms = 160, ease = "inOutQuad", dir = "out", stagger = 8, cap = 80, reverse = true },
  collapse_in = { ms = 100, ease = "outCubic", dir = "in", stagger = 8, cap = 80, reverse = true },
  expand_out = { ms = 80, ease = "inOutQuad", dir = "out", stagger = 12, cap = 120, reverse = false },
  expand_in = { ms = 220, ease = "outCubic", dir = "in", stagger = 12, cap = 120, reverse = false },
  hover = { ms = 60, ease = "linear", dir = "in", stagger = 0, cap = 0, reverse = false },
}

local function painted_rows(frame)
  local rows = {}
  for row = 1, frame.rows_n or 0 do
    if frame.rows[row] then
      rows[#rows + 1] = row
    end
  end
  return rows
end

---Builds the backend `anim` command for one phase.
---@param phase string a key of `M.PHASES`
---@param frame table a `render.render` result: `rows` strings and `rows_n`
---@param opts table `{ id, anchor = "#rrggbb", fps, rows = { row, ... }|nil }`
---@return table|nil command, string|nil reason `"phase" | "empty" | "rows" | "size" | "anchor"`
function M.build(phase, frame, opts)
  local spec = M.PHASES[phase]
  if not spec then
    return nil, "phase"
  end
  opts = opts or {}
  if type(opts.anchor) ~= "string" or not opts.anchor:match "^#%x%x%x%x%x%x$" then
    return nil, "anchor"
  end
  local rows = opts.rows or painted_rows(frame)
  local selected = {}
  for _, row in ipairs(rows) do
    if frame.rows[row] then
      selected[#selected + 1] = row
    end
  end
  if #selected == 0 then
    return nil, "empty"
  end
  if #selected > M.MAX_ROWS then
    return nil, "rows"
  end

  local out, entries = { ansi.HIDE_CURSOR }, {}
  for i, row in ipairs(selected) do
    out[#out + 1] = ansi.cup(row, 1) .. frame.rows[row]
    local nth = spec.reverse and (#selected - i) or (i - 1)
    entries[i] = { y = row, delay = math.min(spec.stagger * nth, spec.cap) }
  end
  out[#out + 1] = ansi.RESET
  local data = table.concat(out)
  if #data > M.MAX_DATA then
    return nil, "size"
  end

  return {
    t = "anim",
    id = opts.id or 1,
    ms = spec.ms,
    fps = opts.fps or 30,
    ease = spec.ease,
    dir = spec.dir,
    anchor = opts.anchor,
    rows = entries,
    data = data,
  },
    selected
end

return M
