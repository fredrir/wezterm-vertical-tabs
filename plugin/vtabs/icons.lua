local wezterm = require "wezterm" ---@type Wezterm
local util = require "vtabs.util"

local M = {}

local nf = wezterm.nerdfonts or {}

local function glyph(name, fallback)
  return nf[name] or fallback
end

M.defaults = {
  default = glyph("cod_terminal", ">"),
  zsh = glyph("dev_terminal", "$"),
  bash = glyph("dev_terminal", "$"),
  fish = glyph("md_fish", "$"),
  nu = glyph("dev_terminal", "$"),
  sh = glyph("dev_terminal", "$"),
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
  unseen = glyph("md_circle_medium", "*"),
  drag = glyph("md_drag_vertical", "="),
}

local function process_key(pane)
  local ok, name = pcall(function()
    return pane:get_foreground_process_name()
  end)
  if not ok or not name or name == "" then
    return nil
  end
  local base = util.basename(name)
  if base then
    base = base:gsub("^%-", "")
  end
  return base
end

---Picks an icon for a pane from its foreground process, with user overrides.
function M.for_pane(pane, icon_map)
  local map = util.merge(M.defaults, icon_map or {})
  local key = process_key(pane)
  if key and map[key] then
    return map[key]
  end
  if key then
    for pattern, icon in pairs(icon_map or {}) do
      if type(pattern) == "string" and pattern:find "[%^%$%*%+%?%[]" and key:match(pattern) then
        return icon
      end
    end
  end
  return map.default
end

function M.get(name, icon_map)
  local map = util.merge(M.defaults, icon_map or {})
  return map[name] or ""
end

return M
