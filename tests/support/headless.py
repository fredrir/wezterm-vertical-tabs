"""Owned X11 displays for tests, with no access to the user's desktop."""

from __future__ import annotations

import os
import selectors
import shutil
import subprocess
import sys
from contextlib import ExitStack
from pathlib import Path

from tests.support.environment import clean_env, xdg_tree
from tests.support.polling import log_diagnostics, wait_until
from tests.support.process import spawn_logged, stop_process


class HeadlessDisplay:
    def __init__(self, root: Path):
        self.root = root.resolve()
        self.env = clean_env()
        self.xserver: subprocess.Popen | None = None
        self.manager: subprocess.Popen | None = None

    @property
    def owned(self) -> bool:
        return all(
            process is not None and process.poll() is None
            for process in (self.xserver, self.manager)
        )

    def assert_live(self) -> None:
        if not self.owned or not self.env.get("DISPLAY"):
            raise RuntimeError("tests require a live, owned HeadlessDisplay")

    def start(self) -> HeadlessDisplay:
        if self.owned:
            return self
        if sys.platform != "linux":
            raise RuntimeError("headless tests require Linux with Xvfb and Openbox")
        xvfb, openbox, xdotool = (
            shutil.which("Xvfb"),
            shutil.which("openbox"),
            shutil.which("xdotool"),
        )
        if not xvfb or not openbox or not xdotool:
            raise RuntimeError("install Xvfb, Openbox, and xdotool to run isolated tests")
        self.root.mkdir(mode=0o700, parents=True, exist_ok=True)
        self.env.update(xdg_tree(self.root))
        manager_config = self.root / "openbox.xml"
        manager_config.write_text(
            '<openbox_config xmlns="http://openbox.org/3.4/rc"><keyboard/><mouse/></openbox_config>\n',
            encoding="utf-8",
        )
        read_fd, write_fd = os.pipe()
        try:
            self.xserver = spawn_logged(
                [
                    xvfb,
                    "-displayfd",
                    str(write_fd),
                    "-screen",
                    "0",
                    "1600x1100x24",
                    "-nolisten",
                    "tcp",
                ],
                self.root / "xvfb.log",
                env=self.env,
                pass_fds=(write_fd,),
            )
            os.close(write_fd)
            write_fd = -1
            with selectors.DefaultSelector() as selector:
                selector.register(read_fd, selectors.EVENT_READ)
                if not selector.select(timeout=10):
                    raise RuntimeError(f"Xvfb did not become ready: {self.root / 'xvfb.log'}")
                number = os.read(read_fd, 32).decode("ascii").strip()
            if not number.isdecimal():
                raise RuntimeError(f"Xvfb failed to allocate a display: {self.root / 'xvfb.log'}")
            self.env["DISPLAY"] = f":{number}"
            self.manager = spawn_logged(
                [openbox, "--sm-disable", "--config-file", manager_config],
                self.root / "openbox.log",
                env=self.env,
            )

            def ready():
                result = subprocess.run(
                    [xdotool, "get_desktop"],
                    env=self.env,
                    capture_output=True,
                    timeout=2,
                )
                return result.returncode == 0

            wait_until(
                ready,
                timeout=10,
                interval=0.02,
                message="Openbox did not become ready",
                check_live=self.assert_live,
                diagnostics=lambda: "\n".join(
                    log_diagnostics(self.root / name) for name in ("xvfb.log", "openbox.log")
                ),
                error=RuntimeError,
            )
            return self
        except BaseException:
            self.close()
            raise
        finally:
            os.close(read_fd)
            if write_fd != -1:
                os.close(write_fd)

    def close(self) -> None:
        with ExitStack() as cleanup:
            cleanup.callback(self.env.pop, "DISPLAY", None)
            for process in (self.xserver, self.manager):
                if process is not None:
                    cleanup.callback(stop_process, process)

    def __enter__(self) -> HeadlessDisplay:
        return self.start()

    def __exit__(self, *exc) -> None:
        self.close()
