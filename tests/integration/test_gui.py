"""Production startup, rendering, and shutdown on owned displays."""

import json
import subprocess
from contextlib import ExitStack, suppress
from pathlib import Path

import pytest

from tests.scenarios.scenarios import Probe
from tests.scenarios.tls_fixture import LocalTlsMux
from tests.scenarios.ui_scenarios import GuiInput, scenarios


@pytest.mark.gui
def test_native_launcher_configuration_uses_sidebar_and_preserves_spawn_options(
    wezterm_binaries, headless_display, tmp_path
):
    probe = Probe(
        tmp_path / "launcher",
        wezterm_binaries["wezterm-gui"],
        wezterm_binaries["wez-vtabs-store"],
        "local",
        display=headless_display,
    )
    gui = GuiInput(probe, headless_display, tmp_path)
    working_directory = probe.root / "launch-cwd"
    working_directory.mkdir()
    probe.config.write_text(
        probe.config.read_text().replace(
            "return cfg",
            r"""
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
return cfg
""",
        )
    )

    def entries(state):
        return [
            hit["id"]
            for hit in state.get("model", {}).get("hits", [])
            if hit["id"].startswith('Menu("command/')
        ]

    def dismiss():
        gui.key("Escape")
        probe.wait(lambda state: GuiInput.hit(state, "Editor") is None)

    try:
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
    finally:
        probe.close()


@pytest.mark.gui
@pytest.mark.parametrize("domain", ["local", "unix"])
def test_start_render_and_shutdown(wezterm_binaries, headless_display, tmp_path, domain):
    probe = Probe(
        tmp_path / domain,
        wezterm_binaries["wezterm-gui"],
        wezterm_binaries["wez-vtabs-store"],
        domain,
        server=wezterm_binaries["wezterm-mux-server"],
        chrome=True,
        effects=True,
        display=headless_display,
    )
    try:
        state = probe.start()
        assert state["visible"]
        assert state["content"]["width"] > 0
        probe.intent({"SetSetting": {"key": "width", "value": 300}})
        probe.wait(lambda current: current["model"]["settings"]["width"] == 300)
        probe.action("quit")
        probe.gui_process.wait(timeout=10)
        assert probe.gui_process.returncode == 0
        if domain == "unix":
            assert probe.processes[0].poll() is None, "quitting the GUI stopped the remote mux"
    finally:
        probe.close()
    assert not any(process.poll() is None for process in probe.processes)
    assert headless_display.owned
    (tmp_path / "result.json").write_text(json.dumps({"domain": domain, "shutdown": "clean"}))


@pytest.mark.gui
def test_keyboard_pointer_and_clipboard(wezterm_binaries, headless_display, tmp_path):
    probe = Probe(
        tmp_path / "gui",
        wezterm_binaries["wezterm-gui"],
        wezterm_binaries["wez-vtabs-store"],
        "local",
        effects=True,
        initial_size={"cols": 110, "rows": 40},
        display=headless_display,
    )
    gui = GuiInput(probe, headless_display, tmp_path)
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
    finally:
        probe.close()


@pytest.mark.gui
def test_mutual_tls_tab_lifecycle(wezterm_binaries, headless_display, tmp_path):
    with ExitStack() as cleanup:
        server = LocalTlsMux(tmp_path / "server", wezterm_binaries["wezterm-mux-server"])
        cleanup.callback(server.close)
        domain = server.start()
        assert server.protocol in {"TLSv1.2", "TLSv1.3"}
        probe = Probe(
            tmp_path / "client",
            wezterm_binaries["wezterm-gui"],
            wezterm_binaries["wez-vtabs-store"],
            "tls",
            tls_config=domain,
            display=headless_display,
        )
        cleanup.callback(probe.close)
        probe.start()
        probe.action("new_tab")
        probe.wait(lambda state: len(state["tabs"]) == 2)
        probe.action("close")
        probe.wait(lambda state: len(state["tabs"]) == 1)
    assert server.process.poll() is not None
    assert list(server.certificates.glob("*.key")) == []


