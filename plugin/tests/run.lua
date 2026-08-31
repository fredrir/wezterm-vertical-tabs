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

---One file per module under test. They share the fake mux and the warn-once ledger, so the order
---is part of the contract: a suite loaded out of turn sees warnings already spent.
for _, suite in ipairs {
  "run_core",
  "run_mux",
  "run_sidebar",
  "run_render",
  "run_layout",
  "run_state",
  "run_geometry",
  "run_input",
  "run_theme",
  "run_platform",
  "run_view",
  "run_interaction",
  "run_schema",
  "run_popover",
  "run_settings",
  "run_frame",
} do
  require(suite)
end

os.remove(state.file)

local passed, failed = H.report()
print(string.format("%d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)
