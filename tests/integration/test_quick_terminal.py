"""Quick terminal shells and programs on owned displays."""

from pathlib import Path

import pytest

from tests.support.gui import GuiInput
from tests.support.wezterm import WeztermCli

PROGRAMS = r"""
local vtabs = assert(loadfile(os.getenv('WEZ_VTABS_PLUGIN')))()
local record = 'pwd > "$WEZ_VTABS_SCENARIO/program-cwd"; echo $$ > "$WEZ_VTABS_SCENARIO/program-pid";'
  .. ' echo "$PROGRAM_MARK" >> "$WEZ_VTABS_SCENARIO/program-starts"; exec cat -v > "$WEZ_VTABS_SCENARIO/program-input"'
table.insert(cfg.keys,{key='F5',action=vtabs.action{QuickTerminal={
  args={'/bin/sh','-c',record},set_environment_variables={PROGRAM_MARK='started'},width=0.5,height=0.6,
}}})
table.insert(cfg.keys,{key='F6',action=vtabs.action{QuickTerminal={args={'/bin/sh','-c','exec cat'}}}})
table.insert(cfg.keys,{key='F7',action=vtabs.action{QuickTerminal={
  args={'/bin/sh','-c','echo $$ >> "$WEZ_VTABS_SCENARIO/kept-pids"; exec cat'},preserve=true,
  window_padding={left=0,right=0,top=0,bottom=0},border_color='none',
}}})
"""


def quick_programs(probe_factory, tmp_path, domain, local_pane_layout):
    extra_lua = PROGRAMS
    if local_pane_layout:
        extra_lua += "cfg.unix_domains[1].local_pane_layout = true\n"
    probe = probe_factory(
        f"{domain}-{local_pane_layout}",
        domain,
        initial_size={"cols": 110, "rows": 40},
        extra_lua=extra_lua,
    )
    return probe, GuiInput(probe, tmp_path)


def quick(state):
    return state["quick_terminal"]


def alive(pid):
    try:
        return Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1][0] != "Z"
    except FileNotFoundError:
        return False


def change_directory(gui, directory):
    # OSC 7 reports the directory percent-encoded with the shell's host name.
    gui.text(
        f'cd "{directory}" && '
        + r"""printf '\033]7;file://%s%s\033\\' "$(uname -n)" "$PWD" && """
        + 'touch "$WEZ_VTABS_SCENARIO/moved"'
    )
    gui.key("Return")
    gui.probe.wait(lambda state: (gui.probe.root / "moved").exists())


@pytest.mark.gui
@pytest.mark.parametrize("domain", ["local", "unix"])
def test_quick_terminals_belong_to_their_space(probe_factory, tmp_path, domain):
    probe = probe_factory(domain, domain)
    gui = GuiInput(probe, tmp_path)
    initial = probe.start()
    gui.attach()
    home = initial["model"]["selected_space"]

    def open_shell():
        probe.intent("quick_terminal")
        return quick(probe.wait(lambda state: quick(state)["visible"] and quick(state)["pane"]))[
            "pane"
        ]

    def record(value, name):
        gui.text(f'{value}; echo $$ > "$WEZ_VTABS_SCENARIO/{name}-pid"')
        gui.key("Return")
        probe.wait(lambda state: (probe.root / f"{name}-pid").exists())
        return int((probe.root / f"{name}-pid").read_text())

    home_pane = open_shell()
    home_pid = record("QT_SPACE=home", "home")
    probe.intent({"CreateSpace": {"name": "Work"}})
    created = probe.wait(lambda state: state["model"]["selected_space"] != home)
    work = created["model"]["selected_space"]
    assert not quick(created)["visible"]
    assert quick(created)["pane"] is None
    assert created["tabs"] == initial["tabs"]

    work_pane = open_shell()
    work_pid = record("QT_SPACE=work", "work")
    assert work_pane != home_pane
    assert work_pid != home_pid
    for space, pane, value in [(home, home_pane, "home"), (work, work_pane, "work")]:
        probe.intent({"SelectSpace": space})
        selected = probe.wait(lambda state, space=space: state["model"]["selected_space"] == space)
        assert not quick(selected)["visible"]
        assert open_shell() == pane
        gui.text(f'printf "%s" "$QT_SPACE" > "$WEZ_VTABS_SCENARIO/{value}-value"')
        gui.key("Return")
        probe.wait(lambda state, value=value: (probe.root / f"{value}-value").exists())
        assert (probe.root / f"{value}-value").read_text() == value

    # Display names do not change the session's identity.
    probe.intent({"RenameSpace": {"id": work, "name": "Renamed"}})
    probe.wait(lambda state: any(s["name"] == "Renamed" for s in state["model"]["spaces"]))
    assert open_shell() == work_pane
    probe.intent({"DeleteSpace": {"id": work, "destination": home}})
    probe.wait(lambda state: state["model"]["selected_space"] == home and not alive(work_pid))
    assert alive(home_pid)
    assert open_shell() == home_pane


