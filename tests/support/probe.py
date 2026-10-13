"""GUI process and observable state shared by isolated tests and executable scenarios."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
import uuid
from contextlib import ExitStack
from pathlib import Path

from tests.support.headless import HeadlessDisplay
from tests.support.process import stop_process

CONFIG = r"""
local wezterm = require 'wezterm'
local cfg = wezterm.config_builder()
local root = os.getenv('WEZ_VTABS_SCENARIO')
local identity = os.getenv('WEZ_VTABS_SCENARIO_ID')
local has_vtabs = os.getenv('WEZ_VTABS_SCENARIO_ENABLED') == '1'
local hook_calls = {title=0,routing=0,filter=0,theme=0,footer=0}
cfg.default_prog = wezterm.target_triple:find('windows') and {'cmd.exe'} or {'/bin/sh'}
cfg.initial_cols = tonumber(os.getenv('WEZ_VTABS_SCENARIO_COLS')) or 100
cfg.initial_rows = tonumber(os.getenv('WEZ_VTABS_SCENARIO_ROWS')) or 32
cfg.window_padding = {left=0,right=0,top=0,bottom=0}
cfg.window_decorations = 'RESIZE'
if os.getenv('WEZ_VTABS_SCENARIO_CHROME') == '1' then
  cfg.window_decorations = 'INTEGRATED_BUTTONS|RESIZE'
  cfg.window_padding = {left=7,right=9,top=5,bottom=11}
end
cfg.enable_tab_bar = false
cfg.window_close_confirmation = 'NeverPrompt'
cfg.exit_behavior = 'Close'
cfg.cursor_blink_rate = 0
cfg.status_update_interval = 16
cfg.check_for_updates = false
cfg.automatically_reload_config = false
cfg.keys = {}
for i=1,9 do
  table.insert(cfg.keys,{key=tostring(i),mods='SUPER',action=wezterm.action.ActivateTab(i-1)})
end
if has_vtabs then
  local plugin = assert(loadfile(os.getenv('WEZ_VTABS_PLUGIN')))()
  local options={profile=identity,settings={confirm_close=false}}
  if os.getenv('WEZ_VTABS_SCENARIO_EFFECTS') ~= '1' then options.settings.animations=false end
  if os.getenv('WEZ_VTABS_SCENARIO_HOOKS') == '1' then
    options.spaces={{id='home',name='Home',rules={{fields={{'process',{'wez-vtabs-scenario-unused-process'}}}}}}}
    options.hooks={
      title=function(tab) hook_calls.title=hook_calls.title+1; return tab.title end,
      routing=function(tab) hook_calls.routing=hook_calls.routing+1; return nil end,
      filter=function(tab) hook_calls.filter=hook_calls.filter+1; return true end,
      theme=function(context) hook_calls.theme=hook_calls.theme+1; return {accent='#89b4fa'} end,
      footer=function(context) hook_calls.footer=hook_calls.footer+1; return context.profile end,
    }
  end
  plugin.apply_to_config(cfg,options)
end
if os.getenv('WEZ_VTABS_SCENARIO_DOMAIN') == 'unix' then
  cfg.unix_domains={{name='scenario-unix',socket_path=root..'/mux.sock',no_serve_automatically=true}}
elseif os.getenv('WEZ_VTABS_SCENARIO_DOMAIN') == 'ssh' then
  local f=assert(io.open(root..'/ssh.json','r'))
  local domain=wezterm.json_parse(f:read('*a')); f:close()
  domain.name='scenario-ssh'; domain.multiplexing='WezTerm'
  cfg.ssh_domains={domain}
elseif os.getenv('WEZ_VTABS_SCENARIO_DOMAIN') == 'tls' then
  local f=assert(io.open(root..'/tls.json','r'))
  local domain=wezterm.json_parse(f:read('*a')); f:close()
  domain.name='scenario-tls'
  cfg.tls_clients={domain}
