local H = require "support.helpers"
local wezterm = require "wezterm"
local util = require "vtabs.util"
local config = require "vtabs.config"
local fake = require "fake_mux"
local model_mod = require "vtabs.model"

local test, eq = H.test, H.eq
local legacy = H.legacy

local function meta_of(pane_opts, over)
  local base = { backend = { path = "/bin/wez-vtabs" }, meta = "auto", tab_height = "card" }
  config.setup(legacy(util.merge(base, over or {})))
  local win = fake.window()
  local tab = win:add_tab { title = "t" }
  local pane = tab.pane_list[1]
  for k, v in pairs(pane_opts) do
    pane[k] = v
  end
  model_mod.forget_tab(tab:tab_id())
  return model_mod.build(win.gui)[1].meta
end

test("the meta line names the cwd for shells and the process for anything else", function()
  local home = wezterm.home_dir
  eq(meta_of { process = "/bin/zsh", cwd = { file_path = "/tmp/work" } }, "~/work", "home_dir collapses to ~")
  eq(meta_of { process = "/usr/bin/fish", cwd = { file_path = "/etc" } }, "/etc")
  eq(meta_of { process = "/usr/bin/nvim", cwd = { file_path = "/tmp/work" } }, "nvim  work", "no separator glyph")
  eq(meta_of { process = "/usr/bin/cargo", cwd = { file_path = "/srv/api" } }, "cargo  api")
  if home and home ~= "" then
    eq(meta_of { process = "/bin/bash", cwd = { file_path = home .. "/projects/api" } }, "~/projects/api")
    eq(meta_of { process = "/bin/bash", cwd = { file_path = home } }, "~")
  end
end)

test("ssh names the remote user only when the pane reports one, never the local $USER", function()
  local ssh = meta_of { process = "/usr/bin/ssh", cwd = { file_path = "/home/x", host = "archie" } }
  eq(ssh, "archie", "no authority in the cwd means host alone")
  local named = meta_of {
    process = "/usr/bin/ssh",
    cwd = { file_path = "/home/admin", host = "buildbox", username = "admin" },
  }
  eq(named, "admin@buildbox", "the URL authority is the only source")
  local url = meta_of { process = "/usr/bin/ssh", cwd = "file://admin@buildbox/home/admin" }
  eq(url, "admin@buildbox", "and it is parsed out of the string form too")
  eq(meta_of { process = "/usr/bin/ssh", cwd = false }, "ssh", "nothing resolvable falls back to the process")
  -- get_foreground_process_name is nil off the local domain, so the domain carries the line.
  eq(meta_of { domain = "SSH:archie", cwd = { file_path = "/home/x/api" } }, "SSH:archie  /home/x/api")
  eq(meta_of { domain = "local", process = nil, cwd = { file_path = "/srv" } }, "/srv")
end)

test("meta = cwd, process and false force one column or none", function()
  local pane = { process = "/usr/bin/nvim", cwd = { file_path = "/tmp/work" } }
  eq(meta_of(pane, { meta = "cwd" }), "~/work")
  eq(meta_of(pane, { meta = "process" }), "nvim")
  eq(meta_of(pane, { meta_sep = " · " }), "nvim · work", "the separator is configurable")
  eq(meta_of(pane, { meta = false }), nil)
end)

test("a pane that resolves nothing leaves meta nil rather than an empty row", function()
  eq(meta_of { process = nil, domain = "local", cwd = false }, nil)
end)

test("meta is resolved at most once per poll_ms per tab", function()
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" }, poll_ms = 60000, meta = "auto", tab_height = "card" })
  local win = fake.window()
  local tab = win:add_tab { title = "t" }
  local pane = tab.pane_list[1]
  pane.process, pane.cwd = "/bin/zsh", { file_path = "/tmp/first" }
  model_mod.forget_tab(tab:tab_id())
  local calls = 0
  local original = getmetatable(pane).get_current_working_dir
  getmetatable(pane).get_current_working_dir = function(self)
    calls = calls + 1
    return original(self)
  end
  eq(model_mod.build(win.gui)[1].meta, "~/first")
  pane.cwd = { file_path = "/tmp/second" }
  for _ = 1, 5 do
    model_mod.build(win.gui)
  end
  getmetatable(pane).get_current_working_dir = original
  eq(calls, 1, "five more builds inside one poll cost nothing")
  eq(model_mod.build(win.gui)[1].meta, "~/first", "the cached value is what renders")
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)