@pytest.mark.gui
@pytest.mark.parametrize("domain", ["local", "unix"])
def test_quick_terminal_preserves_its_shell_without_creating_a_tab(probe_factory, tmp_path, domain):
    probe = probe_factory(
        domain,
        domain,
        initial_size={"cols": 110, "rows": 40},
        extra_lua="cfg.exit_behavior = 'CloseOnCleanExit'",
    )
    gui = GuiInput(probe, tmp_path)
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
    gui.text('QT_SESSION=retained; (sleep 0.3; printf alive > "$WEZ_VTABS_SCENARIO/quick-alive") &')
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
    minimized = probe.sample_for(0.5)
    gui.command("windowactivate", "--sync", gui.window)
    assert all(
        state["quick_terminal"]["visible"] and state["quick_terminal"]["pane"] == pane
        for state in minimized + probe.sample_for(0.3)
        if state["window"] == probe.window
    )
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

    # WezTerm's default AdjustPaneSize keys step the shell in cells.
    gui.key("ctrl+shift+alt+Right")
    gui.key("ctrl+shift+alt+Up")
    stepped = probe.wait(
        lambda state: (
            state["quick_terminal"]["cols"] == resized["quick_terminal"]["cols"] + 1
            and state["quick_terminal"]["rows"] == resized["quick_terminal"]["rows"] - 1
        )
    )
    assert stepped["quick_terminal"]["pane"] == pane
    gui.text('stty size > "$WEZ_VTABS_SCENARIO/quick-stepped"')
    gui.key("Return")
    probe.wait(lambda state: (probe.root / "quick-stepped").exists())
    assert [int(n) for n in (probe.root / "quick-stepped").read_text().split()] == [
        stepped["quick_terminal"]["rows"],
        stepped["quick_terminal"]["cols"],
    ]

    # Escape reaches what runs in the shell, ahead of the keys typed after it.
    gui.text('cat -v > "$WEZ_VTABS_SCENARIO/quick-escape"')
    gui.key("Return")
    probe.wait(lambda state: (probe.root / "quick-escape").exists())
    gui.key("Escape")
    gui.text("x")
    gui.key("Return")
    probe.wait(lambda state: "^[x" in (probe.root / "quick-escape").read_text())
    assert gui.state()["quick_terminal"]["visible"]
    gui.key("ctrl+d")
    gui.text(': > "$WEZ_VTABS_SCENARIO/quick-idle"')
    gui.key("Return")
    probe.wait(lambda state: (probe.root / "quick-idle").exists())

    # At the idle shell, Escape dismisses the overlay and keeps its shell running.
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


@pytest.mark.gui
@pytest.mark.parametrize("domain", ["local", "unix"])
def test_quick_terminal_can_hide_on_focus_loss_and_start_fresh(probe_factory, tmp_path, domain):
    probe = probe_factory(domain, domain)
    gui = GuiInput(probe, tmp_path)

    def opened():
        probe.intent("quick_terminal")
        return probe.wait(
            lambda state: (
                state["quick_terminal"]["visible"] and state["quick_terminal"]["pane"] is not None
            )
        )["quick_terminal"]["pane"]

    initial = probe.start()
    gui.attach()
    probe.setting("quick_terminal_auto_hide", True)
    pane = opened()
    gui.command("windowminimize", gui.window)
    hidden = probe.wait(lambda state: not state["quick_terminal"]["visible"])
    assert hidden["quick_terminal"]["pane"] == pane
    gui.command("windowactivate", "--sync", gui.window)
    assert opened() == pane

    probe.setting("quick_terminal_preserve_shell", False)
    discarded = opened()
    assert discarded != pane
    gui.text("QT_SESSION=discarded")
    gui.key("Return")
    gui.command("windowminimize", gui.window)
    hidden = probe.wait(lambda state: not state["quick_terminal"]["visible"])
    assert hidden["quick_terminal"]["pane"] is None
    gui.command("windowactivate", "--sync", gui.window)
    fresh = opened()
    assert fresh != discarded
    gui.text('printf "%s" "${QT_SESSION-fresh}" > "$WEZ_VTABS_SCENARIO/quick-fresh"')
    gui.key("Return")
    probe.wait(lambda state: (probe.root / "quick-fresh").exists())
    assert (probe.root / "quick-fresh").read_text() == "fresh"
    gui.key("Escape")
    hidden = probe.wait(lambda state: not state["quick_terminal"]["visible"])
    assert hidden["quick_terminal"]["pane"] is None
    assert opened() != fresh
    assert gui.state()["tabs"] == initial["tabs"]


