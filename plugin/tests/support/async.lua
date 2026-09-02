---Cooperative tasks for the lifecycle suite: a handler is a coroutine, and the fake mux and the
---wezterm stub yield where the real ones await. On the main thread every yield is a no-op, so the
---older suites run exactly as before.
local M = {}

local tasks = {}

function M.yield(tag)
  if coroutine.isyieldable() then
    coroutine.yield(tag)
  end
end

function M.step(task, ...)
  if task.done then
    return
  end
  local res = table.pack(coroutine.resume(task.co, ...))
  if not res[1] then
    task.done = true
    error(res[2], 0)
  end
  if coroutine.status(task.co) == "dead" then
    task.done = true
    task.results = table.pack(table.unpack(res, 2, res.n))
  else
    task.tag = res[2]
  end
end

---Starts `fn` and runs it to its first yield. `task.parked = true` keeps `run` off it.
function M.spawn(fn, ...)
  local task = { co = coroutine.create(fn), done = false, parked = false }
  tasks[#tasks + 1] = task
  M.step(task, ...)
  return task
end

---Resumes every live task round-robin until none is pending; `max` steps guards a task that never ends.
function M.run(max)
  max = max or 1000
  local steps = 0
  while true do
    local pending = false
    for _, task in ipairs(tasks) do
      if not task.done and not task.parked then
        pending = true
        steps = steps + 1
        if steps > max then
          error("async.run: still pending after " .. max .. " steps", 2)
        end
        M.step(task)
      end
    end
    if not pending then
      break
    end
  end
  local live = {}
  for _, task in ipairs(tasks) do
    if not task.done then
      live[#live + 1] = task
    end
  end
  tasks = live
end

function M.pending()
  local n = 0
  for _, task in ipairs(tasks) do
    if not task.done then
      n = n + 1
    end
  end
  return n
end

return M
