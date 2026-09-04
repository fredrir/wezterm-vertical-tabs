local wezterm = require "wezterm" ---@type Wezterm
local platform = require "vtabs.platform"
local version = require "vtabs.version"
local util = require "vtabs.util"

local M = {}

M.root = nil

local script_cache = nil

---Bootstrap source, embedded so the machine that runs the split needs no plugin checkout.
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

---Local and unix domains: reached without ssh, though a unix mux may still proxy a pane to another host.
function M.machine_domain(domain)
  return domain == nil or machine_domains[domain] == true
end

local local_host = nil

local function this_host()
  if local_host == nil then
    local_host = util.try(wezterm.hostname) or ""
    local_host = local_host:lower():gsub("%..*$", "")
  end
  return local_host
end

---A hint for transport and display, never for the command a split runs: a pane's cwd host arrives
---with its shell's first prompt, and a shell that ssh'd elsewhere names that host while its pane stays here.
function M.is_local(domain, host)
  if not M.machine_domain(domain) then
    return false
  end
  if host == nil or host == "" or host == "localhost" then
    return true
  end
  return host:lower():gsub("%..*$", "") == this_host()
end

local function sorted_keys(t)
  local keys = {}
  for key in pairs(t) do
    keys[#keys + 1] = key
  end
  table.sort(keys, function(a, b)
    return tostring(a) < tostring(b)
  end)
  return keys
end

local function add_paths(list, seen, value)
  if type(value) == "table" then
    for _, key in ipairs(sorted_keys(value)) do
      add_paths(list, seen, value[key])
    end
  elseif type(value) == "string" and value ~= "" and not seen[value] then
    seen[value] = true
    list[#list + 1] = value
  end
end

---Every path `backend.path` names, the one keyed to this host or domain first. Which of them exists
---is for the machine that runs the split to find out: a unix mux may hand the split to another host.
function M.candidates(cfg, domain, host)
  local path = cfg.backend.path
  if type(path) == "function" then
    path = util.try(path, domain, host)
  end
  local list, seen = {}, {}
  if type(path) == "table" then
    add_paths(list, seen, path[host])
    add_paths(list, seen, path[domain])
  end
  add_paths(list, seen, path)
  return list
end

---Whether `backend.path` names this domain or host outright: a table keyed to it, or a function
---answering for it. The plain string names no domain in particular.
function M.names(cfg, domain, host)
  local path = cfg.backend.path
  if type(path) == "function" then
    return util.try(path, domain, host) ~= nil
  end
  return type(path) == "table" and (path[host] ~= nil or path[domain] ~= nil)
end

---Local binaries that can answer the synchronous boot normalizer. This never returns a bootstrap,
---build, download, shell, or remote-domain command.
function M.normalizer_candidates(opts)
  opts = type(opts) == "table" and opts or {}
  local candidates, seen = {}, {}
  local function add(path)
    if type(path) ~= "string" or path == "" or seen[path] then
      return
    end
    -- Absolute/relative file paths are only candidates when the file already exists. A bare name
    -- may still resolve through PATH when it came from VTABS_BIN or an explicit local override.
    if path:find "[/\\]" then
      local file = io.open(path, "rb")
      if not file then
        return
      end
      file:close()
      if not platform.is_windows and util.try(wezterm.run_child_process, { "test", "-x", path }) ~= true then
        return
      end
    end
    if not seen[path] then
      seen[path] = true
      candidates[#candidates + 1] = path
    end
  end
  local raw_backend = type(opts.backend) == "table" and opts.backend or {}
  local listed = {}
  add_paths(listed, {}, raw_backend.path)
  local env_bin = util.getenv "VTABS_BIN"
  if type(env_bin) == "string" then
    for line in env_bin:gmatch "[^\n]+" do
      listed[#listed + 1] = line
    end
  end
  for _, path in ipairs(listed) do
    add(path)
  end

  local name = "wez-vtabs-" .. platform.triple .. "-" .. version .. (platform.is_windows and ".exe" or "")
  if platform.is_windows then
    local base = util.getenv "LOCALAPPDATA"
    if type(base) == "string" and base ~= "" then
      add(base .. "\\wez-vtabs\\bin\\" .. name)
    end
  else
    local base = util.getenv "XDG_DATA_HOME"
    if type(base) ~= "string" or base == "" or base:sub(1, 1) ~= "/" then
      local home = wezterm.home_dir or util.getenv "HOME"
      base = type(home) == "string" and home ~= "" and (home .. "/.local/share") or nil
    end
    if base then
      add(base .. "/wez-vtabs/bin/" .. name)
    end
  end
  return candidates
end

---Extra argv for a non-default role; the bootstraps forward whatever follows them to the binary.
local function with_role(args, role)
  if role ~= nil and role ~= "sidebar" then
    args[#args + 1] = "--role"
    args[#args + 1] = role
  end
  return args
end

---What a split runs, on whichever machine WezTerm hands it to: the bootstrap execs the first
---`VTABS_BIN` line found there and otherwise fetches or builds for the machine it is on. Only a
---Windows pane of a machine domain runs the PowerShell twin from this checkout.
function M.spawn_args(domain, role)
  if platform.is_windows and M.machine_domain(domain) then
    return with_role({
      "powershell",
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      M.root .. "\\bin\\bootstrap.ps1",
    }, role)
  end
  local script = bootstrap_script()
  if not script then
    return nil
  end
  -- `sh -c script` takes the next word as $0; the role flags follow it
  return with_role({ "sh", "-c", script, "wez-vtabs" }, role)
end

function M.env(cfg, domain, host, bg)
  local env = {}
  for key, value in pairs(cfg.backend.env or {}) do
    if type(key) == "string" and type(value) == "string" then
      env[key] = value
    end
  end
  env.VTABS_USERVAR = cfg.backend.uservar
  env.VTABS_REPO = cfg.backend.repo
  env.VTABS_VERSION = version
  env.VTABS_BG = type(bg) == "string" and bg:match "^#%x%x%x%x%x%x$" or nil
  -- settled where the split runs: uname outranks the triple, the first VTABS_BIN line that exists is exec'd
  env.VTABS_TARGET = platform.triple
  env.VTABS_SRC = M.root .. "/../backend"
  env.VTABS_BUILD = cfg.backend.build and "1" or "0"
  local candidates = M.candidates(cfg, domain, host)
  env.VTABS_BIN = candidates[1] and table.concat(candidates, "\n") or nil
  -- a mux pane of this machine may take its frames from a directory instead of the link
  if M.is_local(domain, host) and type(domain) == "string" and domain ~= "local" and cfg.backend.inbox ~= false then
    env.VTABS_INBOX_ROOT = util.runtime_dir()
  end
  return env
end

return M
