"""Sidebar UI driven by real keyboard and pointer input on owned displays."""

import json
import subprocess
from contextlib import suppress

import pytest

from tests.scenarios.ui_scenarios import scenarios
from tests.support.gui import GuiInput


@pytest.mark.gui
def test_color_picker_previews_applies_and_cancels(probe_factory, tmp_path):
    probe = probe_factory("color-picker", initial_size={"cols": 110, "rows": 42})
    gui = GuiInput(probe, tmp_path)

    def open_accent():
        gui.click('Setting("accent")')
        probe.wait(lambda state: gui.hit(state, "ColorPlane") is not None)

    original = probe.start()["model"]["settings"]["accent"]
    gui.attach()
    gui.click("Settings")
    gui.click('SettingsCategory("theme")')
    open_accent()
    gui.capture("color-picker")
    gui.click("ColorSwatch(17)")
    assert gui.state()["model"]["settings"]["accent"] == original
    gui.capture("color-picker-preview")
    gui.click("Submit")
    probe.wait(lambda state: state["model"]["settings"]["accent"] == "#3b82f6")
    probe.wait(lambda state: gui.hit(state, "ColorPlane") is None)

    open_accent()
    gui.click("ColorFormat")
    gui.text("255, 128, 64")
    gui.capture("color-picker-rgb")
    gui.key("Return")
    probe.wait(lambda state: state["model"]["settings"]["accent"] == "#ff8040")
    probe.wait(lambda state: gui.hit(state, "ColorPlane") is None)

    open_accent()
    gui.drag("ColorPlane", "ColorHue")
    gui.capture("color-picker-drag")
    gui.click("Cancel")
    probe.wait(lambda state: gui.hit(state, "ColorPlane") is None)
    assert gui.state()["model"]["settings"]["accent"] == "#ff8040"

    open_accent()
    gui.text("#oops")
    gui.key("Return")
    gui.capture("color-picker-invalid")
    assert gui.hit(gui.state(), "ColorPlane") is not None
    assert gui.state()["model"]["settings"]["accent"] == "#ff8040"
    gui.key("Escape")
    assert gui.state()["model"]["settings_page"]


@pytest.mark.gui
def test_native_launcher_configuration_uses_sidebar_and_preserves_spawn_options(
    probe_factory, tmp_path
):
    probe = probe_factory(
        "launcher",
        extra_lua=r"""
cfg.launch_menu = {
  {label='Unused tool', args={'/bin/sh'}},
  {
    label='Build server shell',
    args={'/bin/sh','-c','printf "%s\n%s\n" "$PWD" "$LAUNCH_MARKER" > "$LAUNCH_RESULT"; exec /bin/sh'},
    cwd=root..'/launch-cwd',
    domain={DomainName='local'},
    set_environment_variables={LAUNCH_MARKER='configured',LAUNCH_RESULT=root..'/launcher-result'},
  },
}
table.insert(cfg.keys,{key='F6',action=wezterm.action.ShowLauncher})
table.insert(cfg.keys,{key='F7',action=wezterm.action.ShowLauncherArgs{
  flags='FUZZY|LAUNCH_MENU_ITEMS',title='My tools',fuzzy_help_text='Find a tool',
}})
table.insert(cfg.keys,{key='F8',action=wezterm.action.ShowLauncherArgs{flags='FUZZY'}})
table.insert(cfg.keys,{key='F9',action=wezterm.action_callback(function(window,pane)
  window:set_config_overrides{launch_menu={{label='Window tool',args={'/bin/sh'}}}}
  window:perform_action(wezterm.action.ShowLauncherArgs{flags='FUZZY|LAUNCH_MENU_ITEMS'},pane)
end)})
""",
    )
    gui = GuiInput(probe, tmp_path)
    working_directory = probe.root / "launch-cwd"
    working_directory.mkdir()

    def entries(state):
        return [
            hit["id"]
            for hit in state.get("model", {}).get("hits", [])
            if hit["id"].startswith('Menu("command/')
        ]

    def dismiss():
        gui.key("Escape")
        probe.wait(lambda state: GuiInput.hit(state, "Editor") is None)

    initial = probe.start()
    gui.attach()
    # Both native actions open our UI, whose hit regions are observable here.
    gui.key("F6")
    probe.wait(lambda state: len(entries(state)) > 2)
    dismiss()
    gui.key("F8")
    empty = probe.wait(lambda state: GuiInput.hit(state, "Editor") is not None)
    assert entries(empty) == []  # FUZZY alone never adds categories.
    dismiss()
    gui.key("F7")
    probe.wait(lambda state: len(entries(state)) == 2)
    gui.text("bsvsh")
    probe.wait(lambda state: entries(state) == ['Menu("command/1")'])
    gui.capture("configured-launcher")
    gui.key("Return")
    probe.wait(lambda state: (probe.root / "launcher-result").exists())
    assert (probe.root / "launcher-result").read_text().splitlines() == [
        str(working_directory),
        "configured",
    ]
    probe.wait(lambda state: len(state["tabs"]) == len(initial["tabs"]) + 1)
    probe.wait(lambda state: GuiInput.hit(state, "Editor") is None)
    # Read effective window configuration each time, including Lua overrides.
    gui.key("F9")
    probe.wait(lambda state: entries(state) == ['Menu("command/0")'])
    gui.text("Window tool")
    probe.wait(lambda state: entries(state) == ['Menu("command/0")'])
    dismiss()


@pytest.mark.gui
def test_keyboard_pointer_and_clipboard(probe_factory, tmp_path):
    probe = probe_factory("gui", effects=True, initial_size={"cols": 110, "rows": 40})
    gui = GuiInput(probe, tmp_path)
    try:
        report = scenarios(probe, gui)
        (tmp_path / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        assert report["errors"] == []
    except Exception as error:
        (tmp_path / "report.json").write_text(
            json.dumps(
                {"passed": False, "error": str(error), "screenshots": gui.captures}, indent=2
            )
            + "\n"
        )
        if gui.window is not None:
            with suppress(OSError, subprocess.SubprocessError, AssertionError):
                gui.capture("failure", pause=0)
        raise
