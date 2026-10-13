#!/usr/bin/env python3
"""scenarios on an owned headless display and isolated application state."""

from __future__ import annotations

import argparse
import json
import sys
import tempfile
import time
from pathlib import Path

if __package__ in {None, ""}:
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from tests.support.probe import Probe, pane_shape


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
    sizes = [
        {
            "width": dimensions["pixel_width"] + delta,
            "height": dimensions["pixel_height"],
        }
        for delta in deltas
    ] * probe.resize_rounds
    start = len(probe.samples)
    command = probe.send("resize_sequence", sizes)
    probe.wait(
        lambda state: state.get("command", 0) >= command and state.get("step") == len(sizes),
        timeout=max(10, len(sizes) * 0.06 + 5),
    )
    probe.sample_for(0.35)
    frames = [
        state
        for state in probe.samples[start:]
        if state["window"] == probe.window and state.get("command", 0) >= command
    ]
    assert len({state["dimensions"]["pixel_width"] for state in frames}) >= 5, (
        "insufficient resize samples"
    )
    for state in frames:
        tab = next(tab for tab in state["tabs"] if tab["id"] == first)
        assert [pane["id"] for pane in tab["panes"]] == pane_ids, (
            "resize changed content pane identities"
        )
        assert all(pane["top"] == 0 for pane in tab["panes"]), (
            "resize rearranged horizontal content splits"
        )
        if probe.vtabs:
            expected_width = state["model"]["settings"]["width"] * state["dimensions"]["dpi"] / 96
            assert abs(state["sidebar"]["width"] - expected_width) <= 1, (
                "transient sidebar width correction"
            )
            assert state["content"]["x"] >= state["sidebar"]["x"] + state["sidebar"]["width"] - 1, (
                "content overlaps sidebar"
            )
            sizes_now = {(tab["size"]["cols"], tab["size"]["rows"]) for tab in state["tabs"]}
            assert len(sizes_now) == 1, "background tab has stale content dimensions"
    before = probe.latest[probe.window]
    for index, expected in ((0, first), (1, second), (-1, second), (0, first)):
        requested_at = time.monotonic()
        command = probe.action("activate", index=index)
        selected = probe.wait(
            lambda state, command=command, expected=expected: (
                state.get("command", 0) >= command
                and next((tab["id"] for tab in state.get("tabs", []) if tab["active"]), None)
                == expected
            )
        )
        if probe.vtabs:
            assert selected["active"] == expected, "vtabs binding and visible projection disagree"
            assert selected["sidebar"] == before["sidebar"], (
                "vtabs activation changed sidebar geometry"
            )
        selected["activation_observed_seconds"] = selected["observed_at"] - requested_at
    probe.sample_for(0.4)
    final = probe.latest[probe.window]
    observed_steps = {}
    for state in frames:
        if (
            state.get("step", 0)
            and state.get("requested")
            and abs(state["dimensions"]["pixel_width"] - state["requested"]["width"]) <= 1
        ):
            observed_steps[state["step"]] = pane_shape(state)
    return {
        "initial_size": split["tabs"][0]["size"],
        "initial_split": original_shape,
        "final_split": pane_shape(final)[0],
        "resize_steps": observed_steps,
        "frames": len(frames),
    }


def wait_for_managed(probe, path, text, failure):
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if path.exists() and text in path.read_text(encoding="utf-8"):
            return
        probe.sample_for(0.05)
    raise AssertionError(failure)


