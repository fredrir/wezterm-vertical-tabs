#!/usr/bin/env python3
"""Opt-in Linux GUI keyboard, pointer, drag, and screenshot checks on a private display."""

from __future__ import annotations

import argparse
import json
import sys
import tempfile
from pathlib import Path

if __package__ in {None, ""}:
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from tests.support.gui import GuiInput
from tests.support.headless import HeadlessDisplay
from tests.support.polling import check_process, log_diagnostics, wait_until
from tests.support.probe import Probe, pane_shape


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
        raise AssertionError("GUI build does not expose UI hit regions")
    home = state["model"]["selected_space"]
    first = state["tabs"][0]["id"]
    intent(probe, {"RenameSpace": {"id": home, "name": "Home"}})
    intent(probe, {"CreateSpace": {"name": "Work"}})
    work = probe.wait(lambda state: state["model"]["selected_space"] != home)["model"][
        "selected_space"
    ]
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
    # A row's middle joins the tabs as a split; its top edge reorders.
    gui.drag(f"Tab({ids[3]})", f"Tab({ids[2]})", y_fraction=0.15)
    probe.wait(
        lambda state: (
            ids[3] in state.get("visible", [])
            and ids[2] in state.get("visible", [])
            and state["visible"].index(ids[3]) < state["visible"].index(ids[2])
        )
    )
    gui.hover("SpaceTitle")
    gui.click("CreateFolder")
    probe.wait(lambda state: gui.hit(state, "Editor") is not None)
    gui.text("Project")
    gui.key("Return")
    state = probe.wait(
        lambda state: any(
            folder["name"] == "Project" for folder in state["model"].get("folders", [])
        )
    )
    folder = next(
        folder["id"] for folder in state["model"]["folders"] if folder["name"] == "Project"
    )
    folder_hit = f"Folder({json.dumps(folder)})"
    probe.wait(lambda state: gui.hit(state, folder_hit) is not None)
    gui.drag(f"Tab({ids[1]})", folder_hit)
    probe.wait(
        lambda state: any(
            tab["id"] == ids[1] and tab.get("folder_id") == folder
            for tab in state["model"].get("tabs", [])
        )
    )
    gui.click(folder_hit)
    probe.wait(lambda state: gui.hit(state, f"Tab({ids[1]})") is None)
    gui.click(folder_hit)
    probe.wait(lambda state: gui.hit(state, f"Tab({ids[1]})") is not None)
    gui.click(f"Tab({ids[1]})")
    probe.wait(lambda state: state.get("active") == ids[1])
    gui.click(f"Tab({first})")
    probe.wait(lambda state: state.get("active") == first)
    capture = gui.start_input_capture()
    terminal_pixels = gui.terminal_text_pixels(gui.capture("sidebar"))
    # Tooltips open beside the sidebar, level with their control; this one clears the text.
    gui.hover(f"Space({json.dumps(home)})")
    assert gui.terminal_text_pixels(gui.capture("tooltip", pause=0.9)) == terminal_pixels, (
        "Tooltip changed uncovered terminal text"
    )
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
    probe.wait(
        lambda state: (
            gui.hit(state, 'Setting("reduced_motion")') is not None
            and gui.hit(state, 'Setting("width")') is None
        )
    )
    gui.capture("settings-search")
    gui.key("ctrl+a")
    gui.key("ctrl+c")
    gui.text("no matching settings")
    gui.key("ctrl+a")
    gui.key("ctrl+v")
    gui.point(12, 12)
    probe.wait(
        lambda state: (
            gui.hit(state, 'Setting("reduced_motion")') is not None
            and gui.hit(state, 'Setting("width")') is None
        )
    )
    gui.key("ctrl+a")
    gui.text("accent")
    probe.wait(lambda state: gui.hit(state, 'Setting("accent")') is not None)
    gui.click('Setting("accent")')
    probe.wait(lambda state: gui.hit(state, "Editor") is not None)
    gui.key("ctrl+a")
    gui.text("#8eb4ed")
    gui.key("ctrl+a")
    gui.key("ctrl+c")
    gui.key("BackSpace")
    gui.key("ctrl+v")
    gui.point(12, 12)
    gui.key("Return")
    probe.wait(lambda state: state["model"]["settings"]["accent"] == "#8eb4ed")
    gui.capture("color-edited")
    # Like any tab, the page stays open on Escape and closes with the close shortcut.
    gui.key("Escape")
    probe.sample_for(0.2)
    if not gui.state()["model"].get("settings_page"):
        errors.append("Escape closed the settings page")
    gui.key("ctrl+shift+w")
    probe.wait(lambda state: not state["model"].get("settings_page"))
    shortcut(
        "ctrl+shift+comma",
        lambda state: state["model"].get("settings_page") is True,
        "Settings",
    )
    gui.key("ctrl+shift+w")
    probe.wait(lambda state: not state["model"].get("settings_page"))
    shortcut("ctrl+shift+k", lambda state: gui.hit(state, "Editor") is not None, "Search")
    gui.text("Logs")
    probe.wait(
        lambda state: (
            gui.hit(state, f'Menu("tab/{ids[2]}")') is not None
            and gui.hit(state, f'Menu("tab/{first}")') is None
        )
    )
    assert gui.terminal_text_pixels(gui.capture("search")) == terminal_pixels, (
        "Launcher changed uncovered terminal text"
    )
    gui.key("ctrl+a")
    gui.key("ctrl+c")
    gui.text("no matching tabs")
    probe.wait(lambda state: gui.hit(state, f'Menu("tab/{ids[2]}")') is None)
    gui.key("ctrl+a")
    gui.key("ctrl+v")
    gui.point(12, 12)
    probe.wait(
        lambda state: (
            gui.hit(state, f'Menu("tab/{ids[2]}")') is not None
            and gui.hit(state, f'Menu("tab/{first}")') is None
        )
    )
    gui.key("Return")
    probe.wait(lambda state: state.get("active") == ids[2])
    gui.click(f"Tab({first})")
    probe.wait(lambda state: state.get("active") == first)
    gui.focus_terminal()
    shortcut(
        "ctrl+shift+b",
        lambda state: state["model"]["settings"]["rail"] == "hidden",
        "Rail",
    )
    gui.focus_terminal()
    state = shortcut(
        "ctrl+shift+b",
        lambda state: state["model"]["settings"]["rail"] == "expanded",
        "Rail",
    )
    assert {tab["id"] for tab in state["tabs"]} == set(ids), "Sidebar toggle lost tabs"
    gui.focus_terminal()
    shortcut(
        "super+comma",
        lambda state: state["model"].get("settings_page") is True,
        "Settings",
    )
    gui.key("ctrl+shift+w")
    probe.wait(lambda state: not state["model"].get("settings_page"))
    probe.sample_for(0.2)
    leaked = capture.read_bytes()
    if leaked:
        errors.append(f"UI input leaked into terminal: {leaked!r}")
    gui.focus_terminal()
    gui.key("ctrl+c")
    gui.text('printf "VTABS_TERMINAL_KEYS_OK\\n" > "$WEZ_VTABS_SCENARIO/terminal-keys-ok"')
    gui.key("Return")
    terminal_keys = probe.root / "terminal-keys-ok"
    wait_until(
        terminal_keys.exists,
        timeout=5,
        interval=0.02,
        message="Plain Ctrl+C and terminal text were not delivered after leaving the UI",
        check_live=lambda: check_process(probe.gui_process, "GUI"),
        diagnostics=lambda: log_diagnostics(probe.root / "gui.log"),
    )
    assert terminal_keys.read_text() == "VTABS_TERMINAL_KEYS_OK\n"
    return {
        "tabs": ids,
        "folder": folder,
        "spaces": [home, work],
        "keyboard": [
            "Ctrl+Shift+T",
            "Ctrl+Shift+Comma",
            "Ctrl+Shift+K",
            "Ctrl+Shift+B",
            "Super+Comma",
            "Escape",
        ],
        "mouse": [
            "new tab",
            "tab activation",
            "folder child activation",
            "folder collapse and expand",
            "tab reorder",
            "drag tab into folder",
            "settings",
            "tooltip hover",
        ],
        "settings_preserve_geometry": True,
        "clipboard": ["settings search", "color editor", "tab search"],
        "terminal_input_leaked_bytes": len(leaked),
        "errors": errors,
        "ordinary_terminal_input_preserved": True,
        "screenshots": gui.captures,
        "samples": len(probe.samples),
    }


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
    display, probe, gui = HeadlessDisplay(output / "display"), None, None
    report = {
        "gui": str(binary),
        "gui_modified_ns": binary.stat().st_mtime_ns,
        "isolation": "private Xvfb display and owned GUI process",
    }
    try:
        display.start()
        probe = Probe(
            output / "gui",
            binary,
            helper,
            "local",
            effects=True,
            initial_size={"cols": 110, "rows": 40},
            display=display,
        )
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
