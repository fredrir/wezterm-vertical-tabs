local M = {}

function M.at(hits, y)
  return hits and hits[y] or { kind = "space" }
end

function M.in_close(hit, x)
  return hit.close ~= nil and x >= hit.close.from and x <= hit.close.to
end

---True when `x` is on the sidebar edge that borders the content pane.
function M.on_inner_edge(x, cols, position)
  if position == "right" then
    return x <= 1
  end
  return x >= cols
end

---Slot the dragged tab would take when dropped at row `y`.
function M.drop_slot(hits, y, rows, top_padding)
  local hit = M.at(hits, y)
  if hit.kind == "tab" then
    return hit.slot
  end
  local last_slot = 0
  for row = 1, rows do
    local h = M.at(hits, row)
    if h.kind == "tab" then
      last_slot = math.max(last_slot, h.slot)
      if row > y then
        return h.slot
      end
    end
  end
  if y <= top_padding then
    return 1
  end
  return last_slot + 1
end

---Double click = same target twice within `window_ms`; the match consumes the first click.
function M.double_click(last, key, now, window_ms)
  local hit = last ~= nil and last.key == key and now - last.at <= window_ms
  if hit then
    return true, nil
  end
  return false, { key = key, at = now }
end

---Pin state a dragged tab should end up with: pinned when it lands inside the pinned block.
function M.should_pin(slot, pinned_others, was_pinned)
  local block = pinned_others + (was_pinned and 1 or 0)
  return slot >= 1 and slot <= block
end

return M
