local M = {}

function M.at(hits, y)
  return hits and hits[y] or { kind = "space" }
end

---First sub-target under `x`: "close" or "pin" on a card row, else nil.
function M.span(hit, x)
  for _, span in ipairs(hit and hit.spans or {}) do
    if x >= span.x1 and x <= span.x2 then
      return span.id
    end
  end
  return nil
end

---True when `x` is on the row's card surface; cols outside it behave as empty space.
function M.in_card(hit, x)
  return hit ~= nil and hit.x1 ~= nil and x >= hit.x1 and x <= hit.x2
end

---True when `x` is on the sidebar edge that borders the content pane.
function M.on_inner_edge(x, cols, position)
  if position == "right" then
    return x <= 1
  end
  return x >= cols
end

---Slot the dragged tab would take when dropped at row `y`; a gap row drops below its card.
function M.drop_slot(hits, y, rows)
  local hit = M.at(hits, y)
  if hit.kind == "tab" and hit.slot then
    return hit.part == "gap" and hit.slot + 1 or hit.slot
  end
  local last_slot = 0
  for row = 1, rows do
    local h = M.at(hits, row)
    if h.kind == "tab" and h.slot then
      last_slot = math.max(last_slot, h.slot)
      if row > y then
        return h.slot
      end
    end
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
