local here = arg[0]:match "^(.*)[/\\]" or "."
package.path = here .. "/../?.lua;" .. here .. "/?.lua;" .. package.path
package.preload.wezterm = function()
  return require "wezterm_stub"
end

local H = require "support.helpers"
local backend = require "vtabs.backend"
local state = require "vtabs.state"

backend.root = here .. "/.."
state.file = os.tmpname()

-- Suites share the fake mux and the warn-once ledger, so the order is part of the contract
for _, suite in ipairs { "run_state", "run_geometry", "run_schema", "run_apply", "run_spaces", "run_lifecycle" } do
  require(suite)
end

os.remove(state.file)

local passed, failed = H.report()
print(string.format("%d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)