@pytest.mark.gui
@pytest.mark.parametrize("hidden", [[], ["__backing:elsewhere"]])
def test_detaching_the_last_pane_quits_and_reconnecting_starts_fresh(
    wezterm_binaries, headless_display, tmp_path, hidden
):
    probe = Probe(
        tmp_path / "unix",
        wezterm_binaries["wezterm-gui"],
        wezterm_binaries["wez-vtabs-store"],
        "unix",
        server=wezterm_binaries["wezterm-mux-server"],
        display=headless_display,
    )
    mux_env = dict(probe.env, WEZTERM_UNIX_SOCKET=str(probe.root / "mux.sock"))

    def cli(*arguments):
        return subprocess.check_output(
            [wezterm_binaries["wezterm"], "--config-file", probe.config, "cli", *arguments],
            env=mux_env,
            text=True,
            timeout=5,
        )

    def panes():
        return json.loads(cli("list", "--format", "json"))

    def workspaces():
        return sorted(pane["workspace"] for pane in panes())

    try:
        workspace = probe.start()["workspace"]
        # The mux server's own startup pane would give the GUI a workspace to fall back to.
        for pane in panes():
            if pane["workspace"] != workspace:
                cli("kill-pane", "--pane-id", str(pane["pane_id"]))
        # Tabs backing another host's panes must not keep the GUI open or receive it.
        for name in hidden:
            cli("spawn", "--new-window", "--workspace", name)
        probe.sample_for(0.5)
        probe.send("detach")
        probe.gui_process.wait(timeout=10)
        assert probe.gui_process.returncode == 0
        assert probe.processes[0].poll() is None, "quitting the GUI stopped the mux"
        assert workspaces() == sorted(["__detached", *hidden])

        (probe.root / "command.json").unlink()
        probe.latest.clear()
        probe.gui_process = probe.start_process(
            [
                probe.gui,
                "--config-file",
                probe.config,
                "connect",
                "scenario-unix",
                "--class",
                probe.identity,
            ],
            "reconnect.log",
        )
        window = probe.wait(lambda state: state.get("tabs"), timeout=25, any_window=True)["window"]
        # The connection UI's tab closes once the domain attaches.
        probe.sample_for(0.5)
        state = probe.latest[window]
        assert state["workspace"] not in ["__detached", *hidden]
        assert len(state["tabs"]) == 1
        assert workspaces() == sorted(["__detached", "default", *hidden])
    finally:
        probe.close()


@pytest.mark.gui
def test_unix_attach_and_new_tab_agree_without_a_window_resize(
    wezterm_binaries, headless_display, tmp_path
):
    probe = Probe(
        tmp_path / "unix",
        wezterm_binaries["wezterm-gui"],
        wezterm_binaries["wez-vtabs-store"],
        "unix",
        server=wezterm_binaries["wezterm-mux-server"],
        chrome=True,
        initial_size={"cols": 100, "rows": 32},
        display=headless_display,
    )
    try:
        initial = probe.start()
        dimensions = initial["dimensions"]
        initial_size = initial["tabs"][0]["size"]
        probe.action("new_tab")
        created = probe.wait(lambda state: len(state.get("tabs", [])) == 2)
        samples = [created, *probe.sample_for(0.25)]
        for state in samples:
            if state["window"] != probe.window:
                continue
            assert state["dimensions"] == dimensions, "attaching or spawning resized the window"
            assert all(tab["size"] == initial_size for tab in state["tabs"]), (
                "remote attachment kept provisional geometry until the next physical resize"
            )
    finally:
        probe.close()