def feature_scenarios(probe):
    state = probe.latest[probe.window]
    home = state["model"]["selected_space"]
    old_tabs = {tab["id"] for tab in state["tabs"]}
    probe.intent({"CreateSpace": {"name": "Scenario Work"}})
    empty = probe.wait(lambda state: state.get("model", {}).get("selected_space") != home)
    space = empty["model"]["selected_space"]
    assert empty["visible"] == [], "empty space exposes hidden terminal"
    assert empty["active"] is None, "empty space keeps a hidden terminal active"
    assert {tab["id"] for tab in empty["tabs"]} == old_tabs, (
        "selecting space spawned or moved content"
    )
    assert any(item["name"] == "Scenario Work" for item in empty["model"]["spaces"])
    probe.capture("empty-space")
    probe.action("new_tab")
    created = probe.wait(
        lambda state: (
            len(state.get("tabs", [])) == len(old_tabs) + 1 and len(state.get("visible", [])) == 1
        )
    )
    assert created["active"] == created["visible"][0]
    assert created["model"]["selected_space"] == space
    probe.intent({"SelectSpace": home})
    probe.wait(
        lambda state: (
            state.get("model", {}).get("selected_space") == home
            and len(state.get("visible", [])) == len(old_tabs)
        )
    )
    probe.sample_for(0.6)
    idle = [state for state in probe.sample_for(0.3) if state["window"] == probe.window]
    assert idle, "idle sidebar produced no samples"
    assert len({state["model"]["surface_revision"] for state in idle}) == 1, (
        "idle sidebar recomposes"
    )
    if probe.env["WEZ_VTABS_SCENARIO_HOOKS"] == "1":
        assert all(value > 0 for value in idle[0]["hook_calls"].values()), (
            "configured hook was not exercised"
        )
        assert idle[0]["hook_calls"] == idle[-1]["hook_calls"], "idle sidebar reruns Lua hooks"
    probe.intent({"SetSetting": {"key": "width", "value": 300}})
    probe.wait(lambda state: state.get("model", {}).get("settings", {}).get("width") == 300)
    managed = probe.root / "vtabs_settings.lua"
    wait_for_managed(probe, managed, "width = 300", "settings write did not reach the Lua file")
    return {
        "empty_space": space,
        "idle_samples": len(idle),
    }


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
        probe.wait(
            lambda state, command=command, tab=tab: (
                state.get("command", 0) >= command and tab in state.get("visible", [])
            )
        )
    command = probe.intent({"PinTab": {"id": tabs[0], "pinned": True}})
    probe.wait(lambda state: state.get("command", 0) >= command)
    probe.action("activate", index=0)
    probe.wait(lambda state: state.get("active") == tabs[0])
    switched_at = time.monotonic()
    probe.action("workspace", workspace=probe.identity + "-other")
    other = probe.wait(
        lambda state: (
            state.get("mux_window") != original_mux
            and state.get("workspace") == probe.identity + "-other"
            and state.get("observed_at", 0) > switched_at
        ),
        any_window=True,
    )
    probe.window = other["window"]
    switched_at = time.monotonic()
    probe.action("workspace", workspace=probe.identity)
    restored = probe.wait(
        lambda state: (
            state.get("mux_window") == original_mux
            and state.get("workspace") == probe.identity
            and state.get("observed_at", 0) > switched_at
        ),
        any_window=True,
    )
    probe.window = restored["window"]
    assert restored["model"]["selected_space"] == space, "workspace switch lost selected space"
    assert set(restored["visible"]) == set(tabs), "workspace switch lost tab membership"
    assert any(
        tab["id"] == tabs[0] and tab["pinned"] for tab in restored["model"].get("tabs", [])
    ), "workspace switch lost pin"
    for tab in tabs:
        command = probe.intent({"AssignTab": {"id": tab, "space_id": original_space}})
        probe.wait(lambda state, command=command: state.get("command", 0) >= command)
    command = probe.intent({"PinTab": {"id": tabs[0], "pinned": False}})
    probe.wait(lambda state: state.get("command", 0) >= command)
    probe.intent({"SelectSpace": original_space})
    probe.wait(
        lambda state: (
            state.get("model", {}).get("selected_space") == original_space
            and len(state.get("visible", [])) == len(tabs)
        )
    )
    return {
        "original_window": original_window,
        "other_window": other["window"],
        "original_mux": original_mux,
        "other_mux": other["mux_window"],
        "retained_space": space,
    }


