#!/usr/bin/env python3
"""Opt-in Linux GUI keyboard, pointer, drag, and screenshot checks on a private display."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import sys
import tempfile
import time

from scenarios import Probe, pane_shape


class PrivateDisplay:
    def __init__(self, root: Path):
        self.root = root
        self.processes = []
        self.outputs = []
        self.env = os.environ.copy()
        self.env.pop("WAYLAND_DISPLAY", None)

    def start(self):
        self.root.mkdir()
        for binary in ("Xvfb", "openbox", "xdotool", "import"):
            if not shutil.which(binary):
                raise RuntimeError(f"Required GUI fixture command is missing: {binary}")
        read_fd, write_fd = os.pipe()
        output = (self.root / "xvfb.log").open("wb")
        self.outputs.append(output)
        try:
            server = subprocess.Popen(
                ["Xvfb", "-displayfd", str(write_fd), "-screen", "0", "1440x1000x24",
                 "-nolisten", "tcp", "-noreset"],
                pass_fds=(write_fd,), stdin=subprocess.DEVNULL,
                stdout=output, stderr=subprocess.STDOUT,
            )
            self.processes.append(server)
            os.close(write_fd)
            write_fd = None
            if not select.select([read_fd], [], [], 10)[0]:
                raise RuntimeError("Private Xvfb display did not become ready")
            display = os.read(read_fd, 64).decode().strip()
            if not display.isdecimal():
                raise RuntimeError("Private Xvfb did not return a display number")
        finally:
            os.close(read_fd)
            if write_fd is not None:
                os.close(write_fd)
        self.env["DISPLAY"] = f":{display}"
        config = self.root / "openbox.xml"
        config.write_text(
            '<?xml version="1.0"?><openbox_config xmlns="http://openbox.org/3.4/rc">'
            '<focus><focusNew>yes</focusNew></focus><keyboard/><mouse/>'
            '</openbox_config>\n', encoding="utf-8",
        )
        output = (self.root / "openbox.log").open("wb")
        self.outputs.append(output)
        self.processes.append(subprocess.Popen(
            ["openbox", "--sm-disable", "--config-file", str(config)], env=self.env,
            stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT,
        ))
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if subprocess.run(["xdotool", "get_desktop"], env=self.env, capture_output=True).returncode == 0:
                return self
            time.sleep(.05)
        raise RuntimeError("Private window manager did not become ready")

    def close(self):
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


class GuiInput:
    def __init__(self, probe: Probe, display: PrivateDisplay, output: Path):
        self.probe, self.display, self.output = probe, display, output
        self.window = None
        self.captures = []

    def command(self, *args):
        return subprocess.run(["xdotool", *map(str, args)], env=self.display.env,
                              check=True, text=True, capture_output=True, timeout=10).stdout.strip()

    def attach(self):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            result = subprocess.run(
                ["xdotool", "search", "--onlyvisible", "--class", self.probe.identity],
                env=self.display.env, text=True, capture_output=True,
            )
            if result.returncode == 0 and result.stdout.strip():
                windows = result.stdout.splitlines()
                if len(windows) != 1:
                    raise AssertionError(f"Expected one owned GUI window, found {windows}")
                self.window = windows[0]
                self.command("windowactivate", "--sync", self.window)
                return
            time.sleep(.05)
        raise AssertionError("Owned GUI window was not mapped on the private display")

    def key(self, chord):
        self.command("key", "--clearmodifiers", chord)

    def text(self, text):
        self.command("type", "--clearmodifiers", "--delay", "18", "--", text)

    def state(self):
        self.probe.read()
        return self.probe.latest[self.probe.window]

    @staticmethod
    def hit(state, identifier):
        return next((hit for hit in state.get("model", {}).get("hits", [])
                     if hit["id"] == identifier), None)

    def hit_position(self, identifier):
        state = self.probe.wait(lambda state: self.hit(state, identifier) is not None)
        hit = self.hit(state, identifier)
        tab = state["tabs"][0]["size"]
        cell_width = tab["pixel_width"] / tab["cols"]
        cell_height = tab["pixel_height"] / tab["rows"]
        bounds = state["sidebar"]
        x, y = bounds["x"], bounds["y"]
        if state["model"].get("settings_page"):
            x, y = min(x, state["content"]["x"]), min(y, state["content"]["y"])
        return (round(x + (hit["x"] + hit["width"] / 2) * cell_width),
                round(y + (hit["y"] + hit["height"] / 2) * cell_height))

    def point(self, x, y):
        self.command("mousemove", "--window", self.window, x, y)

    def hover(self, identifier):
        self.point(*self.hit_position(identifier))

    def click(self, identifier):
        self.hover(identifier)
        self.command("click", "1")

    def drag(self, source, target):
        start, end = self.hit_position(source), self.hit_position(target)
        self.point(*start)
        self.command("mousedown", "1")
        try:
            for index in range(1, 13):
                fraction = index / 12
                self.point(round(start[0] + (end[0] - start[0]) * fraction),
                           round(start[1] + (end[1] - start[1]) * fraction))
                time.sleep(.018)
        finally:
            self.command("mouseup", "1")

    def capture(self, name, pause=.18):
        self.probe.sample_for(pause)
        path = self.output / f"{name}.png"
        subprocess.run(["import", "-display", self.display.env["DISPLAY"], "-window",
                        self.window, str(path)], env=self.display.env,
                       check=True, capture_output=True, timeout=10)
        assert path.is_file() and path.stat().st_size > 1000, "GUI capture is empty"
        self.captures.append(str(path))
        return path

    def start_input_capture(self):
        self.focus_terminal()
        self.text(r'printf "\033[2J\033[HWorkspace ready\n\n"; stty -echo -icanon min 1 time 0; cat > "$WEZ_VTABS_SCENARIO/physical-input.bin"')
        self.key("Return")
        path = self.probe.root / "physical-input.bin"
        deadline = time.monotonic() + 5
        while not path.exists():
            if time.monotonic() > deadline:
                raise AssertionError("Physical terminal input capture did not start")
            time.sleep(.02)
        return path

    def focus_terminal(self):
        state = self.state()
        content = state["content"]
        self.point(round(content["x"] + content["width"] / 2),
                   round(content["y"] + content["height"] / 2))
        self.command("click", "1")


def intent(probe, value):
    command = probe.intent(value)
    return probe.wait(lambda state: state.get("command", 0) >= command)


def scenarios(probe: Probe, gui: GuiInput):
    errors = []

    def shortcut(chord, predicate, fallback):
        gui.key(chord)
        try:
            return probe.wait(predicate, timeout=2)
        except AssertionError:
            errors.append(f"Shortcut {chord} did not trigger its UI action")
            gui.click(fallback)
            return probe.wait(predicate)

    state = probe.start()
    gui.attach()
    if "hits" not in state.get("model", {}):
        raise AssertionError("GUI build does not expose native UI hit regions")
    home = state["model"]["selected_space"]
    first = state["tabs"][0]["id"]
    intent(probe, {"RenameSpace": {"id": home, "name": "Home"}})
    intent(probe, {"CreateSpace": {"name": "Work"}})
    work = probe.wait(lambda state: state["model"]["selected_space"] != home)["model"]["selected_space"]
    intent(probe, {"AssignTab": {"id": first, "space_id": work}})
    intent(probe, {"RenameTab": {"id": first, "title": "Editor"}})
    intent(probe, {"PinTab": {"id": first, "pinned": True}})
    ids = [first]
    for name in ("Shell", "Logs"):
        gui.key("ctrl+shift+t")
        state = probe.wait(lambda state: len(state.get("tabs", [])) == len(ids) + 1)
        created = next(tab["id"] for tab in state["tabs"] if tab["id"] not in ids)
        ids.append(created)
        intent(probe, {"RenameTab": {"id": created, "title": name}})
    gui.click("NewTab")
    state = probe.wait(lambda state: len(state.get("tabs", [])) == len(ids) + 1)
    created = next(tab["id"] for tab in state["tabs"] if tab["id"] not in ids)
    ids.append(created)
    intent(probe, {"RenameTab": {"id": created, "title": "Build"}})
    gui.drag(f"Tab({ids[3]})", f"Tab({ids[2]})")
    probe.wait(lambda state: ids[3] in state.get("visible", [])
               and ids[2] in state.get("visible", [])
               and state["visible"].index(ids[3]) < state["visible"].index(ids[2]))
    gui.click("CreateFolder")
    probe.wait(lambda state: gui.hit(state, "Editor") is not None)
    gui.text("Project")
    gui.key("Return")
    state = probe.wait(lambda state: any(folder["name"] == "Project" for folder in state["model"].get("folders", [])))
    folder = next(folder["id"] for folder in state["model"]["folders"] if folder["name"] == "Project")
    folder_hit = f"Folder({json.dumps(folder)})"
    probe.wait(lambda state: gui.hit(state, folder_hit) is not None)
    gui.drag(f"Tab({ids[1]})", folder_hit)
    probe.wait(lambda state: any(tab["id"] == ids[1] and tab.get("folder_id") == folder for tab in state["model"].get("tabs", [])))
    gui.click(folder_hit)
    probe.wait(lambda state: gui.hit(state, f"Tab({ids[1]})") is None)
    gui.click(folder_hit)
    probe.wait(lambda state: gui.hit(state, f"Tab({ids[1]})") is not None)
    gui.click(f"Tab({ids[1]})")
    probe.wait(lambda state: state.get("active") == ids[1])
    gui.click(f"Tab({first})")
    probe.wait(lambda state: state.get("active") == first)
    capture = gui.start_input_capture()
    gui.capture("sidebar")
    gui.hover("Settings")
    gui.capture("tooltip", pause=.9)
    before = gui.state()
    geometry, topology = (before["sidebar"], before["content"]), pane_shape(before)
    gui.click("Settings")
    state = probe.wait(lambda state: state["model"].get("settings_page") is True)
    assert (state["sidebar"], state["content"]) == geometry, "Settings changed terminal reservation"
    assert pane_shape(state) == topology, "Settings resized terminal panes"
    assert {tab["id"] for tab in state["tabs"]} == set(ids), "Settings replaced terminal tabs"
    gui.capture("settings")
    gui.key("ctrl+f")
    gui.text("motion")
    probe.wait(lambda state: gui.hit(state, 'Setting("reduced_motion")') is not None and gui.hit(state, 'Setting("width")') is None)
    gui.capture("settings-search")
    gui.key("Escape")
    gui.key("Escape")
    probe.wait(lambda state: not state["model"].get("settings_page"))
    shortcut("ctrl+shift+comma", lambda state: state["model"].get("settings_page") is True, "Settings")
    gui.key("Escape")
    probe.wait(lambda state: not state["model"].get("settings_page"))
    shortcut("ctrl+shift+k", lambda state: gui.hit(state, "Editor") is not None, "Search")
    gui.text("Logs")
    probe.wait(lambda state: gui.hit(state, f'Menu("tab/{ids[2]}")') is not None and gui.hit(state, f'Menu("tab/{first}")') is None)
    gui.capture("search")
    gui.key("Return")
    probe.wait(lambda state: state.get("active") == ids[2])
    gui.click(f"Tab({first})")
    probe.wait(lambda state: state.get("active") == first)
    gui.focus_terminal()
    shortcut("ctrl+shift+b", lambda state: state["model"]["settings"]["rail"] == "collapsed", "Rail")
    gui.focus_terminal()
    state = shortcut("ctrl+shift+b", lambda state: state["model"]["settings"]["rail"] == "expanded", "Rail")
    assert {tab["id"] for tab in state["tabs"]} == set(ids), "Sidebar toggle lost tabs"
    gui.focus_terminal()
    shortcut("super+comma", lambda state: state["model"].get("settings_page") is True, "Settings")
    gui.key("Escape")
    probe.wait(lambda state: not state["model"].get("settings_page"))
    probe.sample_for(.2)
    leaked = capture.read_bytes()
    if leaked:
        errors.append(f"UI input leaked into terminal: {leaked!r}")
    gui.focus_terminal()
    gui.key("ctrl+c")
    gui.text('printf "VTABS_TERMINAL_KEYS_OK\\n" > "$WEZ_VTABS_SCENARIO/terminal-keys-ok"')
    gui.key("Return")
    terminal_keys = probe.root / "terminal-keys-ok"
    deadline = time.monotonic() + 5
    while not terminal_keys.exists():
        if time.monotonic() > deadline:
            raise AssertionError("Plain Ctrl+C and terminal text were not delivered after leaving the UI")
        time.sleep(.02)
    assert terminal_keys.read_text() == "VTABS_TERMINAL_KEYS_OK\n"
    return {"tabs": ids, "folder": folder, "spaces": [home, work],
            "keyboard": ["Ctrl+Shift+T", "Ctrl+Shift+Comma", "Ctrl+Shift+K", "Ctrl+Shift+B", "Super+Comma", "Escape"],
            "mouse": ["new tab", "tab activation", "folder child activation", "folder collapse and expand", "tab reorder", "drag tab into folder", "settings", "tooltip hover"],
            "settings_preserve_geometry": True, "terminal_input_leaked_bytes": len(leaked),
            "errors": errors,
            "ordinary_terminal_input_preserved": True,
            "screenshots": gui.captures, "samples": len(probe.samples)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gui", type=Path, required=True)
    parser.add_argument("--helper", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if sys.platform != "linux":
        parser.error("This isolated Xvfb fixture requires Linux")
    output = (args.output or Path(tempfile.mkdtemp(prefix="vtabs-ui-input-"))).resolve()
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        parser.error("--output must be an empty directory")
    binary = args.gui.resolve()
    helper = args.helper.resolve() if args.helper else binary.with_name("wez-vtabs-store")
    if not binary.is_file() or not helper.is_file():
        parser.error("GUI and wez-vtabs-store binaries must exist")
    display, probe, gui = PrivateDisplay(output / "display"), None, None
    report = {"gui": str(binary), "gui_modified_ns": binary.stat().st_mtime_ns,
              "isolation": "private Xvfb display and owned native GUI process"}
    try:
        display.start()
        probe = Probe(output / "native", binary, helper, "local", effects=True,
                      initial_size={"cols": 110, "rows": 40})
        probe.env.update({"DISPLAY": display.env["DISPLAY"], "GDK_BACKEND": "x11"})
        probe.env.pop("WAYLAND_DISPLAY", None)
        probe.env.pop("SSH_AUTH_SOCK", None)
        gui = GuiInput(probe, display, output)
        report.update(scenarios(probe, gui))
        if report["errors"]:
            raise AssertionError("; ".join(report["errors"]))
        report["passed"] = True
    except BaseException as error:
        report["passed"], report["error"] = False, str(error)
        if gui is not None and gui.window is not None:
            try:
                gui.capture("failure", pause=0)
            except Exception:
                pass
            report["screenshots"] = gui.captures
        raise
    finally:
        if probe is not None:
            probe.close()
        display.close()
        (output / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(output, flush=True)


if __name__ == "__main__":
    main()
