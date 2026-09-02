local H = require "support.helpers"
local theme = require "vtabs.theme"

local test, eq = H.test, H.eq

test("the default sidebar surface is darker than dark and light terminal content", function()
  for _, case in ipairs {
    { H.palette("#1e1e2e", "#cdd6f4"), "#171723" },
    { H.palette("#eff1f5", "#4c4f69"), "#e1e3e6" },
  } do
    local resolved = theme.resolve({}, case[1])
    assert(theme.luminance(resolved.bg) < theme.luminance(resolved.content_bg))
    eq(H.rgb(resolved.bg), H.hex(case[2]))
  end
end)

test("zero elevation is seamless and an explicit sidebar background wins", function()
  local palette = H.palette("#1e1e2e", "#cdd6f4")
  local seamless = theme.resolve({ elevation = 0 }, palette)
  eq(H.rgb(seamless.bg), H.rgb(seamless.content_bg))
  eq(H.rgb(theme.resolve({ bg = "#123456" }, palette).bg), H.hex "#123456")
end)
