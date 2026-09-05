"""Owned X11 displays for native tests, with no access to the user's desktop."""

from __future__ import annotations

import os
import selectors
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path


class HeadlessDisplay:
    def __init__(self, root: Path):
        self.root = root.resolve()
        self.env = {
            key: value
            for key, value in os.environ.items()
            if key
            not in {
                "DISPLAY",
                "WAYLAND_DISPLAY",
                "XAUTHORITY",
                "SSH_AUTH_SOCK",
                "DBUS_SESSION_BUS_ADDRESS",
            }
            and not key.startswith("XDG_SESSION")
            and not key.startswith(("WEZTERM_", "WEZ_VTABS_"))
        }
        self.xserver: subprocess.Popen | None = None
        self.manager: subprocess.Popen | None = None
        self.logs: list = []

    @property
    def owned(self) -> bool:
        return all(
            process is not None and process.poll() is None
            for process in (self.xserver, self.manager)
        )

    def assert_live(self) -> None:
        if not self.owned or not self.env.get("DISPLAY"):
            raise RuntimeError("native tests require a live, owned HeadlessDisplay")

    def start(self) -> HeadlessDisplay:
        if self.owned:
            return self
        if sys.platform != "linux":
            raise RuntimeError("headless native tests require Linux with Xvfb and Openbox")
        xvfb, openbox, xdotool = (
            shutil.which("Xvfb"),
            shutil.which("openbox"),
            shutil.which("xdotool"),
        )
        if not xvfb or not openbox or not xdotool:
            raise RuntimeError("install Xvfb, Openbox, and xdotool to run isolated native tests")
        self.root.mkdir(mode=0o700, parents=True, exist_ok=True)
        runtime = self.root / "runtime"
        runtime.mkdir(mode=0o700, exist_ok=True)
        runtime.chmod(0o700)
        self.env["XDG_RUNTIME_DIR"] = str(runtime)
        config = self.root / "config"
        config.mkdir(mode=0o700, exist_ok=True)
        self.env["XDG_CONFIG_HOME"] = str(config)
        manager_config = self.root / "openbox.xml"
        manager_config.write_text(
            '<openbox_config xmlns="http://openbox.org/3.4/rc"><keyboard/><mouse/></openbox_config>\n',
            encoding="utf-8",
        )
        read_fd, write_fd = os.pipe()
        log = (self.root / "xvfb.log").open("wb")
        self.logs.append(log)
        try:
            self.xserver = subprocess.Popen(
                [
                    xvfb,
                    "-displayfd",
                    str(write_fd),
                    "-screen",
                    "0",
                    "1280x900x24",
                    "-nolisten",
                    "tcp",
                ],
                pass_fds=(write_fd,),
                env=self.env,
                stdin=subprocess.DEVNULL,
                stdout=log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
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
            log = (self.root / "openbox.log").open("wb")
            self.logs.append(log)
            self.manager = subprocess.Popen(
                [openbox, "--sm-disable", "--config-file", str(manager_config)],
                env=self.env,
                stdin=subprocess.DEVNULL,
                stdout=log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            deadline = time.monotonic() + 10
            while True:
                self.assert_live()
                ready = subprocess.run(
                    [xdotool, "get_desktop"],
                    env=self.env,
                    capture_output=True,
                    timeout=2,
                )
                if ready.returncode == 0:
                    break
                if time.monotonic() >= deadline:
                    raise RuntimeError(f"Openbox did not become ready: {self.root / 'openbox.log'}")
                time.sleep(0.02)
            return self
        except BaseException:
            self.close()
            raise
        finally:
            os.close(read_fd)
            if write_fd != -1:
                os.close(write_fd)

    def close(self) -> None:
        for process in (self.manager, self.xserver):
            if process is None or process.poll() is not None:
                continue
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait(timeout=5)
        self.env.pop("DISPLAY", None)
        for log in self.logs:
            log.close()
        self.logs.clear()

    def __enter__(self) -> HeadlessDisplay:
        return self.start()

    def __exit__(self, *exc) -> None:
        self.close()