end
-- Settings writes reload the config; executed commands and the sample count outlive it.
wezterm.GLOBAL.scenario = wezterm.GLOBAL.scenario or {seen={},sequence=0}
local shared = wezterm.GLOBAL.scenario
local started, current_step, requested = false, 0, nil
local closing = {}
local function execute(window,pane,command)
  if command.kind=='action' then
    local value=command.value
    local action
    if value.name=='activate' then action=wezterm.action.ActivateTab(value.index)
    elseif value.name=='relative' then action=wezterm.action.ActivateTabRelative(value.delta)
    elseif value.name=='last' then action=wezterm.action.ActivateLastTab
    elseif value.name=='navigator' then action=wezterm.action.ShowTabNavigator
    elseif value.name=='new_tab' then action=wezterm.action.SpawnTab('CurrentPaneDomain')
    elseif value.name=='split' then action=wezterm.action.SplitHorizontal({domain='CurrentPaneDomain'})
    elseif value.name=='workspace' then action=wezterm.action.SwitchToWorkspace({name=value.workspace})
    elseif value.name=='font_increase' then action=wezterm.action.IncreaseFontSize
    elseif value.name=='font_reset' then action=wezterm.action.ResetFontSize
    elseif value.name=='fullscreen' then action=wezterm.action.ToggleFullScreen
    elseif value.name=='zoom' then action=wezterm.action.TogglePaneZoomState
    elseif value.name=='close' then action=wezterm.action.CloseCurrentTab({confirm=false})
    elseif value.name=='quit' then action=wezterm.action.QuitApplication
    else error('unknown vtabs action') end
    window:perform_action(action,pane)
  elseif command.kind=='intent' then wezterm.vtabs.dispatch(window,command.value)
  elseif command.kind=='resize' then
    current_step=command.step or 0; requested=command.value
    window:set_inner_size(command.value.width,command.value.height)
  elseif command.kind=='resize_sequence' then
    for i,size in ipairs(command.value) do
      local step=i
      wezterm.time.call_after((i-1)*0.06,function()
        current_step=step; requested=size
        window:set_inner_size(size.width,size.height)
      end)
    end
  elseif command.kind=='close_window' then
    closing[window:window_id()]=window:mux_window():window_id()
    for _,tab in ipairs(window:mux_window():tabs()) do
      for _,p in ipairs(tab:panes()) do
        p:send_text('exit\n')
      end
    end
  elseif command.kind=='detach' then
    closing[window:window_id()]=window:mux_window():window_id()
    pane:move_to_new_window('__detached')
  else error('unknown test command') end
end
local sample
sample=function(window)
  local id=window:window_id()
  local ok,err=pcall(function()
    local active=window:mux_window():active_tab()
    local pane=active and active:active_pane() or nil
    local f=io.open(root..'/command.json','r')
    if f then
      local text=f:read('*a'); f:close()
      local parsed,command=pcall(wezterm.json_parse,text)
      if parsed and command.window==id and command.id~=(shared.seen[tostring(id)] or 0) then
        shared.seen[tostring(id)]=command.id
        execute(window,pane,command)
      end
    end
    local state=has_vtabs and wezterm.vtabs.inspect(window) or {}
    state.window=id
    state.hook_calls=hook_calls
    state.mux_window=window:mux_window():window_id()
    state.workspace=window:mux_window():get_workspace()
    state.dimensions=window:get_dimensions()
    state.tabs={}
    for _,info in ipairs(window:mux_window():tabs_with_info()) do
      local tab={id=info.tab:tab_id(),active=info.is_active,size=info.tab:get_size(),panes={},zoomed=false}
      for _,pos in ipairs(info.tab:panes_with_info()) do
        if pos.is_zoomed then tab.zoomed=true end
        table.insert(tab.panes,{id=pos.pane:pane_id(),left=pos.left,top=pos.top,cols=pos.width,rows=pos.height})
      end
      table.insert(state.tabs,tab)
    end
    shared.sequence=shared.sequence+1; state.sequence=shared.sequence
    state.command=shared.seen[tostring(id)] or 0; state.step=current_step; state.requested=requested
    local output=assert(io.open(root..'/samples.jsonl','a'))
    output:write(wezterm.json_encode(state),'\n'); output:close()
  end)
  if not ok then
    -- inspect/get_dimensions may yield while this fixture's explicit close completes.
    -- Suppress only that exact removed mux window; unrelated sampling errors still fail.
    local closing_mux=closing[id]
    if closing_mux and tostring(err):find('window id '..closing_mux..' not found in mux',1,true)
        and wezterm.mux.get_window(closing_mux)==nil then return end
    local output=assert(io.open(root..'/errors.log','a')); output:write(tostring(err),'\n'); output:close()
  end
end
local function tick()
  for _,window in ipairs(wezterm.gui.gui_windows()) do sample(window) end
  wezterm.time.call_after(0.016,tick)
end
local function start()
  if not started then started=true; tick() end