@pytest.mark.gui
@pytest.mark.parametrize("frontend", ["OpenGL", "WebGpu"])
def test_quick_terminal_background_opacity_and_blur_render_in_both_backends(
    probe_factory, tmp_path, frontend
):
    probe = probe_factory(
        frontend, extra_lua=f"cfg.front_end = '{frontend}'; cfg.colors = {{background='#ffffff'}}"
    )
    gui = GuiInput(probe, tmp_path)

    def center(name):
        if not gui.state()["quick_terminal"]["visible"]:
            probe.intent("quick_terminal")
            probe.wait(lambda state: state["quick_terminal"]["visible"])
        capture = gui.capture(f"{frontend}-{name}")
        frame = gui.state()["quick_terminal"]["bounds"]
        x, y = round(frame["x"] + frame["width"] / 2), round(frame["y"] + frame["height"] / 2)
        return gui.pixels(capture, x, y)

    probe.start()
    gui.attach()
    probe.setting("quick_terminal_background_color", "#123456")
    probe.setting("quick_terminal_border_color", "#ff0000")
    probe.intent("quick_terminal")
    probe.wait(lambda state: state["quick_terminal"]["pane"] is not None)
    opaque = center("opaque")
    assert len(opaque) == 3
    assert all(
        abs(actual - expected) <= 2 for actual, expected in zip(opaque, [18, 52, 86], strict=True)
    )
    probe.setting("quick_terminal_opacity", 0.5)
    translucent = center("translucent")
    assert all(after > before + 20 for before, after in zip(opaque, translucent, strict=True))
    probe.setting("quick_terminal_blur", 12)
    blurred = center("blurred")
    assert all(abs(a - b) <= 3 for a, b in zip(translucent, blurred, strict=True))
    probe.send("resize", {"width": 900, "height": 600})
    probe.wait(lambda state: state["dimensions"]["pixel_width"] == 900)
    assert all(value > 100 for value in center("resized"))
    probe.setting("quick_terminal_opacity", 1)
    assert all(abs(a - b) <= 2 for a, b in zip(opaque, center("opaque-again"), strict=True))


@pytest.mark.gui
@pytest.mark.parametrize(
    ("domain", "local_pane_layout"), [("local", False), ("unix", True), ("unix", False)]
)
def test_preserved_quick_programs_keep_running_with_their_own_padding(
    probe_factory, tmp_path, domain, local_pane_layout
):
    probe, gui = quick_programs(probe_factory, tmp_path, domain, local_pane_layout)
    pids = probe.root / "kept-pids"

    def started():
        return pids.read_text().split() if pids.exists() else []

    def inset(state):
        terminal = quick(state)
        cell = terminal["content"]["width"] / terminal["cols"]
        return (terminal["content"]["x"] - terminal["bounds"]["x"]) / cell

    probe.start()
    gui.attach()
    probe.setting("quick_terminal_window_padding", {"left": "2cell", "right": "2cell"})
    gui.key("F7")
    opened = probe.wait(
        lambda state: quick(state)["visible"] and quick(state)["program"] is not None
    )
    program = quick(opened)["program"]
    probe.wait(lambda state: len(started()) == 1)
    assert inset(opened) < 1

    # Hiding keeps the process for the next opening.
    gui.key("F7")
    hidden = probe.wait(lambda state: not quick(state)["visible"])
    assert quick(hidden)["program"] is None
    assert quick(hidden)["kept"] == [program]
    assert alive(int(started()[0]))
    gui.key("F7")
    probe.wait(lambda state: quick(state)["visible"] and quick(state)["program"] == program)

    # Another program and the shell take over without ending it; the shell keeps its padding.
    gui.key("F6")
    other = probe.wait(lambda state: quick(state)["program"] not in (None, program))
    assert quick(other)["kept"] == [program]
    gui.key("ctrl+grave")
    shell = probe.wait(
        lambda state: quick(state)["pane"] is not None and quick(state)["program"] is None
    )
    assert inset(shell) >= 2
    gui.key("F7")
    probe.wait(lambda state: quick(state)["program"] == program)
    assert len(started()) == 1

    # Once the program exits, the next opening starts it again.
    gui.key("ctrl+d")
    ended = probe.wait(lambda state: not quick(state)["visible"])
    assert not quick(ended)["kept"]
    gui.key("F7")
    probe.wait(lambda state: quick(state)["program"] not in (None, program))
    probe.wait(lambda state: len(started()) == 2)


