"""Keyboard, pointer and screenshot control of an owned headless GUI window."""

from __future__ import annotations

import shutil
import subprocess
import time
from pathlib import Path

from tests.support.headless import HeadlessDisplay
from tests.support.polling import check_process, log_diagnostics, wait_until
from tests.support.probe import Probe


class GuiInput:
    def __init__(self, probe: Probe, display: HeadlessDisplay, output: Path):
        self.probe, self.display, self.output = probe, display, output
        self.window = None
        self.captures = []

    def command(self, *args):
        self.display.assert_live()
        return subprocess.run(
            ["xdotool", *map(str, args)],
            env=self.display.env,
            check=True,
            text=True,
            capture_output=True,
            timeout=10,
        ).stdout.strip()

    def attach(self):
        def ready():
            result = subprocess.run(
                ["xdotool", "search", "--onlyvisible", "--class", self.probe.identity],
                env=self.display.env,
                text=True,
                capture_output=True,
                timeout=3,
            )
            if result.returncode == 0 and result.stdout.strip():
                windows = result.stdout.splitlines()
                if len(windows) != 1:
                    raise AssertionError(f"Expected one owned GUI window, found {windows}")
                return windows[0]
            return None

        self.window = wait_until(
            ready,
            timeout=10,
            message="Owned GUI window was not mapped on the private display",
            check_live=lambda: check_process(self.probe.gui_process, "GUI"),
            diagnostics=lambda: log_diagnostics(self.probe.root / "gui.log"),
        )
        self.command("windowactivate", "--sync", self.window)

    def key(self, chord):
        self.command("key", "--clearmodifiers", chord)

    def text(self, text):
        self.command("type", "--clearmodifiers", "--delay", "18", "--", text)

    def state(self):
        self.probe.read()
        return self.probe.latest[self.probe.window]

    @staticmethod
    def hit(state, identifier):
        return next(
            (hit for hit in state.get("model", {}).get("hits", []) if hit["id"] == identifier),
            None,
        )

    def hit_position(self, identifier, y_fraction=0.5):
        state = self.probe.wait(lambda state: self.hit(state, identifier) is not None)
        hit = self.hit(state, identifier)
        grid = state["model"]["grid"]
        return (
            round(grid["x"] + (hit["x"] + hit["width"] / 2) * grid["cell_width"]),
            round(grid["y"] + (hit["y"] + hit["height"] * y_fraction) * grid["cell_height"]),
        )

    def point(self, x, y):
        self.command("mousemove", "--window", self.window, x, y)

    def hover(self, identifier):
        self.point(*self.hit_position(identifier))

    def click(self, identifier):
        self.hover(identifier)
        self.command("click", "1")

    def drag(self, source, target, y_fraction=0.5):
        start, end = self.hit_position(source), self.hit_position(target, y_fraction)
        self.point(*start)
        self.command("mousedown", "1")
        try:
            for index in range(1, 13):
                fraction = index / 12
                self.point(
                    round(start[0] + (end[0] - start[0]) * fraction),
                    round(start[1] + (end[1] - start[1]) * fraction),
                )
                time.sleep(0.018)
        finally:
            self.command("mouseup", "1")

    def capture(self, name, pause=0.18):
        self.probe.sample_for(pause)
        path = self.output / f"{name}.png"
        subprocess.run(
            [
                "import",
                "-display",
                self.display.env["DISPLAY"],
                "-window",
                self.window,
                str(path),
            ],
            env=self.display.env,
            check=True,
            capture_output=True,
            timeout=10,
        )
        assert path.is_file(), "GUI capture is missing"
        assert path.stat().st_size > 1000, "GUI capture is empty"
        self.captures.append(str(path))
        return path

    def terminal_text_pixels(self, capture):
        state = self.state()
        cell = state["model"]["grid"]
        content = state["content"]
        x = round(content["x"] + 2 * cell["cell_width"])
        y = round(content["y"] + cell["cell_height"])
        width, height = round(18 * cell["cell_width"]), round(cell["cell_height"])
        command = shutil.which("magick") or shutil.which("convert")
        if command is None:
            raise RuntimeError("ImageMagick is required for terminal rendering checks")
        pixels = subprocess.run(
            [
                command,
                str(capture),
                "-crop",
                f"{width}x{height}+{x}+{y}",
                "-depth",
                "8",
                "rgb:-",
            ],
            env=self.display.env,
            check=True,
            capture_output=True,
            timeout=10,
        ).stdout
        assert len(set(pixels)) > 8, "Terminal fixture text is not visible"
        return pixels

    def start_input_capture(self):
        self.focus_terminal()
        self.text(
            r'printf "\033[2J\033[HWorkspace ready\n\n"; stty -echo -icanon min 1 time 0; cat > "$WEZ_VTABS_SCENARIO/physical-input.bin"'
        )
        self.key("Return")
        path = self.probe.root / "physical-input.bin"
        wait_until(
            path.exists,
            timeout=5,
            interval=0.02,
            message="Physical terminal input capture did not start",
            check_live=lambda: check_process(self.probe.gui_process, "GUI"),
            diagnostics=lambda: log_diagnostics(self.probe.root / "gui.log"),
        )
        return path

    def focus_terminal(self):
        state = self.state()
        content = state["content"]
        self.point(
            round(content["x"] + content["width"] / 2),
            round(content["y"] + content["height"] / 2),
        )
        self.command("click", "1")
