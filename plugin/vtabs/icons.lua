local wezterm = require "wezterm" ---@type Wezterm
local util = require "vtabs.util"

local M = {}

local nf = wezterm.nerdfonts or {}

local function glyph(name, fallback)
  return nf[name] or fallback
end

M.defaults = {
  default = glyph("md_console_line", ">"),
  zsh = glyph("md_console_line", "$"),
  bash = glyph("md_console_line", "$"),
  fish = glyph("md_fish", "$"),
  nu = glyph("md_console_line", "$"),
  sh = glyph("md_console_line", "$"),
  ["cmd.exe"] = glyph("cod_terminal_cmd", ">"),
  ["pwsh.exe"] = glyph("cod_terminal_powershell", ">"),
  ["powershell.exe"] = glyph("cod_terminal_powershell", ">"),
  nvim = glyph("custom_neovim", "v"),
  vim = glyph("custom_vim", "v"),
  vi = glyph("custom_vim", "v"),
  hx = glyph("md_alpha_h_box", "h"),
  ssh = glyph("md_lan_connect", "@"),
  mosh = glyph("md_lan_connect", "@"),
  docker = glyph("md_docker", "d"),
  git = glyph("dev_git", "g"),
  lazygit = glyph("dev_git", "g"),
  node = glyph("dev_nodejs_small", "n"),
  bun = glyph("md_language_javascript", "n"),
  deno = glyph("md_language_javascript", "n"),
  python = glyph("dev_python", "p"),
  python3 = glyph("dev_python", "p"),
  cargo = glyph("dev_rust", "r"),
  rustc = glyph("dev_rust", "r"),
  go = glyph("dev_go", "G"),
  make = glyph("cod_tools", "m"),
  htop = glyph("md_chart_line", "%"),
  btop = glyph("md_chart_line", "%"),
  top = glyph("md_chart_line", "%"),
  tmux = glyph("cod_terminal_tmux", "t"),
  claude = glyph("md_robot", "*"),
  mux = glyph("md_lan_connect", "@"),
  pinned = glyph("md_pin", "*"),
  private = glyph("md_incognito", "~"),
  close = glyph("cod_close", "x"),
  new_tab = glyph("cod_add", "+"),
  unseen = glyph("md_circle_medium", "•"),
  focus = "›",
  active = "▎",
  scroll = "▐",
}

---Merges user overrides once; patterns are kept in a stable order.
function M.resolve(icon_map)
  local map = util.merge(M.defaults, icon_map or {})
  local patterns = {}
  for _, key in ipairs(util.sorted_keys(icon_map or {})) do
    if type(key) == "string" and key:find "[%^%$%*%+%?%[]" then
      patterns[#patterns + 1] = { pattern = key, icon = icon_map[key] }
    end
  end
  map.patterns = patterns
  return map
end

local function process_key(pane)
  local name = util.try(function()
    return pane:get_foreground_process_name()
  end)
  if not name or name == "" then
    return nil
  end
  local base = util.basename(name)
  return base and base:gsub("^%-", "") or nil
end

---Picks an icon for a pane from its foreground process.
function M.for_pane(pane, glyphs)
  local key = process_key(pane)
  if key then
    if glyphs[key] then
      return glyphs[key]
    end
    for _, entry in ipairs(glyphs.patterns) do
      if key:match(entry.pattern) then
        return entry.icon
      end
    end
  end
  return glyphs.default
end

return M
