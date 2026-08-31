local wezterm = require "wezterm" ---@type Wezterm
local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local icons = require "vtabs.icons"
local mux = require "vtabs.mux"
local store = require "vtabs.store"
local util = require "vtabs.util"

local M = {}

local function title_for(tab, pane, cfg)
  if cfg.title then
    local ok, custom = pcall(cfg.title, tab, pane)
    if not ok then
      util.warn_once("hook-title", "title hook failed: %s", tostring(custom))
    elseif custom and custom ~= "" then
      return custom
    end
  end
  local title = tab:get_title()
  if title ~= "" and not sidebar.marker(title) then
    return title
  end
  local pane_title = mux.title(pane)
  if pane_title and pane_title ~= "" and not sidebar.marker(pane_title) then
    return pane_title
  end
  return "tab " .. tostring(tab:tab_id())
end

local SHELLS = {
  bash = true,
  fish = true,
  nu = true,
  sh = true,
  zsh = true,
  ["cmd.exe"] = true,
  ["pwsh.exe"] = true,
  ["powershell.exe"] = true,
}
local REMOTE = { ssh = true, mosh = true, ["mosh-client"] = true, ["ssh.exe"] = true }

local META_TTL_MS = 60000
---Declared through `store`, so forgetting a tab clears it without a hook to register.
local scope = store.scope "model"
local meta_cache = scope.tab()

---`file_path` prefixes a Windows drive with a slash (url-funcs/src/lib.rs:60-76); drop it.
local function local_path(path)
  if type(path) ~= "string" then
    return nil
  end
  return path:match "^/(%a:[/\\].*)$" or path
end

local function some(s)
  return type(s) == "string" and s ~= "" and s or nil
end

---Path, host and user of a pane's OSC 7 cwd; the user is the URL's, never the local `$USER`.
local function cwd_of(pane)
  local cwd = mux.cwd(pane)
  if not cwd then
    return nil, nil, nil
  end
  if type(cwd) == "string" then
    local authority, path = cwd:match "^file://([^/]*)(/.*)$"
    if not authority then
      return local_path(cwd), nil, nil
    end
    local user, host = authority:match "^([^@]*)@(.*)$"
    return local_path(path), some(host or authority), some(user)
  end
  return local_path(cwd.file_path), some(cwd.host), some(cwd.username)
end

---`~` for the user's home, so the meta line spends its 20 columns on what differs.
local function tilde(path)
  local home = some(wezterm.home_dir) or os.getenv "HOME" or os.getenv "USERPROFILE"
  if not path or not home then
    return path
  end
  if path == home then
    return "~"
  end
  local head = path:sub(1, #home + 1)
  if head == home .. "/" or head == home .. "\\" then
    return "~" .. path:sub(#home + 1)
  end
  return path
end

local function join(prefix, tail)
  if prefix and tail then
    return prefix .. config.get().meta_sep .. tail
  end
  return prefix or tail
end

---Second card line: cwd for shells, `user@host` for ssh, `proc dir` otherwise, domain when remote.
local function meta_for(pane, cfg)
  if cfg.meta == false then
    return nil
  end
  local path, host, remote_user = cwd_of(pane)
  local dir = tilde(path)
  local process = util.basename(mux.foreground(pane))
  if cfg.meta == "cwd" then
    return dir
  end
  if cfg.meta == "process" then
    return process
  end
  if not process then
    -- A mux pane reports no process (mux/src/pane.rs:331), so name where it is instead.
    local domain = mux.domain(pane)
    return join(domain ~= "local" and domain or nil, dir)
  end
  if REMOTE[process] then
    if not host then
      return process
    end
    return remote_user and remote_user .. "@" .. host or host
  end
  if SHELLS[process] then
    return dir
  end
  return join(process, dir and util.basename(dir))
end

local function cached_meta(tab_id, pane, cfg, now)
  local seen = meta_cache[tab_id]
  if seen and now - seen.at < cfg.poll_ms then
    return seen.value
  end
  local value = util.sanitize(meta_for(pane, cfg) or "")
  meta_cache[tab_id] = { value = value ~= "" and value or nil, at = now }
  return meta_cache[tab_id].value
end

M.forget_tab = scope.forget_tab

local pruned_at = 0

---Sweeping every build is pointless work on the hot path; entries only expire once per TTL.
local function prune_meta(now)
  if now - pruned_at < META_TTL_MS then
    return
  end
  pruned_at = now
  for tab_id, entry in pairs(meta_cache) do
    if now - entry.at > META_TTL_MS then
      meta_cache[tab_id] = nil
    end
  end
end

local function included(cfg, tab, mux_win)
  if not cfg.hooks.filter then
    return true
  end
  local ok, keep = pcall(cfg.hooks.filter, tab, mux_win)
  if not ok then
    util.warn_once("hook-filter", "filter hook failed: %s", tostring(keep))
    return true
  end
  return keep ~= false
end

---Builds the list of visible sidebar items for a window, in physical order.
function M.build(gui_window)
  local cfg = config.get()
  local mux_win = gui_window:mux_window()
  local private = state.is_private(gui_window:window_id())
  local now = util.now_ms()
  prune_meta(now)
  local items = {}
  for _, info in ipairs(mux_win:tabs_with_info()) do
    -- a tab that dies mid-poll drops out of the list instead of failing the whole window
    local ok, item = pcall(function()
      local tab = info.tab
      local pane = included(cfg, tab, mux_win) and sidebar.content_pane(tab) or nil
      if not pane then
        return nil
      end
      local tab_id = tab:tab_id()
      if sidebar.is_settings(pane) then
        return {
          tab_id = tab_id,
          index = info.index + 1,
          is_active = info.is_active,
          is_pinned = state.is_pinned(tab_id),
          is_private = private,
          title = "Settings",
          icon = cfg.icons and cfg.glyphs.settings or "",
          has_unseen = false,
        }
      end
      return {
        tab_id = tab_id,
        index = info.index + 1,
        is_active = info.is_active,
        is_pinned = state.is_pinned(tab_id),
        is_private = private,
        title = util.sanitize(title_for(tab, pane, cfg)),
        icon = cfg.icons and icons.for_pane(pane, cfg.glyphs) or "",
        has_unseen = mux.unseen(pane) == true,
        meta = cached_meta(tab_id, pane, cfg, now),
      }
    end)
    if ok and item then
      items[#items + 1] = item
    elseif not ok then
      util.warn_once("model-tab", "tab skipped: %s", tostring(item):match "^[^\n]*")
    end
  end
  return items
end

---Rendered order: pinned first, then the rest, both in physical order.
function M.ordered(items)
  local pinned, rest = util.partition(items, function(item)
    return item.is_pinned
  end)
  for _, item in ipairs(rest) do
    pinned[#pinned + 1] = item
  end
  return pinned
end

function M.find(items, tab_id)
  for i, item in ipairs(items) do
    if item.tab_id == tab_id then
      return item, i
    end
  end
  return nil
end

function M.ids(items)
  local ids = {}
  for i, item in ipairs(items) do
    ids[i] = item.tab_id
  end
  return ids
end

return M