@pytest.mark.gui
@pytest.mark.parametrize("domain", ["local", "unix"])
def test_quick_terminal_preserves_its_shell_without_creating_a_tab(
    wezterm_binaries, headless_display, tmp_path, domain
):
    probe = Probe(
        tmp_path / domain,
        wezterm_binaries["wezterm-gui"],
        wezterm_binaries["wez-vtabs-store"],
        domain,
        server=wezterm_binaries["wezterm-mux-server"],
        initial_size={"cols": 110, "rows": 40},
        display=headless_display,
    )
    gui = GuiInput(probe, headless_display, tmp_path)
    probe.config.write_text(
        probe.config.read_text().replace(
            "cfg.exit_behavior = 'Close'", "cfg.exit_behavior = 'CloseOnCleanExit'"
        )
    )
    try:
        initial = probe.start()
        gui.attach()
        gui.key("ctrl+grave")
        opened = probe.wait(
            lambda state: (
                state["quick_terminal"]["visible"] and state["quick_terminal"]["pane"] is not None
            )
        )
        terminal = opened["quick_terminal"]
        pane = terminal["pane"]
        assert opened["tabs"] == initial["tabs"]
        assert opened["active"] == initial["active"]
        width = opened["sidebar"]["width"] + opened["content"]["width"]
        assert terminal["bounds"]["width"] == pytest.approx(width * 0.75)
        gui.text(
            'QT_SESSION=retained; (sleep 0.3; printf alive > "$WEZ_VTABS_SCENARIO/quick-alive") &'
        )
        gui.key("Return")
        gui.key("ctrl+grave")
        probe.wait(lambda state: not state["quick_terminal"]["visible"])
        probe.wait(lambda state: (probe.root / "quick-alive").exists())
        gui.key("ctrl+grave")
        probe.wait(lambda state: state["quick_terminal"]["visible"])
        gui.text('printf "%s" "$QT_SESSION" > "$WEZ_VTABS_SCENARIO/quick-session"')
        gui.key("Return")
        probe.wait(lambda state: (probe.root / "quick-session").exists())
        assert (probe.root / "quick-session").read_text() == "retained"
        assert gui.state()["quick_terminal"]["pane"] == pane
        gui.capture(f"quick-terminal-{domain}")

        # A sidebar click dismisses the overlay without creating a tab beneath it.
        gui.click("NewTab")
        hidden = probe.wait(lambda state: not state["quick_terminal"]["visible"])
        assert hidden["tabs"] == initial["tabs"]
        assert hidden["quick_terminal"]["pane"] == pane
        probe.intent("quick_terminal")
        probe.wait(lambda state: state["quick_terminal"]["visible"])
        gui.command("windowminimize", gui.window)
        probe.wait(lambda state: not state["quick_terminal"]["visible"])
        gui.command("windowactivate", "--sync", gui.window)
        gui.key("ctrl+grave")
        probe.wait(lambda state: state["quick_terminal"]["visible"])
        probe.send("resize", {"width": 900, "height": 600})
        resized = probe.wait(
            lambda state: (
                state["dimensions"]["pixel_width"] == 900
                and state["quick_terminal"]["cols"] != terminal["cols"]
            )
        )
        assert resized["quick_terminal"]["pane"] == pane
        gui.text('stty size > "$WEZ_VTABS_SCENARIO/quick-size"')
        gui.key("Return")
        probe.wait(lambda state: (probe.root / "quick-size").exists())
        assert [int(n) for n in (probe.root / "quick-size").read_text().split()] == [
            resized["quick_terminal"]["rows"],
            resized["quick_terminal"]["cols"],
        ]

        # Escape dismisses the overlay and keeps its shell running.
        gui.key("Escape")
        probe.wait(lambda state: not state["quick_terminal"]["visible"])
        assert gui.state()["quick_terminal"]["pane"] == pane
        gui.key("ctrl+grave")
        probe.wait(lambda state: state["quick_terminal"]["visible"])

        probe.action("navigator")
        probe.wait(
            lambda state: (
                not state["quick_terminal"]["visible"] and gui.hit(state, "Editor") is not None
            )
        )
        gui.key("Escape")
        gui.key("ctrl+grave")
        probe.wait(lambda state: state["quick_terminal"]["visible"])
        assert gui.state()["quick_terminal"]["pane"] == pane

        # Exiting the shell hides the surface; the next invocation starts a new shell.
        gui.text("exit")
        gui.key("Return")
        probe.wait(lambda state: not state["quick_terminal"]["visible"])
        gui.key("ctrl+grave")
        probe.wait(
            lambda state: (
                state["quick_terminal"]["visible"]
                and state["quick_terminal"]["pane"] not in (None, pane)
            )
        )
        assert len(gui.state()["tabs"]) == len(initial["tabs"])
    finally:
        probe.close()


PROGRAMS = r"""
local vtabs = assert(loadfile(os.getenv('WEZ_VTABS_PLUGIN')))()
local record = 'pwd > "$WEZ_VTABS_SCENARIO/program-cwd"; echo $$ > "$WEZ_VTABS_SCENARIO/program-pid";'
  .. ' echo "$PROGRAM_MARK" >> "$WEZ_VTABS_SCENARIO/program-starts"; exec cat -v > "$WEZ_VTABS_SCENARIO/program-input"'
table.insert(cfg.keys,{key='F5',action=vtabs.action{QuickTerminal={
  args={'/bin/sh','-c',record},set_environment_variables={PROGRAM_MARK='started'},width=0.5,height=0.6,
}}})
table.insert(cfg.keys,{key='F6',action=vtabs.action{QuickTerminal={args={'/bin/sh','-c','exec cat'}}}})
return cfg
"""


