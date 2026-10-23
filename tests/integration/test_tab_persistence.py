"""GUI restart restores placement for shells that remain in the mux server."""

import pytest

from tests.support.wezterm import WeztermCli


@pytest.mark.gui
def test_surviving_mux_tabs_restore_spaces_pins_and_folders(wezterm_binaries, probe_factory):
    probe = probe_factory("restart", "unix", extra_lua="cfg.default_workspace = identity")
    probe.config.write_text(
        probe.config.read_text().replace(
            "  plugin.apply_to_config(cfg,options)",
            "  options.spaces={{id='home',name='Home'},{id='work',name='Work'}}\n"
            "  plugin.apply_to_config(cfg,options)",
        )
    )
    cli = WeztermCli(wezterm_binaries["wezterm"], probe.config, probe.root / "mux.sock", probe.env)
    initial = probe.start()
    workspace = initial["workspace"]
    tab = initial["tabs"][0]["id"]

    probe.intent({"AssignTab": {"id": tab, "space_id": "work"}})
    probe.wait(lambda state: state["model"]["tabs"][0]["space_id"] == "work")
    probe.intent({"SelectSpace": "work"})
    probe.wait(lambda state: state["model"]["selected_space"] == "work")
    probe.intent({"CreateFolder": {"name": "Tools"}})
    created = probe.wait(lambda state: bool(state["model"]["folders"]))
    folder = created["model"]["folders"][0]["id"]
    probe.intent({"AssignFolder": {"tab_id": tab, "folder_id": folder}})
    probe.wait(lambda state: state["model"]["tabs"][0]["folder_id"] == folder)

    def server_panes():
        return {pane["pane_id"] for pane in cli.panes() if pane["workspace"] == workspace}

    surviving = server_panes()
    assert surviving
    probe.action("quit")
    probe.gui_process.wait(timeout=10)
    assert probe.gui_process.returncode == 0
    assert probe.processes[0].poll() is None
    assert server_panes() == surviving

    probe.read()
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
        "restart.log",
    )
    restored = probe.wait(
        lambda state: (
            state.get("workspace") == workspace
            and len(state.get("model", {}).get("tabs", [])) == 1
            and state["model"]["tabs"][0]["folder_id"] == folder
        ),
        timeout=25,
        any_window=True,
    )
    probe.window = restored["window"]
    placement = restored["model"]["tabs"][0]
    assert placement["space_id"] == "work"
    assert placement["pinned"]
    assert restored["model"]["selected_space"] == "work"
    assert server_panes() == surviving
