#!/usr/bin/env python3
"""Real native tab moves, remote pane ownership, resize, and reopen regression."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import time

from scenarios import Probe


def run(probe, cli, external):
    state = probe.start()
    source = probe.window
    moved = state["tabs"][0]["id"]
    workspace = state["workspace"]
    remote_env = dict(probe.env, WEZTERM_UNIX_SOCKET=str(probe.root / "mux.sock"))

    def remote_command(*arguments):
        return subprocess.check_output(
            [str(cli), "--config-file", str(probe.config), "cli", *arguments],
            env=remote_env, text=True, timeout=5,
        )

    def remote_list():
        return json.loads(remote_command("list", "--format", "json"))

    def remote_wait(predicate, message):
        deadline = time.monotonic() + 5
        while True:
            panes = remote_list()
            if predicate(panes):
                return
            if time.monotonic() > deadline:
                raise AssertionError(f"{message}: {panes!r}")
            time.sleep(0.05)

    remote_pane = None
    if probe.domain == "unix":
        remote_pane = next(p["pane_id"] for p in remote_list() if p["workspace"] == workspace)
    else:
        probe.action("split")
        state = probe.wait(lambda s: len(s.get("tabs", [{}])[0].get("panes", [])) == 2)
    pane_ids = [p["id"] for p in state["tabs"][0]["panes"]]
    probe.intent({"PinTab": {"id": moved, "pinned": True}})
    probe.wait(lambda s: any(t["id"] == moved and t["pinned"] for t in s.get("model", {}).get("tabs", [])))
    probe.action("new_tab")
    probe.wait(lambda s: len(s.get("tabs", [])) == 2)

    if external:
        # A second client exercises authoritative resync without a GUI move callback.
        remote_command("move-pane-to-new-tab", "--pane-id", str(remote_pane),
                       "--new-window", "--workspace", workspace)
    else:
        probe.intent({"MoveTabToNewWindow": moved})
    destination = probe.wait(
        lambda s: s.get("window") != source and len(s.get("tabs", [])) == 1
        and [p["id"] for p in s["tabs"][0]["panes"]] == pane_ids,
        any_window=True,
    )
    destination_id = destination["window"]
    if not external:
        assert any(t["pinned"] for t in destination["model"]["tabs"]), "move lost pin metadata"
    assert destination["workspace"] == workspace, "move changed native workspace"

    size, dimensions = destination["tabs"][0]["size"], destination["dimensions"]
    cols, rows = size["cols"] + 4, size["rows"] + 2
    command = probe.send("resize", {
        "width": dimensions["pixel_width"] + 4 * size["pixel_width"] // size["cols"],
        "height": dimensions["pixel_height"] + 2 * size["pixel_height"] // size["rows"],
    }, window=destination_id)
    probe.wait(
        lambda s: s.get("window") == destination_id and s.get("command", 0) >= command
        and s["tabs"][0]["size"]["cols"] == cols and s["tabs"][0]["size"]["rows"] == rows,
        any_window=True,
    )
    if remote_pane is not None:
        # Optimistic local dimensions alone cannot prove the containing-tab ID
        # was refreshed: verify that the remote resize RPC reached its new tab.
        remote_wait(
            lambda panes: any(p["pane_id"] == remote_pane and p["size"]["cols"] == cols
                              and p["size"]["rows"] == rows for p in panes),
            "destination resize did not reach remote mux",
        )

    state = probe.wait(lambda s: len(s.get("tabs", [])) == 1 and s.get("model", {}).get("can_reopen") is False)
    assert all(t["id"] != moved for t in state["tabs"]), "moved tab remained in source"
    probe.action("new_tab")
    probe.wait(lambda s: len(s.get("tabs", [])) == 2)
    probe.action("close")
    probe.wait(lambda s: len(s.get("tabs", [])) == 1 and s.get("model", {}).get("can_reopen") is True)
    probe.intent("Reopen")
    probe.wait(lambda s: len(s.get("tabs", [])) == 2 and s.get("model", {}).get("can_reopen") is False)
    sequence = probe.latest[destination_id]["sequence"]
    probe.send("close_window")
    probe.sample_for(0.3)
    probe.wait(
        lambda s: s.get("window") == destination_id and s.get("sequence", 0) > sequence
        and len(s.get("tabs", [])) == 1
        and [p["id"] for p in s["tabs"][0]["panes"]] == pane_ids,
        any_window=True,
    )
    if remote_pane is not None:
        remote_wait(
            lambda panes: [p["pane_id"] for p in panes if p["workspace"] == workspace] == [remote_pane],
            "closing source did not leave exactly the moved destination pane alive",
        )
    return {
        "source_window": source, "destination_window": destination_id,
        "preserved_pane_ids": pane_ids, "workspace_preserved": True,
        # CLI pane moves allocate a new native tab without plugin transfer context.
        "pin_preserved": None if external else True, "pin_transfer_checked": not external,
        "move_did_not_enter_history": True, "native_close_and_reopen": True,
        "destination_resize": [cols, rows], "source_close_preserved_destination": True,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gui", type=Path, required=True)
    parser.add_argument("--helper", type=Path, help="defaults to the GUI's sibling wez-vtabs-store")
    parser.add_argument("--mux-server", type=Path, help="defaults to the GUI's sibling mux server")
    parser.add_argument("--domain", choices=("local", "unix"), default="local")
    parser.add_argument("--external", action="store_true", help="move through a second Unix mux CLI client")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.external and args.domain != "unix":
        parser.error("--external requires --domain unix")
    suffix = ".exe" if os.name == "nt" else ""
    gui = args.gui.resolve()
    helper = (args.helper or gui.with_name("wez-vtabs-store" + suffix)).resolve()
    server = (args.mux_server or gui.with_name("wezterm-mux-server" + suffix)).resolve()
    output = args.output.resolve()
    probe = Probe(output / "native", gui, helper, args.domain, server=server, chrome=True)
    report = {"domain": args.domain, "external": args.external}
    try:
        report.update(run(probe, gui.with_name("wezterm" + suffix), args.external))
    except BaseException as error:
        report["error"] = str(error)
        raise
    finally:
        probe.close()
        for name in ("errors.log", "gui.log", "mux.log"):
            log = probe.root / name
            report[name] = log.stat().st_size if log.exists() else 0
        (output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(output)


if __name__ == "__main__":
    main()
