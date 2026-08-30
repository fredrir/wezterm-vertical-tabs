-- luacheck: max_line_length 400
-- Every option, once. Drives config validation, the generated docs table and, later, the
-- settings UI's widget choice. Dotted keys address nested options. No wezterm dependency, so
-- `scripts/gen-docs.lua` can load it under a plain Lua interpreter.
local M = {}

M.options = {
  {
    key = "width",
    type = "number",
    default = 28,
    min = 8,
    label = "Width",
    group = "layout",
    help = "sidebar width in cells (min 8); re-asserted on the active tab after every window resize, a divider drag is adopted until the config reloads. Two-row cards give a 19-column title and a 20-column meta line at 28; raise to `32` if titles truncate too often",
  },
  {
    key = "position",
    type = "enum",
    default = "left",
    enum = { "left", "right" },
    label = "Position",
    group = "layout",
    help = '`"left"` or `"right"`',
  },
  {
    key = "hide_native_tab_bar",
    type = "boolean",
    default = true,
    label = "Hide native tab bar",
    group = "layout",
    help = "sets `enable_tab_bar = false`",
  },
  {
    key = "poll_ms",
    type = "number",
    default = 500,
    min = 50,
    label = "Poll interval",
    group = "behaviour",
    help = "upper bound for `status_update_interval`; drives sidebar refresh",
  },
  {
    key = "padding",
    type = "table",
    container = true,
    shown = "`{ top = 1, left = 1, right = 1 }`",
    label = "Padding",
    group = "layout",
    help = "cells of padding; `top` is added to the top strip, which owns the rows above the first card",
  },
  {
    key = "padding.top",
    type = "number",
    default = 1,
    min = 0,
    docs = false,
    label = "Padding top",
    group = "layout",
  },
  {
    key = "padding.left",
    type = "number",
    default = 1,
    min = 0,
    docs = false,
    label = "Padding left",
    group = "layout",
  },
  {
    key = "padding.right",
    type = "number",
    default = 1,
    min = 0,
    docs = false,
    label = "Padding right",
    group = "layout",
  },
  {
    key = "tab_height",
    type = "enum",
    default = "card",
    enum = { "card", "row" },
    alias = { [2] = "card", [1] = "row", [true] = "card", [false] = "row" },
    label = "Card height",
    group = "cards",
    help = '`"card"` (or `2`): 2 painted rows per tab; `"row"` (or `1`): 1 row, same as `meta = false`',
  },
  {
    key = "meta",
    type = "enum",
    default = "auto",
    enum = { "auto", "cwd", "process", false },
    alias = { [true] = "auto" },
    label = "Meta line",
    group = "cards",
    help = 'second card row: `"auto"` (cwd for shells, `user@host` for ssh, `proc · dir` otherwise, `domain · cwd` on a mux), `"cwd"`, `"process"`, or `false` for 1-row cards',
  },
  {
    key = "row_gap",
    type = "number",
    default = 1,
    min = 0,
    label = "Row gap",
    group = "cards",
    help = "blank rows after each card; the gap row is part of the card's click target",
  },
  {
    key = "new_tab_button",
    type = "enum",
    default = "ghost",
    enum = { "ghost", "row", false },
    alias = { [true] = "ghost" },
    label = "New tab button",
    group = "chrome",
    help = '`"ghost"`: dashed card, sticky at the bottom; `"row"`: single row; `false`: hidden. `true` = `"ghost"`',
  },
  {
    key = "new_tab_label",
    type = "string",
    default = "New tab",
    label = "New tab label",
    group = "chrome",
    help = "label inside the card",
  },
  {
    key = "corners",
    type = "enum",
    default = "chamfer",
    enum = { "chamfer", "square" },
    label = "Corners",
    group = "cards",
    help = '`"chamfer"`: quadrant-cut card corners; `"square"`. Forced to `"square"` when `custom_block_glyphs = false`',
  },
  {
    key = "titlebar",
    type = "enum",
    default = "auto",
    enum = { "auto", "integrate", "plain" },
    label = "Title bar",
    group = "chrome",
    help = '`"auto"`: reserve cells for the macOS traffic lights when the window has `INTEGRATED_BUTTONS`; `"integrate"`: always reserve; `"plain"`: never',
  },
  {
    key = "toggle_button",
    type = "boolean",
    default = true,
    label = "Toggle button",
    group = "chrome",
    help = "draw `«`/`»` in the top strip; clicking it hides the sidebar, `toggle_sidebar` brings it back",
  },
  {
    key = "close_button",
    type = "enum",
    default = "hover",
    enum = { "hover", "always", "never" },
    label = "Close button",
    group = "cards",
    help = '`"hover"` (hovered + active rows), `"always"` or `"never"`; the column is reserved so rows never reflow. The hit target is 3 columns × both card rows. Treated as `"always"` when `hover = "press"`, where no non-active row is ever hovered',
  },
  {
    key = "confirm_close",
    type = "boolean",
    default = true,
    label = "Confirm close",
    group = "behaviour",
    help = "let WezTerm prompt before closing tabs with stateful processes",
  },
  {
    key = "debug",
    type = "boolean",
    default = false,
    label = "Debug logging",
    group = "behaviour",
    help = "log backend events and hit rows via `wezterm.log_info`",
  },
  {
    key = "show_index",
    type = "boolean",
    default = false,
    label = "Show index",
    group = "cards",
    help = "prefix titles with the tab index; with two-row cards the index renders on the meta line (`1 · ~/projects/api`) so the title grid never shifts, and goes back inline with `meta = false`",
  },
  {
    key = "pinned_style",
    type = "enum",
    default = "dense",
    enum = { "dense", "compact", "full" },
    label = "Pinned style",
    group = "cards",
    help = '`"dense"`: 1-row entries, pin glyph on hover; `"compact"`: as before; `"full"`: normal 2-row cards',
  },
  {
    key = "separator",
    type = "enum",
    default = "gap",
    enum = { "rule", "gap", "none" },
    label = "Separator",
    group = "cards",
    help = 'between pinned and other tabs: `"gap"`, `"rule"` or `"none"`',
  },
  {
    key = "scroll_indicator",
    type = "enum",
    default = "auto",
    enum = { "auto", "always", "never" },
    alias = { [true] = "auto", [false] = "never" },
    label = "Scroll indicator",
    group = "chrome",
    help = 'right-edge thumb when tabs overflow: `"auto"` dims it while the sidebar is idle, `"always"`, `"never"`. `true`/`false` accepted',
  },
  {
    key = "wheel",
    type = "enum",
    default = "scroll",
    enum = { "scroll", "switch" },
    label = "Wheel",
    group = "behaviour",
    help = '`"scroll"` the list or `"switch"` tabs',
  },
  {
    key = "tear_off",
    type = "enum",
    default = true,
    enum = { true, false },
    alias = { edge = true },
    label = "Tear off",
    group = "behaviour",
    help = "drag a tab onto the sidebar's inner edge (3+ columns of travel) to move it to a new window",
  },
  {
    key = "adopt",
    type = "enum",
    default = "auto",
    enum = { "auto", true, false },
    label = "Adopt backend panes",
    group = "identity",
    help = 'take over an unmapped pane that carries the `wez-vtabs:` title marker instead of splitting a second sidebar. `"auto"`: only in a domain this plugin spawns backends in (local, already-spawned, or one `backend.path` resolves); `true`: any domain; `false`: never. See the identity table in `docs/limitations.md`',
  },
  {
    key = "window_title",
    type = "boolean",
    default = true,
    label = "Window title",
    group = "chrome",
    help = "while the sidebar is the active pane, title the window after the content pane instead. `false` leaves `format-window-title` unregistered",
  },
  {
    key = "hover",
    type = "enum",
    default = "follow",
    enum = { "follow", "press" },
    label = "Hover",
    group = "behaviour",
    help = '`"follow"`: the sidebar is the tab\'s active pane while the pointer is over it (sets `pane_focus_follows_mouse = true` when you left it unset — this is a global wezterm option); `"press"`: only from press to release',
  },
  {
    key = "hover_timeout_ms",
    type = "number",
    default = 6000,
    min = 0,
    label = "Hover timeout",
    group = "behaviour",
    help = "clear hover highlight after inactivity (`0` = never); terminals report no mouse-leave",
  },
  {
    key = "double_click_ms",
    type = "number",
    default = 400,
    min = 0,
    label = "Double click",
    group = "behaviour",
    help = "double-click on empty space opens a new tab",
  },
  {
    key = "ellipsis",
    type = "string",
    default = "…",
    label = "Ellipsis",
    group = "cards",
    help = "used when truncating titles",
  },
  {
    key = "icons",
    type = "boolean",
    default = true,
    label = "Icons",
    group = "cards",
    help = "show process icons (Nerd Font glyphs)",
  },
  {
    key = "icon_map",
    type = "table",
    default = {},
    open = true,
    label = "Icon overrides",
    group = "cards",
    help = "process name → glyph overrides; Lua patterns allowed. Also overrides UI glyphs: `close new_tab unseen pinned focus active scroll`",
  },
  {
    key = "title",
    type = "function",
    label = "Title hook",
    group = "hooks",
    help = "`fun(tab, pane): string` custom title",
  },
  {
    key = "domain",
    type = "string",
    default = "CurrentPaneDomain",
    label = "Domain",
    group = "identity",
    help = "domain the sidebar pane is spawned in",
  },
  {
    key = "skip_close_confirmation",
    type = "boolean",
    default = true,
    label = "Skip close confirmation",
    group = "behaviour",
    help = "add `wez-vtabs` to `skip_close_confirmation_for_processes_named`",
  },
  {
    key = "private",
    type = "table",
    container = true,
    docs = false,
    label = "Private",
    group = "behaviour",
  },
  {
    key = "private.env",
    type = "table",
    default = { HISTFILE = "", fish_private_mode = "1", VTABS_PRIVATE = "1" },
    open = true,
    shown = '`{ HISTFILE = "", fish_private_mode = "1", VTABS_PRIVATE = "1" }`',
    label = "Private env",
    group = "behaviour",
    help = "env for shells in private windows",
  },
  {
    key = "keys",
    type = "any",
    default = {},
    open = true,
    label = "Key overrides",
    group = "behaviour",
    help = "key overrides, see below; `false` disables all defaults",
  },
  {
    key = "theme",
    type = "table",
    container = true,
    open = true,
    shown = "`{ elevation = 0 }`",
    label = "Theme",
    group = "theme",
    help = "color overrides, see below",
  },
  {
    key = "theme.elevation",
    type = "number",
    default = 0,
    min = 0,
    max = 1,
    docs = false,
    label = "Elevation",
    group = "theme",
  },
  {
    key = "hooks",
    type = "table",
    default = {},
    container = true,
    docs = false,
    label = "Hooks",
    group = "hooks",
  },
  {
    key = "hooks.filter",
    type = "function",
    label = "Filter hook",
    group = "hooks",
    help = "`fun(tab, mux_window): boolean` hide tabs from the sidebar (navigation and reordering only touch visible tabs)",
  },
  {
    key = "hooks.footer",
    type = "function",
    label = "Footer hook",
    group = "hooks",
    help = "`fun(mux_window): (string \\| FooterEntry)[]` sticky rows at the bottom; `FooterEntry = { text, fg?, bg?, id?, on_click? = fun(window, entry) }`",
  },
  {
    key = "hooks.theme",
    type = "function",
    label = "Theme hook",
    group = "hooks",
    help = "`fun(window, theme): theme` per-window theme override",
  },
  {
    key = "hooks.route",
    type = "function",
    label = "Route hook",
    group = "hooks",
    help = "reserved for Spaces (`fun(meta): space_id`), not called yet",
  },
  {
    key = "backend",
    type = "table",
    container = true,
    docs = false,
    label = "Backend",
    group = "backend",
  },
  {
    key = "backend.path",
    type = "any",
    label = "Backend path",
    group = "backend",
    help = 'path to the `wez-vtabs` binary: string (this machine), table keyed by host or domain (`{ ["local"] = "…", archie = "…" }`) or `fun(domain, host): string?`; `host` comes from the pane\\\'s OSC 7 cwd, which is what identifies panes proxied through a mux server',
  },
  {
    key = "backend.repo",
    type = "string",
    default = "fredrir/wezterm-vertical-tabs",
    label = "Backend repo",
    group = "backend",
    help = "GitHub repo used for release downloads",
  },
  {
    key = "backend.version",
    type = "string",
    shown = "plugin version",
    label = "Backend version",
    group = "backend",
    help = "release tag to download (`v<version>`)",
  },
  {
    key = "backend.build",
    type = "boolean",
    default = true,
    label = "Backend build",
    group = "backend",
    help = "fall back to `cargo build` when no release matches",
  },
  {
    key = "backend.uservar",
    type = "string",
    default = "vtabs",
    label = "Backend user var",
    group = "backend",
    help = "user var name used by the backend",
  },
}