@pytest.mark.gui
@pytest.mark.parametrize(
    ("domain", "local_pane_layout"), [("local", False), ("unix", True), ("unix", False)]
)
def test_quick_programs_start_fresh_in_the_active_pane_directory(
    probe_factory, tmp_path, domain, local_pane_layout
):
    # Without local_pane_layout, a unix socket on this machine runs the program locally.
    probe, gui = quick_programs(probe_factory, tmp_path, domain, local_pane_layout)
    project = probe.root / "project dir"
    project.mkdir()

    def program_started(count):
        starts = probe.root / "program-starts"
        return starts.exists() and starts.read_text().count("started") == count

    initial = probe.start()
    gui.attach()
    change_directory(gui, project)

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

    # Losing focus keeps the program.
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

    # The size settings apply to the shell and to programs that set none.
    probe.intent({"SetSetting": {"key": "quick_terminal_width", "value": 0.6}})
    probe.wait(
        lambda state: (
            not quick(state)["visible"]
            and state["model"]["settings"]["quick_terminal_width"] == 0.6
        )
    )
    gui.key("ctrl+grave")
    sized = probe.wait(lambda state: quick(state)["visible"])
    # The frame snaps to whole pixels.
    assert quick(sized)["bounds"]["width"] == pytest.approx(width * 0.6, abs=1)

    # Programs replace the shell and each other; the shell keeps running.
    gui.key("F6")
    opened = probe.wait(lambda state: quick(state)["program"] is not None)
    other = quick(opened)["program"]
    assert quick(opened)["bounds"]["width"] == pytest.approx(width * 0.6, abs=1)
    gui.key("F5")
    replaced = probe.wait(lambda state: quick(state)["program"] not in (None, other))
    assert quick(replaced)["pane"] == quick(shell)["pane"]
    probe.wait(lambda state: program_started(3))

    # Exiting the program hides the surface without touching tabs.
    probe.wait(lambda state: (probe.root / "program-input").exists())
    gui.key("ctrl+d")
    probe.wait(lambda state: not quick(state)["visible"] and quick(state)["program"] is None)
    assert gui.state()["tabs"] == initial["tabs"]


@pytest.mark.gui
def test_localmux_quick_terminal_shell_runs_on_the_server_and_follows_replacement(
    wezterm_binaries, probe_factory, tmp_path
):
    probe = probe_factory(
        "unix",
        "unix",
        initial_size={"cols": 110, "rows": 40},
        extra_lua=r"""
wezterm.on('user-var-changed', function(window, pane, name, value)
  if name == 'QUICK_PROBE' then
    local f = assert(io.open(root..'/quick-user-var', 'w')); f:write(value); f:close()
  end
end)
""",
    )
    gui = GuiInput(probe, tmp_path)
    cli = WeztermCli(wezterm_binaries["wezterm"], probe.config, probe.root / "mux.sock", probe.env)
    project = probe.root / "project dir"
    project.mkdir()

    def quick_shells():
        return [
            pane["pane_id"]
            for pane in cli.panes()
            if pane["workspace"].startswith("__quick:") and pane["workspace"].endswith(":shell")
        ]

    def run(command, marker):
        gui.text(command)
        gui.key("Return")
        probe.wait(lambda state: (probe.root / marker).exists())
        return (probe.root / marker).read_text()

    initial = probe.start()
    gui.attach()
    change_directory(gui, project)

    gui.key("ctrl+grave")
    opened = probe.wait(lambda state: quick(state)["visible"] and quick(state)["pane"] is not None)
    pane = quick(opened)["pane"]
    assert run('pwd > "$WEZ_VTABS_SCENARIO/quick-pwd"', "quick-pwd").strip() == str(project)
    # The shell lives on the server, in a workspace the GUI never shows.
    [server_pane] = quick_shells()
    assert gui.state()["tabs"] == initial["tabs"]

    gui.text("""printf '\\033]1337;SetUserVar=QUICK_PROBE=%s\\007' "$(printf hi | base64)\"""")
    gui.key("Return")
    probe.wait(lambda state: (probe.root / "quick-user-var").exists())
    assert (probe.root / "quick-user-var").read_text() == "hi"

    # ATTACH_MUX replaces the server pane; the overlay follows it.
    cli(
        "replace-pane",
        "--pane-id",
        server_pane,
        "--",
        "/bin/sh",
        "-c",
        'touch "$WEZ_VTABS_SCENARIO/replaced"; exec /bin/sh',
    )
    probe.wait(lambda state: (probe.root / "replaced").exists())
    followed = probe.wait(lambda state: quick(state)["pane"] not in (None, pane))
    assert quick(followed)["visible"]
    assert quick_shells() not in ([], [server_pane])
    run('touch "$WEZ_VTABS_SCENARIO/typed"', "typed")
    assert gui.state()["tabs"] == initial["tabs"]
