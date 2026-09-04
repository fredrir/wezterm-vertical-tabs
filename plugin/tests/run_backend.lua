---What a split runs and the paths it carries: settled by the machine that runs it, never here.
local H = require "support.helpers"
local config = require "vtabs.config"
local backend = require "vtabs.backend"
local test, eq = H.test, H.eq

local function setup(path)
  return config.setup { backend = { path = path } }
end

local function joined(cfg, domain, host)
  return table.concat(backend.candidates(cfg, domain, host), ",")
end

test("every path backend.path names travels, the one keyed to the place first", function()
  eq(joined(setup "/opt/wez-vtabs", "local", nil), "/opt/wez-vtabs")
  eq(
    joined(setup { "/Users/me/wez-vtabs", "/home/me/wez-vtabs" }, "localmux", nil),
    "/Users/me/wez-vtabs,/home/me/wez-vtabs"
  )
  local keyed = setup { archie = "/home/me/wez-vtabs", ["archie-tls"] = "/srv/wez-vtabs", localmux = "/opt/wez-vtabs" }
  eq(
    joined(keyed, "archie-tls", "archie"),
    "/home/me/wez-vtabs,/srv/wez-vtabs,/opt/wez-vtabs",
    "host, domain, the rest by key"
  )
  eq(joined(keyed, "localmux", nil), "/opt/wez-vtabs,/home/me/wez-vtabs,/srv/wez-vtabs")
  local answered = setup(function(_, host)
    return host == "archie" and "/home/me/wez-vtabs" or { "/a", "/b" }
  end)
  eq(joined(answered, "localmux", "archie"), "/home/me/wez-vtabs")
  eq(joined(answered, "localmux", nil), "/a,/b", "a function may answer with a list")
  eq(#backend.candidates(setup(nil), "local", nil), 0)
end)

test("the split runs the bootstrap in every domain, the candidates one per line", function()
  local cfg = setup { "/Users/me/wez-vtabs", "/home/me/wez-vtabs" }
  for _, domain in ipairs { "local", "localmux", "archie-tls" } do
    local args = backend.spawn_args(domain)
    eq(args[1] .. " " .. args[2], "sh -c", domain)
    assert(args[3]:find("VTABS_BIN", 1, true), "the inline script reads the candidates")
    eq(args[4], "wez-vtabs", "$0")
    eq(#args, 4)
    local env = backend.env(cfg, domain, nil)
    eq(env.VTABS_BIN, "/Users/me/wez-vtabs\n/home/me/wez-vtabs")
    eq(env.VTABS_BUILD, "1", "a build is refused where the source is not, by the machine that looks")
    assert(env.VTABS_SRC and env.VTABS_TARGET, "hints for the bootstrap")
  end
  local settings = backend.spawn_args("local", "settings")
  eq(settings[5] .. " " .. settings[6], "--role settings")
  eq(backend.env(setup(nil), "local", nil).VTABS_BIN, nil)
end)

test("backend.path names a domain only when keyed to it", function()
  eq(backend.names(setup "/opt/wez-vtabs", "archie-tls", "archie"), false, "a plain string names nothing")
  eq(backend.names(setup { archie = "/x" }, "archie-tls", "archie"), true)
  eq(backend.names(setup { ["archie-tls"] = "/x" }, "archie-tls", nil), true)
  eq(backend.names(setup { archie = "/x" }, "archie-tls", nil), false, "the host is not known yet")
  local by_domain = setup(function(domain)
    return domain == "archie-tls" and "/x" or nil
  end)
  eq(backend.names(by_domain, "archie-tls", nil), true)
  eq(backend.names(by_domain, "localmux", nil), false)
end)

test("a machine domain is local or unix, whatever host the cwd names", function()
  backend.register_local_domains { unix_domains = { { name = "localmux" } } }
  eq(backend.machine_domain "local", true)
  eq(backend.machine_domain "localmux", true)
  eq(backend.machine_domain "archie-tls", false)
  eq(backend.is_local("localmux", "archie"), false, "the cwd host is still the hint for transport")
  eq(backend.is_local("localmux", nil), true)
end)
