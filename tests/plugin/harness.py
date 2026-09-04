from __future__ import annotations

import json
import os
import shutil
import subprocess
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Any

_BOOTSTRAP = r"""
local root, stub_path = arg[1], arg[2]
if type(root) ~= "string" or type(stub_path) ~= "string" then
  error("the headless harness requires repository and adapter paths", 0)
end
package.path = root .. "/plugin/?.lua;" .. root .. "/plugin/?/init.lua"
package.preload.wezterm = function()
  local chunk, load_error = loadfile(stub_path)
  if not chunk then
    error(load_error, 0)
  end
  return chunk()
end

local function scenario()
__SCENARIO__
end

local ok, observation = xpcall(scenario, debug.traceback)
if not ok then
  io.stderr:write(observation, "\n")
  os.exit(1)
end
io.write(require("wezterm").json_encode(observation), "\n")
"""


class LuaExecutionError(RuntimeError):
    """A Lua scenario failed or did not produce one JSON observation."""


@dataclass(frozen=True)
class LuaHarness:
    repo_root: Path
    scratch: Path
    timeout: float = 5.0

    def run(self, source: str, *, env: Mapping[str, str] | None = None) -> Any:
        """Run a Lua chunk in a new process and return its JSON-safe result."""
        executable = os.environ.get("VTABS_LUA", "lua")
        stub = self.repo_root / "tests" / "plugin" / "lua" / "wezterm_stub.lua"
        script = _BOOTSTRAP.replace("__SCENARIO__", source)

        home = self.scratch / "home"
        temporary = self.scratch / "tmp"
        runtime = self.scratch / "runtime"
        xdg_config = self.scratch / "config"
        xdg_data = self.scratch / "data"
        xdg_state = self.scratch / "state"
        for directory in (home, temporary, runtime, xdg_config, xdg_data, xdg_state):
            directory.mkdir(parents=True, exist_ok=True)
        runtime.chmod(0o700)

        supplied_env = dict(env or {})
        isolated_names = {
            "HOME",
            "TMPDIR",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_RUNTIME_DIR",
            "XDG_STATE_HOME",
        }
        if overlap := isolated_names.intersection(supplied_env):
            names = ", ".join(sorted(overlap))
            raise ValueError(
                f"Lua scenarios cannot override isolated environment variables: {names}"
            )
        lua_environment = ("LUA_INIT", "LUA_PATH", "LUA_CPATH")
        if unsafe := sorted(name for name in supplied_env if name.startswith(lua_environment)):
            raise ValueError(
                f"Lua scenarios cannot set interpreter environment variables: {unsafe}"
            )

        resolved_executable = shutil.which(executable)
        if resolved_executable is None:
            raise LuaExecutionError(
                f"Lua executable {executable!r} was not found; "
                "set VTABS_LUA to a Lua 5.4+ executable"
            )

        process_env = os.environ.copy()
        for name in tuple(process_env):
            if name.startswith((*lua_environment, "VTABS_", "WEZTERM_")):
                process_env.pop(name)
        process_env.update(
            {
                "HOME": str(home),
                "PATH": os.defpath,
                "TMPDIR": str(temporary),
                "XDG_CONFIG_HOME": str(xdg_config),
                "XDG_DATA_HOME": str(xdg_data),
                "XDG_RUNTIME_DIR": str(runtime),
                "XDG_STATE_HOME": str(xdg_state),
                "VTABS_TEST_TRIPLE": "x86_64-unknown-linux-gnu",
            }
        )
        process_env.update(supplied_env)

        command = [resolved_executable, "-E", "-", str(self.repo_root), str(stub)]
        try:
            completed = subprocess.run(
                command,
                input=script,
                text=True,
                capture_output=True,
                cwd=self.repo_root,
                env=process_env,
                timeout=self.timeout,
                check=False,
            )
        except FileNotFoundError as error:
            raise LuaExecutionError(
                f"Lua executable {executable!r} was not found; "
                "set VTABS_LUA to a Lua 5.4+ executable"
            ) from error
        except subprocess.TimeoutExpired as error:
            raise LuaExecutionError(
                f"Lua scenario exceeded {self.timeout:g}s\n"
                f"stdout:\n{error.stdout or ''}\nstderr:\n{error.stderr or ''}"
            ) from error

        if completed.returncode != 0:
            raise LuaExecutionError(
                f"Lua scenario exited with status {completed.returncode}\n"
                f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}\n"
                f"scenario:\n{source}"
            )
        try:
            return json.loads(completed.stdout)
        except json.JSONDecodeError as error:
            raise LuaExecutionError(
                "Lua scenario did not emit one valid JSON observation\n"
                f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}\n"
                f"scenario:\n{source}"
            ) from error
