local M = {}

local ESC = "\27"

M.RESET = ESC .. "[0m"
M.HIDE_CURSOR = ESC .. "[?25l"

function M.cup(row, col)
  return string.format("%s[%d;%dH", ESC, row, col or 1)
end

function M.fg(rgb)
  return string.format("%s[38;2;%d;%d;%dm", ESC, rgb[1], rgb[2], rgb[3])
end

function M.bg(rgb)
  return string.format("%s[48;2;%d;%d;%dm", ESC, rgb[1], rgb[2], rgb[3])
end

function M.bold(on)
  return ESC .. (on and "[1m" or "[22m")
end

return M
