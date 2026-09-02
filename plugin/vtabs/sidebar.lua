local identity = require "vtabs.sidebar_identity"
local attach = require "vtabs.sidebar_attach"
local rescue = require "vtabs.sidebar_rescue"

---The sidebar pane in three halves -- who is one (`sidebar_identity`), how one comes and goes
---(`sidebar_attach`), and what puts a drifted one right (`sidebar_rescue`) -- behind the one name
---every caller already has. `require "vtabs.sidebar"` answers exactly what it did before the split.
local M = {}

M.is_overlay = identity.is_overlay
M.marker = identity.marker
M.title = identity.title
M.has_marker = identity.has_marker
M.is_settings = identity.is_settings
M.is_ready = identity.is_ready
M.is_backend = identity.is_backend
M.classify = identity.classify
M.find = identity.find
M.content_pane = identity.content_pane
M.tab_meta = identity.tab_meta
M.send = identity.send
M.send_raw = identity.send_raw
M.auth = identity.auth

M.attach = attach.attach
M.detach = attach.detach
M.close_orphan = attach.close_orphan
M.give_up = attach.give_up
M.refuse_v1 = attach.refuse_v1
M.ensure = attach.ensure
M.set_collapsed = attach.set_collapsed
M.toggle = attach.toggle

M.rescue_splits = rescue.rescue_splits

return M
