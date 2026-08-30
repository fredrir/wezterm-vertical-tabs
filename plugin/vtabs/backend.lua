local platform = require "vtabs.platform"
local version = require "vtabs.version"

local M = {}

M.root = nil

function M.spawn_args(cfg)
  if cfg.backend.path then
    return { cfg.backend.path }
  end
  if platform.is_windows then
    return { "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", M.root .. "\\bin\\bootstrap.ps1" }
  end
  return { "sh", M.root .. "/bin/bootstrap.sh" }
end

function M.env(cfg)
  return {
    VTABS_USERVAR = cfg.backend.uservar,
    VTABS_TARGET = platform.triple,
    VTABS_REPO = cfg.backend.repo,
    VTABS_VERSION = cfg.backend.version or version,
    VTABS_SRC = M.root .. "/../backend",
    VTABS_BUILD = cfg.backend.build and "1" or "0",
    VTABS_BIN = cfg.backend.path,
  }
end

return M
