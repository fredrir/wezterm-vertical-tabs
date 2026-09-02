local wezterm = require "wezterm" ---@type Wezterm
local backend = require "vtabs.backend"
local config = require "vtabs.config"
local platform = require "vtabs.platform"
local schema = require "vtabs.schema"
local settings = require "vtabs.settings"
local settings_model = require "vtabs.settings_model"
local util = require "vtabs.util"
local version = require "vtabs.version"
local wire = require "vtabs.wire"

local M = {}

local REQUEST_MAX = 1024 * 1024
local RESPONSE_MAX = 1024 * 1024
local NORMALIZER_VERSION = 1

local function cleanup(path, dir)
  if path then
    os.remove(path)
  end
  if dir then
    util.try(wezterm.run_child_process, { "rmdir", dir })
  end
end

local function write_body(path, body)
  local file = io.open(path, "w")
  if not file then
    return false
  end
  local ok = pcall(function()
    assert(file:write(body))
  end)
  local closed = pcall(file.close, file)
  return ok and closed
end

---Creates the handoff where another local user cannot race or read it. Unix gets an atomically
---created 0700 directory and an empty file restricted to 0600 before any config bytes are written.
---On Windows, `os.tmpname` is the narrow primitive Lua exposes and the file inherits temp ACLs.
local function private_request(body)
  local token = util.random_token():sub(1, 24)
  if platform.is_windows then
    local path = os.tmpname()
    if type(path) ~= "string" or path == "" then
      return nil
    end
    if not write_body(path, body) then
      cleanup(path)
      return nil
    end
    return path
  end

  local base = util.getenv "TMPDIR"
  if not settings.safe_path(base) then
    base = "/tmp"
  end
  local dir = base .. "/wez-vtabs-normalize-" .. token
  if util.try(wezterm.run_child_process, { "mkdir", "-m", "700", dir }) ~= true then
    return nil
  end
  local path = dir .. "/request.json"
  local file = io.open(path, "w")
  if not file then
    cleanup(path, dir)
    return nil
  end
  if not pcall(file.close, file) then
    cleanup(path, dir)
    return nil
  end
  if util.try(wezterm.run_child_process, { "chmod", "600", path }) ~= true or not write_body(path, body) then
    cleanup(path, dir)
    return nil
  end
  return path, dir
end

local function request_body(opts, persisted)
  local projected, opaque, invalid = settings_model.project(opts, wire.array)
  local _, explicit = config.explicit_keys(opts)
  local body = wire.encode {
    normalizer_v = NORMALIZER_VERSION,
    plugin_version = version,
    schema_id = schema.schema_id,
    persisted = persisted,
    opts = projected,
    explicit = settings_model.paths(explicit, wire.array),
    invalid = settings_model.paths(invalid, wire.array),
  }
  if #body > REQUEST_MAX then
    return nil
  end
  return body, opaque
end

local function parse_response(body)
  if type(body) ~= "string" or #body > RESPONSE_MAX then
    return nil
  end
  local response = util.try(wezterm.json_parse, body)
  if
    type(response) ~= "table"
    or response.normalizer_v ~= NORMALIZER_VERSION
    or response.plugin_version ~= version
    or response.schema_id ~= schema.schema_id
    or type(response.values) ~= "table"
  then
    return nil
  end
  -- A matching version/schema pair owns detailed policy validation. Lua checks only the root and
  -- mandatory structural containers so an incomplete-but-well-formed helper cannot erase config.
  for _, option in ipairs(schema.options) do
    if option.container and type(schema.get(response.values, option.key)) ~= "table" then
      return nil
    end
  end
  return response
end

---Uses only an already-resolvable local binary. Any absence, old binary, malformed reply, or local
---I/O failure returns nil so apply_to_config can use the generated-schema Lua bootstrap path.
function M.try(opts)
  opts = type(opts) == "table" and opts or {}
  local candidates = backend.normalizer_candidates(opts)
  if #candidates == 0 then
    return nil
  end
  local body, opaque = request_body(opts, settings.read_body { settings = opts.settings })
  if not body then
    return nil
  end
  local request, request_dir = private_request(body)
  if not request then
    return nil
  end

  local response
  for _, executable in ipairs(candidates) do
    local called, ok, stdout = pcall(wezterm.run_child_process, {
      executable,
      "settings",
      "normalize",
      "--input",
      request,
    })
    if called and ok then
      response = parse_response(stdout)
      if response then
        break
      end
    end
  end
  cleanup(request, request_dir)
  if not response then
    return nil
  end
  for _, warning in ipairs(type(response.warnings) == "table" and response.warnings or {}) do
    if type(warning) == "string" then
      util.warn("%s", warning)
    end
  end
  settings_model.restore(response.values, opts, opaque)
  return config.adopt_normalized(opts, response.values)
end

return M