def quick_programs_probe(wezterm_binaries, headless_display, tmp_path, domain, local_pane_layout):
    probe = Probe(
        tmp_path / f"{domain}-{local_pane_layout}",
        wezterm_binaries["wezterm-gui"],
        wezterm_binaries["wez-vtabs-store"],
        domain,
        server=wezterm_binaries["wezterm-mux-server"],
        initial_size={"cols": 110, "rows": 40},
        display=headless_display,
    )
    config = probe.config.read_text().replace("return cfg", PROGRAMS)
    if local_pane_layout:
        config = config.replace(
            "no_serve_automatically=true}", "no_serve_automatically=true,local_pane_layout=true}"
        )
    probe.config.write_text(config)
    return probe, GuiInput(probe, headless_display, tmp_path)


def alive(pid):
    try:
        return Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1][0] != "Z"
    except FileNotFoundError:
        return False


@pytest.mark.gui
@pytest.mark.parametrize(
    ("domain", "local_pane_layout"), [("local", False), ("unix", True), ("unix", False)]
)
def test_quick_programs_start_fresh_in_the_active_pane_directory(
    wezterm_binaries, headless_display, tmp_path, domain, local_pane_layout
):
    # Without local_pane_layout, a unix socket on this machine runs the program locally.
    probe, gui = quick_programs_probe(
        wezterm_binaries, headless_display, tmp_path, domain, local_pane_layout
    )
    project = probe.root / "project dir"
    project.mkdir()

    def quick(state):
        return state["quick_terminal"]

    def program_started(count):
        starts = probe.root / "program-starts"
        return starts.exists() and starts.read_text().count("started") == count

    try:
        initial = probe.start()
        gui.attach()
        # OSC 7 reports the directory percent-encoded, as remote shells do.
        gui.text(
            f'cd "{project}" && '
            + r"""printf '\033]7;file://localhost%s\033\\' "$PWD" && """
            + 'touch "$WEZ_VTABS_SCENARIO/moved"'
        )
        gui.key("Return")
        probe.wait(lambda state: (probe.root / "moved").exists())

        gui.key("F5")
        opened = probe.wait(
            lambda state: quick(state)["visible"] and quick(state)["program"] is not None
        )
        program = quick(opened)["program"]
        assert quick(opened)["pane"] is None
        assert opened["tabs"] == initial["tabs"]
        width = opened["sidebar"]["width"] + opened["content"]["width"]
        assert quick(opened)["bounds"]["width"] == pytest.approx(width * 0.5)
        probe.wait(lambda state: program_started(1))
        assert (probe.root / "program-cwd").read_text().strip() == str(project)
        pid = int((probe.root / "program-pid").read_text())

        # Escape reaches the program instead of dismissing it.
        probe.wait(lambda state: (probe.root / "program-input").exists())
        gui.key("Escape")
        gui.key("Return")
        probe.wait(lambda state: "^[" in (probe.root / "program-input").read_text())
        assert quick(gui.state())["program"] == program

        # Losing focus keeps the program, unlike the shell.
        gui.command("windowminimize", gui.window)
        minimized = probe.sample_for(0.5)
        gui.command("windowactivate", "--sync", gui.window)
        assert all(
            quick(state)["program"] == program
            for state in minimized + probe.sample_for(0.3)
            if state["window"] == probe.window
        )

        # The same binding ends it; the next opening starts a new process.
        gui.key("F5")
        hidden = probe.wait(lambda state: not quick(state)["visible"])
        assert quick(hidden)["program"] is None
        probe.wait(lambda state: not alive(pid))
        gui.key("F5")
        probe.wait(lambda state: quick(state)["program"] not in (None, program))
        probe.wait(lambda state: program_started(2))
        pid = int((probe.root / "program-pid").read_text())

        # The quick terminal shortcut replaces the program with the persistent shell.
        gui.key("ctrl+grave")
        shell = probe.wait(
            lambda state: (
                quick(state)["visible"]
                and quick(state)["program"] is None
                and quick(state)["pane"] is not None
            )
        )
        assert quick(shell)["bounds"]["width"] == pytest.approx(width * 0.75)
        probe.wait(lambda state: not alive(pid))

        # Programs replace the shell and each other; the shell keeps running.
        gui.key("F6")
        other = quick(probe.wait(lambda state: quick(state)["program"] is not None))["program"]
        gui.key("F5")
        replaced = probe.wait(lambda state: quick(state)["program"] not in (None, other))
        assert quick(replaced)["pane"] == quick(shell)["pane"]
        probe.wait(lambda state: program_started(3))

        # Exiting the program hides the surface without touching tabs.
        probe.wait(lambda state: (probe.root / "program-input").exists())
        gui.key("ctrl+d")
        probe.wait(lambda state: not quick(state)["visible"] and quick(state)["program"] is None)
        assert gui.state()["tabs"] == initial["tabs"]
    finally:
        probe.close()