def edge_scenarios(probe):
    initial = probe.latest[probe.window]
    initial_size = initial["dimensions"]
    pane_ids = [[pane["id"] for pane in tab["panes"]] for tab in initial["tabs"]]
    initial_shape = pane_shape(initial)
    cell_width = initial["tabs"][0]["size"]["pixel_width"] / initial["tabs"][0]["size"]["cols"]
    probe.action("font_increase")
    probe.wait(
        lambda state: (
            state.get("tabs")
            and state["tabs"][0]["size"]["pixel_width"] / state["tabs"][0]["size"]["cols"]
            > cell_width
        )
    )
    probe.action("font_reset")
    probe.wait(
        lambda state: (
            state.get("tabs")
            and state["tabs"][0]["size"]["pixel_width"] / state["tabs"][0]["size"]["cols"]
            == cell_width
        )
    )
    for enabled in (True, False):
        probe.action("fullscreen")
        state = probe.wait(
            lambda state, enabled=enabled: (
                state.get("dimensions", {}).get("is_full_screen") is enabled
            )
        )
        assert state["content"]["width"] > 0, "fullscreen content width is invalid"
        assert state["content"]["height"] > 0, "fullscreen content height is invalid"
        expected = state["model"]["settings"]["width"] * state["dimensions"]["dpi"] / 96
        assert abs(state["sidebar"]["width"] - expected) <= 1, (
            "fullscreen changed logical sidebar width"
        )
    for enabled in (True, False):
        probe.action("zoom")
        probe.wait(
            lambda state, enabled=enabled: any(
                tab["active"] and tab["zoomed"] is enabled for tab in state.get("tabs", [])
            )
        )
    assert pane_shape(probe.latest[probe.window]) == initial_shape, (
        "font/fullscreen/zoom roundtrip changed split sizing"
    )
    probe.intent({"SetSetting": {"key": "side", "value": "right"}})
    right = probe.wait(
        lambda state: (
            state.get("model", {}).get("settings", {}).get("side") == "right"
            and state.get("sidebar", {}).get("x", 0) > 0
        )
    )
    assert abs(right["sidebar"]["x"] - right["content"]["x"] - right["content"]["width"]) <= 1, (
        "right sidebar overlaps terminal"
    )
    if probe.env["WEZ_VTABS_SCENARIO_CHROME"] == "1":
        assert right["sidebar"]["y"] > 0, "integrated chrome not reserved"
        assert right["sidebar"]["y"] == right["content"]["y"], "content and sidebar do not align"
    for rail, expected in (
        ("hidden", None),
        ("expanded", "width"),
    ):
        command = probe.intent({"SetSetting": {"key": "rail", "value": rail}})
        state = probe.wait(
            lambda state, command=command, rail=rail: (
                state.get("command", 0) >= command
                and state.get("model", {}).get("settings", {}).get("rail") == rail
            )
        )
        width = (
            state["model"]["settings"][expected] * state["dimensions"]["dpi"] / 96
            if expected
            else 0
        )
        assert abs(state["sidebar"]["width"] - width) <= 1, "rail reservation is stale"
    command = probe.send("resize", {"width": 150, "height": 120})
    tiny = probe.wait(
        lambda state: (
            state.get("command", 0) >= command
            and state.get("dimensions", {}).get("pixel_width", 9999) < initial_size["pixel_width"]
        )
    )
    probe.sample_for(0.25)
    tiny = probe.latest[probe.window]
    assert tiny["content"]["width"] > 0, "tiny resize removes content width reservation"
    assert tiny["content"]["height"] >= 0, "tiny resize removes content height reservation"
    assert tiny["sidebar"]["width"] >= 0, "tiny sidebar has a negative width"
    assert (
        tiny["sidebar"]["x"] + tiny["sidebar"]["width"] <= tiny["dimensions"]["pixel_width"] + 1
    ), "tiny sidebar extends outside window"
    assert [[pane["id"] for pane in tab["panes"]] for tab in tiny["tabs"]] == pane_ids, (
        "tiny resize changed pane topology"
    )
    command = probe.intent({"SetSetting": {"key": "side", "value": "left"}})
    probe.wait(
        lambda state: (
            state.get("command", 0) >= command
            and state.get("model", {}).get("settings", {}).get("side") == "left"
        )
    )
    command = probe.send(
        "resize",
        {"width": initial_size["pixel_width"], "height": initial_size["pixel_height"]},
    )
    restored = probe.wait(
        lambda state: (
            state.get("command", 0) >= command
            and state.get("dimensions", {}).get("pixel_width") == initial_size["pixel_width"]
            and state.get("dimensions", {}).get("pixel_height") == initial_size["pixel_height"]
        )
    )
    assert [[pane["id"] for pane in tab["panes"]] for tab in restored["tabs"]] == pane_ids, (
        "restoring tiny window changed pane identities"
    )
    return {
        "right_x": right["sidebar"]["x"],
        "chrome_height": right["sidebar"]["y"],
        "tiny_size": tiny["dimensions"],
        "tiny_content": tiny["content"],
        "tiny_terminal": tiny["tabs"][0]["size"],
        "restored_split": pane_shape(restored),
        "font_fullscreen_zoom": True,
    }