end
wezterm.on('update-status',start)
wezterm.on('window-config-reloaded',start)
return cfg
"""


class Probe:
    def __init__(
        self,
        root,
        gui,
        helper,
        domain,
        ssh_config=None,
        vtabs=True,
        capture=False,
        server=None,
        chrome=False,
        initial_size=None,
        trace_mux=False,
        hooks=False,
        effects=False,
        resize_rounds=1,
        tls_config=None,
        display=None,
    ):
        self.root, self.gui, self.vtabs = root, gui, vtabs
        self.display = display or HeadlessDisplay(root / "display")
        self.owns_display = display is None
        self.domain = domain
        self.resize_rounds = resize_rounds
        self.capture_enabled = capture
        self.identity = "vtabs-" + uuid.uuid4().hex
        self.processes, self.outputs, self.samples, self.latest = [], [], [], {}
        self.offset, self.pending, self.command_id, self.window = 0, "", 0, None
        root.mkdir(parents=True)
        binaries = root / "bin"
        binaries.mkdir()
        shutil.copy2(gui, binaries / gui.name)
        server = server or gui.with_name(
            "wezterm-mux-server.exe" if os.name == "nt" else "wezterm-mux-server"
        )
        if server.is_file():
            shutil.copy2(server, binaries / server.name)
        self.gui = binaries / gui.name
        if helper:
            shutil.copy2(helper, binaries / helper.name)
            helper = binaries / helper.name
        for name in ("config", "cache", "data", "state", "runtime"):
            (root / name).mkdir()
        (root / "runtime").chmod(0o700)
        self.env = {
            key: value
            for key, value in self.display.env.items()
            if not key.startswith(("WEZTERM_", "WEZ_VTABS_"))
        }
        self.env.update(
            {
                "XDG_CONFIG_HOME": str(root / "config"),
                "XDG_CACHE_HOME": str(root / "cache"),
                "XDG_DATA_HOME": str(root / "data"),
                "XDG_STATE_HOME": str(root / "state"),
                "XDG_RUNTIME_DIR": str(root / "runtime"),
                "WEZ_VTABS_DB": str(root / "state.sqlite"),
                "WEZ_VTABS_SCENARIO": str(root),
                "WEZ_VTABS_SCENARIO_ID": self.identity,
                "WEZ_VTABS_SCENARIO_ENABLED": "1" if vtabs else "0",
                "WEZ_VTABS_SCENARIO_DOMAIN": domain,
            }
        )
        self.env["WEZ_VTABS_PLUGIN"] = str(Path(__file__).resolve().parents[2] / "plugin/init.lua")
        self.env["WEZ_VTABS_SCENARIO_CHROME"] = "1" if chrome else "0"
        self.env["WEZ_VTABS_SCENARIO_HOOKS"] = "1" if hooks else "0"
        self.env["WEZ_VTABS_SCENARIO_EFFECTS"] = "1" if effects else "0"
        if trace_mux:
            self.env["WEZTERM_LOG"] = (
                "warn,wezterm_client=trace,wezterm_mux_server_impl=trace,wezterm_gui::termwindow::resize=trace"
            )
        if initial_size:
            self.env["WEZ_VTABS_SCENARIO_COLS"] = str(initial_size["cols"])
            self.env["WEZ_VTABS_SCENARIO_ROWS"] = str(initial_size["rows"])
        if helper:
            self.env["WEZ_VTABS_STORE"] = str(helper)
        if ssh_config:
            (root / "ssh.json").write_text(ssh_config.read_text(encoding="utf-8"), encoding="utf-8")
        if tls_config:
            (root / "tls.json").write_text(tls_config.read_text(encoding="utf-8"), encoding="utf-8")
        self.config = root / "wezterm.lua"
        self.config.write_text(CONFIG, encoding="utf-8")

    def start_process(self, args, log, env=None):
        self.display.assert_live()
        process_env = dict(env or self.env)
        process_env.update({"DISPLAY": self.display.env["DISPLAY"], "GDK_BACKEND": "x11"})
        for key in (
            "WAYLAND_DISPLAY",
            "XAUTHORITY",
            "DBUS_SESSION_BUS_ADDRESS",
            "SSH_AUTH_SOCK",
        ):
            process_env.pop(key, None)
        output = (self.root / log).open("wb")
        self.outputs.append(output)
        process = subprocess.Popen(
            [str(value) for value in args],
            env=process_env,
            stdin=subprocess.DEVNULL,
            stdout=output,
            stderr=subprocess.STDOUT,
            start_new_session=os.name != "nt",
        )
        self.processes.append(process)
        return process

    def start(self):
        if self.owns_display:
            self.display.start()
        self.display.assert_live()
        self.env.update({"DISPLAY": self.display.env["DISPLAY"], "GDK_BACKEND": "x11"})
        if self.domain == "unix":
            if os.name == "nt":
                raise RuntimeError("Unix mux is unavailable on Windows; use local or SSH")
            server = self.gui.with_name("wezterm-mux-server")
            self.start_process([server, "--config-file", self.config], "mux.log")
            deadline = time.monotonic() + 15
            while not (self.root / "mux.sock").exists():
                if time.monotonic() > deadline:
                    raise AssertionError("owned Unix mux did not start")
                time.sleep(0.05)
        args = [
            self.gui,
            "--config-file",
            self.config,
            "start",
            "--always-new-process",
            "--no-auto-connect",
            "--class",
            self.identity,
            "--workspace",
            self.identity,
        ]
        if self.domain != "local":
            args += ["--domain", "scenario-" + self.domain]
        self.gui_process = self.start_process(args, "gui.log")
        state = self.wait(lambda state: state.get("tabs"), timeout=25, any_window=True)
        self.window = state["window"]
        self.wait(
            lambda state: (
                bool(state.get("tabs"))
                and (not self.vtabs or state.get("sidebar", {}).get("width", 0) > 0)
            )
        )
        self.sample_for(0.7)
        return self.latest[self.window]

    def read(self):
        error = self.root / "errors.log"
        if error.exists() and error.stat().st_size:
            raise AssertionError(error.read_text(encoding="utf-8"))
        path = self.root / "samples.jsonl"
        if not path.exists():
            return
        with path.open(encoding="utf-8") as source:
            source.seek(self.offset)
            text = self.pending + source.read()
            self.offset = source.tell()
        lines = text.split("\n")
        self.pending = lines.pop()
        for line in lines:
            if not line:
                continue
            state = json.loads(line)
            # Lua's JSON encoder represents an untagged empty array as {}.
            if state.get("visible") == {}:
                state["visible"] = []
            state["observed_at"] = time.monotonic()
            self.samples.append(state)
            self.latest[state["window"]] = state

    def wait(self, predicate, timeout=10, any_window=False):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            self.read()
            candidates = (
                list(self.latest.values()) if any_window else [self.latest.get(self.window, {})]
            )
            for state in candidates:
                if predicate(state):
                    return state
            if hasattr(self, "gui_process") and self.gui_process.poll() is not None:
                raise AssertionError(f"GUI exited: {self.root / 'gui.log'}")
            time.sleep(0.008)
        raise AssertionError(f"scenario condition timed out: {self.root / 'samples.jsonl'}")

    def sample_for(self, duration):
        start = len(self.samples)
        deadline = time.monotonic() + duration
        while time.monotonic() < deadline:
            self.read()
            time.sleep(0.008)
        self.read()
        return self.samples[start:]

    def send(self, kind, value=None, window=None, **extra):
        self.command_id += 1
        command = {
            "id": self.command_id,
            "window": self.window if window is None else window,
            "kind": kind,
            "value": value,
            **extra,
        }
        pending = self.root / "command.tmp"
        pending.write_text(json.dumps(command), encoding="utf-8")
        pending.replace(self.root / "command.json")
        return self.command_id

    def action(self, name, **arguments):
        return self.send("action", {"name": name, **arguments})

    def intent(self, value, window=None):
        return self.send("intent", value, window)

    def capture(self, name):
        if not self.capture_enabled:
            return
        self.display.assert_live()
        result = subprocess.run(
            ["xdotool", "search", "--onlyvisible", "--class", self.identity],
            env=self.env,
            text=True,
            capture_output=True,
            timeout=10,
            check=True,
        )
        windows = result.stdout.splitlines()
        if len(windows) != 1:
            raise AssertionError(f"Expected one owned GUI capture window, found {windows}")
        subprocess.run(
            [
                "import",
                "-display",
                self.display.env["DISPLAY"],
                "-window",
                windows[0],
                str(self.root / (name + ".png")),
            ],
            env=self.env,
            check=True,
            capture_output=True,
            timeout=10,
        )

    def close(self):
        with ExitStack() as cleanup:
            if self.owns_display:
                cleanup.callback(self.display.close)
            for output in self.outputs:
                cleanup.callback(output.close)
            for process in self.processes:
                cleanup.callback(stop_process, process, timeout_seconds=3)
            # Only windows created in this process/workspace receive close actions.
            for window in list(self.latest):
                try:
                    self.send("close_window", window=window)
                    time.sleep(0.08)
                except OSError:
                    pass


def pane_shape(state):
    return [
        [(pane["left"], pane["top"], pane["cols"], pane["rows"]) for pane in tab["panes"]]
        for tab in state["tabs"]
    ]
