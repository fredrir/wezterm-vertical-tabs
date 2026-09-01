local M = {}

---Per-process state keyed by a window, tab or pane id. The scope is declared *with* the field, so
---the two cannot drift: three hand-maintained name lists had already lost nine fields between them,
---and two of those were never cleared at all.
---
---`store.scope "name"` returns a namespace. `scope.window "hover"` declares a table keyed by window
---id and registers it both with its own namespace and globally, so `scope.forget_window(id)` clears
---one module's caches and `store.forget_window(id)` clears every module's.
local ALL = { window = {}, tab = {}, pane = {} }

local KEYED = { "window", "tab", "pane" }

function M.scope(name)
  local own = { window = {}, tab = {}, pane = {} }
  local scope = { name = name }
  for _, keyed in ipairs(KEYED) do
    scope[keyed] = function()
      local t = {}
      table.insert(own[keyed], t)
      table.insert(ALL[keyed], t)
      return t
    end
    scope["forget_" .. keyed] = function(id)
      for _, t in ipairs(own[keyed]) do
        t[id] = nil
      end
    end
  end
  -- Keyed by neither: a domain name outlives every window that ever reached it.
  scope.process = function()
    return {}
  end
  return scope
end

local function forget(keyed, id)
  for _, t in ipairs(ALL[keyed]) do
    t[id] = nil
  end
end

function M.forget_window(window_id)
  forget("window", window_id)
end

function M.forget_tab(tab_id)
  forget("tab", tab_id)
end

function M.forget_pane(pane_id)
  forget("pane", pane_id)
end

---Every window id any window-scoped table still holds, so a caller can drop the ones the mux has
---forgotten without keeping a list of what to look in.
function M.window_ids()
  local ids = {}
  for _, t in ipairs(ALL.window) do
    for id in pairs(t) do
      ids[id] = true
    end
  end
  return ids
end

---The shared per-process bus. It is shared because the data is: `hits` is written by the renderer
---and read by the input router, `tab_meta` is written by two modules and read by a third. What was
---missing was never ownership but a declared lifetime -- three hand-written name lists, nine fields
---in none of them, and two that nothing ever cleared. Here the lifetime is the declaration.
---`process` is keyed by a domain name, which outlives every window that ever reached it.
local SESSION = {
  drag = "window",
  scroll = "window",
  user_scrolled = "window",
  known_tabs = "window",
  focus_index = "window",
  last_active = "window",
  applying = "window",
  popover = "window",

  content_pane = "tab",
  tab_meta = "tab",
  moving = "tab",
  attaching = "tab",

  dims = "pane",
  ready = "pane",
  proto = "pane",
  paints = "pane",
  seen = "pane",
  pinged = "pane",
  sent_at = "pane",
  adopted = "pane",
  authed_at = "pane",
  auth_tries = "pane",
  marker = "pane",
  pane_domain = "pane",
  given_up = "pane",

  failed_domains = "process",
  spawned_domains = "process",
  logged_domains = "process",
}

---Every declared table, by name; `state.session` is this, and nothing else.
M.fields = {}

local session = M.scope "session"
for name, keyed in pairs(SESSION) do
  local t = session[keyed]()
  M.fields[name] = t
  M[name] = t
end

return M