def stock_tiny_scenario(probe, vtabs_edges):
    initial = probe.latest[probe.window]
    dimensions = initial["dimensions"]
    terminal = initial["tabs"][0]["size"]
    target = vtabs_edges["tiny_terminal"]
    command = probe.send(
        "resize",
        {
            "width": dimensions["pixel_width"] - terminal["pixel_width"] + target["pixel_width"],
            "height": dimensions["pixel_height"]
            - terminal["pixel_height"]
            + target["pixel_height"],
        },
    )
    probe.wait(
        lambda state: (
            state.get("command", 0) >= command
            and all(
                tab["size"]["cols"] == target["cols"] and tab["size"]["rows"] == target["rows"]
                for tab in state.get("tabs", [])
            )
        )
    )
    command = probe.send(
        "resize",
        {"width": dimensions["pixel_width"], "height": dimensions["pixel_height"]},
    )
    restored = probe.wait(
        lambda state: (
            state.get("command", 0) >= command
            and all(
                tab["size"]["cols"] == terminal["cols"] and tab["size"]["rows"] == terminal["rows"]
                for tab in state.get("tabs", [])
            )
        )
    )
    shape = pane_shape(restored)
    assert shape == vtabs_edges["restored_split"], "tiny-window split rounding differs from stock"
    return shape


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gui", type=Path, required=True)
    parser.add_argument("--helper", type=Path)
    parser.add_argument("--mux-server", type=Path)
    parser.add_argument("--baseline-gui", type=Path)
    parser.add_argument("--geometry-only", action="store_true")
    parser.add_argument(
        "--workspace",
        action="store_true",
        help="also check suspended vtabs workspace state",
    )
    parser.add_argument(
        "--chrome",
        action="store_true",
        help="use integrated title controls and nonzero padding",
    )
    parser.add_argument(
        "--edge-cases",
        action="store_true",
        help="check fonts, fullscreen, zoom, right edge, rail states and tiny windows",
    )
    parser.add_argument(
        "--trace-mux", action="store_true", help="record client/server resize traces"
    )
    parser.add_argument(
        "--hooks",
        action="store_true",
        help="enable all Lua hooks and process routing metadata",
    )
    parser.add_argument(
        "--effects", action="store_true", help="retain default TachyonFX transitions"
    )
    parser.add_argument(
        "--resize-rounds",
        type=int,
        default=1,
        help="repeat the dense resize/reversal sequence in one window",
    )
    parser.add_argument(
        "--capture",
        action="store_true",
        help="capture this runner's private X11 window",
    )
    parser.add_argument("--domain", choices=("local", "unix", "ssh", "tls"), default="local")
    parser.add_argument(
        "--ssh-config",
        type=Path,
        help="SshDomain JSON for a disposable SSH-mux fixture",
    )
    parser.add_argument(
        "--tls-config",
        type=Path,
        help="TlsDomainClient JSON for a disposable TLS-mux fixture",
    )
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if not 1 <= args.resize_rounds <= 20:
        parser.error("--resize-rounds must be between 1 and 20")
    if args.domain == "ssh" and not args.ssh_config:
        parser.error("--domain ssh requires --ssh-config for a disposable test mux")
    if args.domain == "tls" and not args.tls_config:
        parser.error("--domain tls requires --tls-config for a disposable test mux")
    output = (args.output or Path(tempfile.mkdtemp(prefix="vtabs-scenarios-"))).resolve()
    output.mkdir(parents=True, exist_ok=True)
    report = {
        "domain": args.domain,
        "boundary": "GUI state samples and vtabs actions; OS drag smoothness, GPU presentation and physical key latency require visual/profiling checks",
    }
    probe = Probe(
        output / "gui",
        args.gui.resolve(),
        args.helper.resolve() if args.helper else None,
        args.domain,
        args.ssh_config,
        capture=args.capture,
        server=args.mux_server,
        chrome=args.chrome,
        trace_mux=args.trace_mux,
        hooks=args.hooks,
        effects=args.effects,
        resize_rounds=args.resize_rounds,
        tls_config=args.tls_config,
    )
    try:
        report["geometry"] = geometry_scenarios(probe)
        if args.workspace:
            report["workspace"] = workspace_scenario(probe)
        if args.edge_cases:
            report["edges"] = edge_scenarios(probe)
        if not args.geometry_only:
            report["features"] = feature_scenarios(probe)
        report["vtabs_samples"] = len(probe.samples)
    except BaseException as error:
        report["error"] = str(error)
        raise
    finally:
        probe.close()
        (output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(output)
    if args.baseline_gui:
        baseline = Probe(
            output / "stock",
            args.baseline_gui.resolve(),
            None,
            args.domain,
            args.ssh_config,
            vtabs=False,
            capture=args.capture,
            server=args.mux_server,
            chrome=args.chrome,
            initial_size=report["geometry"]["initial_size"],
            trace_mux=args.trace_mux,
            resize_rounds=args.resize_rounds,
            tls_config=args.tls_config,
        )
        try:
            report["stock_geometry"] = geometry_scenarios(baseline)
            common = set(report["geometry"]["resize_steps"]) & set(
                report["stock_geometry"]["resize_steps"]
            )
            assert len(common) >= 5, "insufficient matching vtabs/stock resize samples"
            for step in common:
                assert (
                    report["geometry"]["resize_steps"][step]
                    == report["stock_geometry"]["resize_steps"][step]
                ), f"split distribution differs from stock at resize step {step}"
            if "edges" in report:
                report["stock_tiny_restored"] = stock_tiny_scenario(baseline, report["edges"])
        finally:
            baseline.close()
            (output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    print("vtabs scenarios passed")


if __name__ == "__main__":
    main()
