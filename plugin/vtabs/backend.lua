local wezterm = require "wezterm" ---@type Wezterm
local platform = require "vtabs.platform"
local version = require "vtabs.version"
local util = require "vtabs.util"

local M = {}

M.root = nil

local script_cache = nil

---Bootstrap source, embedded so remote hosts can run it without the plugin checkout.
local function bootstrap_script()
  if script_cache then
    return script_cache
  end
  local f = M.root and io.open(M.root .. "/bin/bootstrap.sh", "r")
  if not f then
    return nil
  end
  script_cache = f:read "a"
  f:close()
  return script_cache
end

local machine_domains = { ["local"] = true }

---Unix domains run on this machine; ssh/tls domains do not.
function M.register_local_domains(config)
  for _, d in ipairs(config.unix_domains or {}) do
    if d.name then
      machine_domains[d.name] = true
    end
  end
end

local local_host = nil

local function this_host()
  if local_host == nil then
    local_host = util.try(wezterm.hostname) or ""
    local_host = local_host:lower():gsub("%..*$", "")
  end
  return local_host
end

---A pane is on this machine when its domain is local/unix and its cwd does not name another host.
function M.is_local(domain, host)
  if domain ~= nil and not machine_domains[domain] then
    return false
  end
  if host == nil or host == "" or host == "localhost" then
    return true
  end
  return host:lower():gsub("%..*$", "") == this_host()
end

---`backend.path` may be a string (this machine), a table keyed by host or domain, or `fun(domain, host)`.
function M.resolve_path(cfg, domain, host)
  local path = cfg.backend.path
  if type(path) == "function" then
    return util.try(path, domain, host)
  end
  if type(path) == "table" then
    return (host and path[host]) or path[domain]
  end
  if type(path) == "string" and M.is_local(domain, host) then
    return path
  end
  return nil
end

---Extra argv for a non-default role; the bootstraps forward whatever follows them to the binary.
local function role_args(role)
  if role == nil or role == "sidebar" then
    return nil
  end
  return { "--role", role }
end

local function with_role(args, role, shell_c)
  local extra = role_args(role)
  if not extra then
    return args
  end
  if shell_c then
    -- `sh -c script` takes the next word as $0, so the role needs one in front of it
    args[#args + 1] = "wez-vtabs"
  end
  for _, arg in ipairs(extra) do
    args[#args + 1] = arg
  end
  return args
end

function M.spawn_args(cfg, domain, host, role)
  local path = M.resolve_path(cfg, domain, host)
  if path then
    return with_role({ path }, role)
  end
  if M.is_local(domain, host) then
    if platform.is_windows then
      return with_role({
        "powershell",
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        M.root .. "\\bin\\bootstrap.ps1",
      }, role)
    end
    return with_role({ "sh", M.root .. "/bin/bootstrap.sh" }, role)
  end
  local script = bootstrap_script()
  if not script then
    return nil
  end
  return with_role({ "sh", "-c", script }, role, true)
end

function M.env(cfg, domain, host, bg)
  local env = {
    VTABS_USERVAR = cfg.backend.uservar,
    VTABS_REPO = cfg.backend.repo,
    VTABS_VERSION = cfg.backend.version or version,
    VTABS_BG = type(bg) == "string" and bg:match "^#%x%x%x%x%x%x$" or nil,
  }
  if M.is_local(domain, host) then
    env.VTABS_TARGET = platform.triple
    env.VTABS_SRC = M.root .. "/../backend"
    env.VTABS_BUILD = cfg.backend.build and "1" or "0"
    env.VTABS_BIN = M.resolve_path(cfg, domain, host)
  else
    env.VTABS_BUILD = "0"
  end
  return env
end

return M
