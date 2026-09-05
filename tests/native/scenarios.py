#!/usr/bin/env python3
"""Opt-in native GUI scenarios; every launch uses an isolated window and state."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tempfile
import time
import uuid


CONFIG = r'''
local wezterm = require 'wezterm'
local cfg = wezterm.config_builder()
local root = os.getenv('WEZ_VTABS_SCENARIO')
local identity = os.getenv('WEZ_VTABS_SCENARIO_ID')
local is_native = os.getenv('WEZ_VTABS_SCENARIO_NATIVE') == '1'
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
if is_native then
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
end
local seen, started, sequence, current_step, requested, env_probe = {}, false, 0, 0, nil, false
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
    else error('unknown native action') end
    window:perform_action(action,pane)
  elseif command.kind=='intent' then wezterm.native_tabs.dispatch(window,command.value)
  elseif command.kind=='probe_private_env' then
    env_probe=true
    pane:send_text(wezterm.target_triple:find('windows') and 'echo VTABS_PRIVATE_CHECK:%VTABS_PRIVATE%\r\n' or 'printf "\\nVTABS_PRIVATE_CHECK:%s\\n" "$VTABS_PRIVATE"\n')
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
      if parsed and command.window==id and command.id~=(seen[id] or 0) then
        seen[id]=command.id
        execute(window,pane,command)
      end
    end
    local state=is_native and wezterm.native_tabs.inspect(window) or {}
    state.window=id
    state.hook_calls=hook_calls
    state.mux_window=window:mux_window():window_id()
    state.workspace=window:mux_window():get_workspace()
    if env_probe and pane then state.private_env_ok=pane:get_lines_as_text(40):find('VTABS_PRIVATE_CHECK:1',1,true)~=nil end
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
    sequence=sequence+1; state.sequence=sequence
    state.command=seen[id] or 0; state.step=current_step; state.requested=requested
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
wezterm.on('update-status',function()
  if not started then started=true; tick() end
end)
return cfg
'''


class Probe:
    def __init__(self, root, gui, helper, domain, ssh_config=None, native=True, capture=False, server=None, chrome=False, initial_size=None, trace_mux=False, hooks=False, effects=False, resize_rounds=1):
        self.root, self.gui, self.native = root, gui, native
        self.domain = domain
        self.resize_rounds = resize_rounds
        self.capture_enabled = capture
        self.identity = "vtabs-native-" + uuid.uuid4().hex
        self.processes, self.outputs, self.samples, self.latest = [], [], [], {}
        self.offset, self.pending, self.command_id, self.window = 0, "", 0, None
        root.mkdir(parents=True)
        binaries = root / "bin"
        binaries.mkdir()
        shutil.copy2(gui, binaries / gui.name)
        server = server or gui.with_name("wezterm-mux-server.exe" if os.name == "nt" else "wezterm-mux-server")
        if server.is_file():
            shutil.copy2(server, binaries / server.name)
        self.gui = binaries / gui.name
        if helper:
            shutil.copy2(helper, binaries / helper.name)
            helper = binaries / helper.name
        for name in ("config", "cache", "data", "state", "runtime"):
            (root / name).mkdir()
        (root / "runtime").chmod(0o700)
        self.env = {key: value for key, value in os.environ.items() if not key.startswith(("WEZTERM_", "WEZ_VTABS_"))}
        self.env.update({"XDG_CONFIG_HOME": str(root / "config"), "XDG_CACHE_HOME": str(root / "cache"), "XDG_DATA_HOME": str(root / "data"), "XDG_STATE_HOME": str(root / "state"), "XDG_RUNTIME_DIR": str(root / "runtime"), "WEZ_VTABS_DB": str(root / "state.sqlite"), "WEZ_VTABS_SCENARIO": str(root), "WEZ_VTABS_SCENARIO_ID": self.identity, "WEZ_VTABS_SCENARIO_NATIVE": "1" if native else "0", "WEZ_VTABS_SCENARIO_DOMAIN": domain})
        self.env["WEZ_VTABS_PLUGIN"] = str(Path(__file__).resolve().parents[2] / "plugin/init.lua")
        self.env["WEZ_VTABS_SCENARIO_CHROME"] = "1" if chrome else "0"
        self.env["WEZ_VTABS_SCENARIO_HOOKS"] = "1" if hooks else "0"
        self.env["WEZ_VTABS_SCENARIO_EFFECTS"] = "1" if effects else "0"
        if trace_mux:
            self.env["WEZTERM_LOG"] = "warn,wezterm_client=trace,wezterm_mux_server_impl=trace,wezterm_gui::termwindow::resize=trace"
        if initial_size:
            self.env["WEZ_VTABS_SCENARIO_COLS"] = str(initial_size["cols"])
            self.env["WEZ_VTABS_SCENARIO_ROWS"] = str(initial_size["rows"])
        if helper:
            self.env["WEZ_VTABS_STORE"] = str(helper)
        if ssh_config:
            (root / "ssh.json").write_text(ssh_config.read_text(encoding="utf-8"), encoding="utf-8")
        self.config = root / "wezterm.lua"
        self.config.write_text(CONFIG, encoding="utf-8")

    def start_process(self, args, log, env=None):
        output = (self.root / log).open("wb")
        self.outputs.append(output)
        process = subprocess.Popen([str(value) for value in args], env=env or self.env, stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT, start_new_session=os.name != "nt")
        self.processes.append(process)
        return process

    def start(self):
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
        args = [self.gui, "--config-file", self.config, "start", "--always-new-process", "--no-auto-connect", "--class", self.identity, "--workspace", self.identity]
        if self.domain != "local":
            args += ["--domain", "scenario-" + self.domain]
        self.gui_process = self.start_process(args, "gui.log")
        state = self.wait(lambda state: state.get("tabs"), timeout=25, any_window=True)
        self.window = state["window"]
        self.wait(lambda state: bool(state.get("tabs")) and (not self.native or state.get("sidebar", {}).get("width", 0) > 0))
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
            candidates = list(self.latest.values()) if any_window else [self.latest.get(self.window, {})]
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
        command = {"id": self.command_id, "window": self.window if window is None else window, "kind": kind, "value": value, **extra}
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
        import sys
        if sys.platform != "darwin":
            return
        program = '''import CoreGraphics
import Foundation
let pid = Int(CommandLine.arguments[1])!
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as! [[String: Any]]
for window in windows {
 if window[kCGWindowOwnerPID as String] as? Int == pid && window[kCGWindowLayer as String] as? Int == 0 {
  print(window[kCGWindowNumber as String]!); break
 }
}
'''
        result = subprocess.run(["swift", "-e", program, str(self.gui_process.pid)], text=True, capture_output=True, timeout=30, check=True)
        identifier = result.stdout.strip()
        if not identifier.isdigit():
            raise AssertionError("owned GUI capture window missing")
        subprocess.run(["/usr/sbin/screencapture", "-x", "-o", "-l", identifier, str(self.root / (name + ".png"))], check=True, timeout=10)

    def close(self):
        # Only windows created in this process/workspace receive close actions.
        for window in list(self.latest):
            try:
                self.send("close_window", window=window)
                time.sleep(0.08)
            except OSError:
                pass
        for process in reversed(self.processes):
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=3)
        for output in self.outputs:
            output.close()


def pane_shape(state):
    return [[(pane["left"], pane["top"], pane["cols"], pane["rows"]) for pane in tab["panes"]] for tab in state["tabs"]]


def geometry_scenarios(probe):
    initial = probe.start()
    first = initial["tabs"][0]["id"]
    assert len(initial["tabs"]) == 1, "fixture must start with one owned tab"
    probe.action("split")
    split = probe.wait(lambda state: len(state.get("tabs", [{}])[0].get("panes", [])) == 2)
    pane_ids = [pane["id"] for pane in split["tabs"][0]["panes"]]
    original_shape = pane_shape(split)[0]
    probe.action("new_tab")
    two = probe.wait(lambda state: len(state.get("tabs", [])) == 2)
    probe.capture("tabs-and-splits")
    second = two["tabs"][1]["id"]
    dimensions = two["dimensions"]
    deltas = [0, 60, 120, 200, 300, 240, 140, 40, -80, -160, -80, 0]
    sizes = [{"width": dimensions["pixel_width"] + delta, "height": dimensions["pixel_height"]} for delta in deltas] * probe.resize_rounds
    start = len(probe.samples)
    command = probe.send("resize_sequence", sizes)
    probe.wait(lambda state: state.get("command", 0) >= command and state.get("step") == len(sizes), timeout=max(10, len(sizes) * 0.06 + 5))
    probe.sample_for(0.35)
    frames = [state for state in probe.samples[start:] if state["window"] == probe.window and state.get("command", 0) >= command]
    assert len({state["dimensions"]["pixel_width"] for state in frames}) >= 5, "insufficient resize samples"
    for state in frames:
        tab = next(tab for tab in state["tabs"] if tab["id"] == first)
        assert [pane["id"] for pane in tab["panes"]] == pane_ids, "resize changed content pane identities"
        assert all(pane["top"] == 0 for pane in tab["panes"]), "resize rearranged horizontal content splits"
        if probe.native:
            expected_width = state["model"]["settings"]["width"] * state["dimensions"]["dpi"] / 96
            assert abs(state["sidebar"]["width"] - expected_width) <= 1, "transient sidebar width correction"
            assert state["content"]["x"] >= state["sidebar"]["x"] + state["sidebar"]["width"] - 1, "content overlaps sidebar"
            sizes_now = {(tab["size"]["cols"], tab["size"]["rows"]) for tab in state["tabs"]}
            assert len(sizes_now) == 1, "background tab has stale content dimensions"
    before = probe.latest[probe.window]
    for index, expected in ((0, first), (1, second), (-1, second), (0, first)):
        requested_at = time.monotonic()
        command = probe.action("activate", index=index)
        selected = probe.wait(lambda state: state.get("command", 0) >= command and next((tab["id"] for tab in state.get("tabs", []) if tab["active"]), None) == expected)
        if probe.native:
            assert selected["active"] == expected, "native binding and visible projection disagree"
            assert selected["sidebar"] == before["sidebar"], "native activation changed sidebar geometry"
        selected["activation_observed_seconds"] = selected["observed_at"] - requested_at
    probe.sample_for(0.4)
    final = probe.latest[probe.window]
    observed_steps = {}
    for state in frames:
        if state.get("step", 0) and state.get("requested") and abs(state["dimensions"]["pixel_width"] - state["requested"]["width"]) <= 1:
            observed_steps[state["step"]] = pane_shape(state)
    return {"initial_size": split["tabs"][0]["size"], "initial_split": original_shape, "final_split": pane_shape(final)[0], "resize_steps": observed_steps, "frames": len(frames)}


def feature_scenarios(probe):
    state = probe.latest[probe.window]
    home = state["model"]["selected_space"]
    old_tabs = {tab["id"] for tab in state["tabs"]}
    probe.intent({"CreateSpace": {"name": "Scenario Work"}})
    empty = probe.wait(lambda state: state.get("model", {}).get("selected_space") != home)
    space = empty["model"]["selected_space"]
    assert empty["visible"] == [] and empty["active"] is None, "empty space exposes hidden terminal"
    assert {tab["id"] for tab in empty["tabs"]} == old_tabs, "selecting space spawned or moved content"
    assert any(item["name"] == "Scenario Work" for item in empty["model"]["spaces"])
    probe.capture("empty-space")
    probe.action("new_tab")
    created = probe.wait(lambda state: len(state.get("tabs", [])) == len(old_tabs) + 1 and len(state.get("visible", [])) == 1)
    assert created["active"] == created["visible"][0]
    assert created["model"]["selected_space"] == space
    probe.intent({"SelectSpace": home})
    probe.wait(lambda state: state.get("model", {}).get("selected_space") == home and len(state.get("visible", [])) == len(old_tabs))
    probe.sample_for(0.6)
    idle = [state for state in probe.sample_for(0.3) if state["window"] == probe.window]
    assert idle and len({state["model"]["surface_revision"] for state in idle}) == 1, "idle sidebar recomposes"
    if probe.env["WEZ_VTABS_SCENARIO_HOOKS"] == "1":
        assert all(value > 0 for value in idle[0]["hook_calls"].values()), "configured hook was not exercised"
        assert idle[0]["hook_calls"] == idle[-1]["hook_calls"], "idle sidebar reruns Lua hooks"
    probe.intent({"SetSetting": {"key": "width", "value": 300}})
    probe.wait(lambda state: state.get("model", {}).get("settings", {}).get("width") == 300)
    deadline = time.monotonic() + 5
    rows = []
    while time.monotonic() < deadline:
        try:
            with sqlite3.connect(f"file:{probe.root / 'state.sqlite'}?mode=ro", uri=True) as connection:
                rows = connection.execute("SELECT entity,field,value FROM fields WHERE value IS NOT NULL").fetchall()
            if any(field == "width" and value == "300" for _, field, value in rows):
                break
        except sqlite3.Error:
            pass
        probe.sample_for(0.05)
    else:
        raise AssertionError("settings write did not reach SQLite")
    probe.intent("PrivateWindow")
    private = probe.wait(lambda state: state.get("model", {}).get("private") is True, any_window=True)
    private_window = private["window"]
    private_tab = private["tabs"][0]["id"]
    private_count = len(private["tabs"])
    probe.send("action", {"name": "new_tab"}, window=private_window)
    private = probe.wait(lambda state: state["window"] == private_window and len(state.get("tabs", [])) == private_count + 1, any_window=True)
    probe.send("probe_private_env", window=private_window)
    probe.wait(lambda state: state["window"] == private_window and state.get("private_env_ok") is True, any_window=True)
    probe.intent({"RenameTab": {"id": private_tab, "title": "PRIVATE MUST NOT PERSIST"}}, window=private_window)
    probe.wait(lambda state: state["window"] == private_window and state.get("command", 0) >= probe.command_id, any_window=True)
    probe.intent({"PinTab": {"id": private_tab, "pinned": True}}, window=private_window)
    probe.wait(lambda state: state["window"] == private_window and state.get("command", 0) >= probe.command_id, any_window=True)
    probe.intent({"CreateSpace": {"name": "Shared catalog edit"}}, window=private_window)
    probe.wait(lambda state: state["window"] == private_window and any(space["name"] == "Shared catalog edit" for space in state.get("model", {}).get("spaces", [])), any_window=True)
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        with sqlite3.connect(f"file:{probe.root / 'state.sqlite'}?mode=ro", uri=True) as connection:
            persisted = connection.execute("SELECT scope,entity,value FROM fields WHERE value IS NOT NULL").fetchall()
        if any('Shared catalog edit' in value for _, _, value in persisted):
            break
        probe.sample_for(0.05)
    else:
        raise AssertionError("explicit shared catalog edit was not persisted")
    assert all("PRIVATE MUST NOT PERSIST" not in value for _, _, value in persisted), "private tab metadata reached SQLite"
    assert not any(json.loads(scope)["kind"] == "session" and entity == f"tab:{private_tab}" for scope, entity, _ in persisted), "private tab state reached SQLite"
    return {"empty_space": space, "private_window": private_window, "persisted_fields": len(rows), "idle_samples": len(idle)}


def workspace_scenario(probe):
    state = probe.latest[probe.window]
    original_window = probe.window
    original_mux = state["mux_window"]
    original_space = state["model"]["selected_space"]
    tabs = [tab["id"] for tab in state["tabs"]]
    probe.intent({"CreateSpace": {"name": "Retained workspace state"}})
    state = probe.wait(lambda state: state.get("model", {}).get("selected_space") != original_space)
    space = state["model"]["selected_space"]
    for tab in tabs:
        command = probe.intent({"AssignTab": {"id": tab, "space_id": space}})
        probe.wait(lambda state: state.get("command", 0) >= command and tab in state.get("visible", []))
    command = probe.intent({"PinTab": {"id": tabs[0], "pinned": True}})
    probe.wait(lambda state: state.get("command", 0) >= command)
    probe.action("activate", index=0)
    probe.wait(lambda state: state.get("active") == tabs[0])
    switched_at = time.monotonic()
    probe.action("workspace", workspace=probe.identity + "-other")
    other = probe.wait(lambda state: state.get("mux_window") != original_mux and state.get("workspace") == probe.identity + "-other" and state.get("observed_at", 0) > switched_at, any_window=True)
    probe.window = other["window"]
    switched_at = time.monotonic()
    probe.action("workspace", workspace=probe.identity)
    restored = probe.wait(lambda state: state.get("mux_window") == original_mux and state.get("workspace") == probe.identity and state.get("observed_at", 0) > switched_at, any_window=True)
    probe.window = restored["window"]
    assert restored["model"]["selected_space"] == space, "workspace switch lost selected space"
    assert set(restored["visible"]) == set(tabs), "workspace switch lost tab membership"
    assert any(tab["id"] == tabs[0] and tab["pinned"] for tab in restored["model"].get("tabs", [])), "workspace switch lost pin"
    for tab in tabs:
        command = probe.intent({"AssignTab": {"id": tab, "space_id": original_space}})
        probe.wait(lambda state: state.get("command", 0) >= command)
    command = probe.intent({"PinTab": {"id": tabs[0], "pinned": False}})
    probe.wait(lambda state: state.get("command", 0) >= command)
    probe.intent({"SelectSpace": original_space})
    probe.wait(lambda state: state.get("model", {}).get("selected_space") == original_space and len(state.get("visible", [])) == len(tabs))
    return {"original_window": original_window, "other_window": other["window"], "original_mux": original_mux, "other_mux": other["mux_window"], "retained_space": space}


def edge_scenarios(probe):
    initial = probe.latest[probe.window]
    initial_size = initial["dimensions"]
    pane_ids = [[pane["id"] for pane in tab["panes"]] for tab in initial["tabs"]]
    initial_shape = pane_shape(initial)
    cell_width = initial["tabs"][0]["size"]["pixel_width"] / initial["tabs"][0]["size"]["cols"]
    probe.action("font_increase")
    probe.wait(lambda state: state.get("tabs") and state["tabs"][0]["size"]["pixel_width"] / state["tabs"][0]["size"]["cols"] > cell_width)
    probe.action("font_reset")
    probe.wait(lambda state: state.get("tabs") and state["tabs"][0]["size"]["pixel_width"] / state["tabs"][0]["size"]["cols"] == cell_width)
    for enabled in (True, False):
        probe.action("fullscreen")
        state = probe.wait(lambda state: state.get("dimensions", {}).get("is_full_screen") is enabled)
        assert state["content"]["width"] > 0 and state["content"]["height"] > 0, "fullscreen content dimensions are invalid"
        expected = state["model"]["settings"]["width"] * state["dimensions"]["dpi"] / 96
        assert abs(state["sidebar"]["width"] - expected) <= 1, "fullscreen changed logical sidebar width"
    for enabled in (True, False):
        probe.action("zoom")
        probe.wait(lambda state: any(tab["active"] and tab["zoomed"] is enabled for tab in state.get("tabs", [])))
    assert pane_shape(probe.latest[probe.window]) == initial_shape, "font/fullscreen/zoom roundtrip changed split sizing"
    probe.intent({"SetSetting": {"key": "side", "value": "right"}})
    right = probe.wait(lambda state: state.get("model", {}).get("settings", {}).get("side") == "right" and state.get("sidebar", {}).get("x", 0) > 0)
    assert abs(right["sidebar"]["x"] - right["content"]["x"] - right["content"]["width"]) <= 1, "right sidebar overlaps terminal"
    if probe.env["WEZ_VTABS_SCENARIO_CHROME"] == "1":
        assert right["sidebar"]["y"] > 0 and right["sidebar"]["y"] == right["content"]["y"], "integrated chrome not reserved"
    for rail, expected in (("collapsed", "rail_width"), ("hidden", None), ("expanded", "width")):
        command = probe.intent({"SetSetting": {"key": "rail", "value": rail}})
        state = probe.wait(lambda state: state.get("command", 0) >= command and state.get("model", {}).get("settings", {}).get("rail") == rail)
        width = state["model"]["settings"][expected] * state["dimensions"]["dpi"] / 96 if expected else 0
        assert abs(state["sidebar"]["width"] - width) <= 1, "rail reservation is stale"
    command = probe.send("resize", {"width": 150, "height": 120})
    tiny = probe.wait(lambda state: state.get("command", 0) >= command and state.get("dimensions", {}).get("pixel_width", 9999) < initial_size["pixel_width"])
    probe.sample_for(0.25)
    tiny = probe.latest[probe.window]
    assert tiny["content"]["width"] > 0 and tiny["content"]["height"] >= 0, "tiny resize removes content reservation"
    assert tiny["sidebar"]["width"] >= 0 and tiny["sidebar"]["x"] + tiny["sidebar"]["width"] <= tiny["dimensions"]["pixel_width"] + 1, "tiny sidebar extends outside window"
    assert [[pane["id"] for pane in tab["panes"]] for tab in tiny["tabs"]] == pane_ids, "tiny resize changed pane topology"
    command = probe.intent({"SetSetting": {"key": "side", "value": "left"}})
    probe.wait(lambda state: state.get("command", 0) >= command and state.get("model", {}).get("settings", {}).get("side") == "left")
    command = probe.send("resize", {"width": initial_size["pixel_width"], "height": initial_size["pixel_height"]})
    restored = probe.wait(lambda state: state.get("command", 0) >= command and state.get("dimensions", {}).get("pixel_width") == initial_size["pixel_width"] and state.get("dimensions", {}).get("pixel_height") == initial_size["pixel_height"])
    assert [[pane["id"] for pane in tab["panes"]] for tab in restored["tabs"]] == pane_ids, "restoring tiny window changed pane identities"
    return {"right_x": right["sidebar"]["x"], "chrome_height": right["sidebar"]["y"], "tiny_size": tiny["dimensions"], "tiny_content": tiny["content"], "tiny_terminal": tiny["tabs"][0]["size"], "restored_split": pane_shape(restored), "font_fullscreen_zoom": True}


def stock_tiny_scenario(probe, native_edges):
    initial = probe.latest[probe.window]
    dimensions = initial["dimensions"]
    terminal = initial["tabs"][0]["size"]
    target = native_edges["tiny_terminal"]
    command = probe.send("resize", {
        "width": dimensions["pixel_width"] - terminal["pixel_width"] + target["pixel_width"],
        "height": dimensions["pixel_height"] - terminal["pixel_height"] + target["pixel_height"],
    })
    probe.wait(lambda state: state.get("command", 0) >= command and all(tab["size"]["cols"] == target["cols"] and tab["size"]["rows"] == target["rows"] for tab in state.get("tabs", [])))
    command = probe.send("resize", {"width": dimensions["pixel_width"], "height": dimensions["pixel_height"]})
    restored = probe.wait(lambda state: state.get("command", 0) >= command and all(tab["size"]["cols"] == terminal["cols"] and tab["size"]["rows"] == terminal["rows"] for tab in state.get("tabs", [])))
    shape = pane_shape(restored)
    assert shape == native_edges["restored_split"], "tiny-window split rounding differs from stock"
    return shape


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gui", type=Path, required=True)
    parser.add_argument("--helper", type=Path)
    parser.add_argument("--mux-server", type=Path)
    parser.add_argument("--baseline-gui", type=Path)
    parser.add_argument("--geometry-only", action="store_true")
    parser.add_argument("--workspace", action="store_true", help="also check suspended native workspace state")
    parser.add_argument("--chrome", action="store_true", help="use integrated title controls and nonzero padding")
    parser.add_argument("--edge-cases", action="store_true", help="check fonts, fullscreen, zoom, right edge, rail states and tiny windows")
    parser.add_argument("--trace-mux", action="store_true", help="record client/server resize traces")
    parser.add_argument("--hooks", action="store_true", help="enable all Lua hooks and process routing metadata")
    parser.add_argument("--effects", action="store_true", help="retain default TachyonFX transitions")
    parser.add_argument("--resize-rounds", type=int, default=1, help="repeat the dense resize/reversal sequence in one window")
    parser.add_argument("--capture", action="store_true", help="capture only this runner's macOS GUI window")
    parser.add_argument("--domain", choices=("local", "unix", "ssh"), default="local")
    parser.add_argument("--ssh-config", type=Path, help="SshDomain JSON for a disposable SSH-mux fixture")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if not 1 <= args.resize_rounds <= 20:
        parser.error("--resize-rounds must be between 1 and 20")
    if args.domain == "ssh" and not args.ssh_config:
        parser.error("--domain ssh requires --ssh-config for a disposable test mux")
    output = (args.output or Path(tempfile.mkdtemp(prefix="vtabs-native-scenarios-"))).resolve()
    output.mkdir(parents=True, exist_ok=True)
    report = {"domain": args.domain, "boundary": "GUI state samples and native actions; OS drag smoothness, GPU presentation and physical key latency require visual/profiling checks"}
    probe = Probe(output / "native", args.gui.resolve(), args.helper.resolve() if args.helper else None, args.domain, args.ssh_config, capture=args.capture, server=args.mux_server, chrome=args.chrome, trace_mux=args.trace_mux, hooks=args.hooks, effects=args.effects, resize_rounds=args.resize_rounds)
    try:
        report["geometry"] = geometry_scenarios(probe)
        if args.workspace:
            report["workspace"] = workspace_scenario(probe)
        if args.edge_cases:
            report["edges"] = edge_scenarios(probe)
        if not args.geometry_only:
            report["features"] = feature_scenarios(probe)
        report["native_samples"] = len(probe.samples)
    except BaseException as error:
        report["error"] = str(error)
        raise
    finally:
        probe.close()
        (output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(output)
    if args.baseline_gui:
        baseline = Probe(output / "stock", args.baseline_gui.resolve(), None, args.domain, args.ssh_config, native=False, capture=args.capture, server=args.mux_server, chrome=args.chrome, initial_size=report["geometry"]["initial_size"], trace_mux=args.trace_mux, resize_rounds=args.resize_rounds)
        try:
            report["stock_geometry"] = geometry_scenarios(baseline)
            common = set(report["geometry"]["resize_steps"]) & set(report["stock_geometry"]["resize_steps"])
            assert len(common) >= 5, "insufficient matching native/stock resize samples"
            for step in common:
                assert report["geometry"]["resize_steps"][step] == report["stock_geometry"]["resize_steps"][step], f"split distribution differs from stock at resize step {step}"
            if "edges" in report:
                report["stock_tiny_restored"] = stock_tiny_scenario(baseline, report["edges"])
        finally:
            baseline.close()
            (output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    print("native scenarios passed")


if __name__ == "__main__":
    main()