M.by_key = {}
for _, option in ipairs(M.options) do
  M.by_key[option.key] = option
end

---Walks a dotted key, creating tables on the way when `build` is set.
function M.at(tbl, key, build)
  local node = tbl
  local last = nil
  for part in key:gmatch "[^.]+" do
    if last then
      if node[last] == nil then
        if not build then
          return nil
        end
        node[last] = {}
      end
      node = node[last]
    end
    last = part
  end
  return node, last
end

function M.get(tbl, key)
  local node, last = M.at(tbl, key, false)
  return node and node[last]
end

function M.set(tbl, key, value)
  local node, last = M.at(tbl, key, true)
  node[last] = value
end

local function copy(value)
  if type(value) ~= "table" then
    return value
  end
  local out = {}
  for k, v in pairs(value) do
    out[k] = copy(v)
  end
  return out
end

---The default table, built from the schema so there is one source of truth.
function M.defaults()
  local out = {}
  for _, option in ipairs(M.options) do
    if option.default ~= nil then
      M.set(out, option.key, copy(option.default))
    end
  end
  return out
end

---True when `key` is inside an `open` container, whose children the schema does not enumerate.
function M.is_open(key)
  local prefix = nil
  for part in key:gmatch "[^.]+" do
    prefix = prefix and (prefix .. "." .. part) or part
    local option = M.by_key[prefix]
    if option and option.open and prefix ~= key then
      return true
    end
  end
  return false
end

return M
