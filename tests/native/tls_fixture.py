#!/usr/bin/env python3
"""Run TLS-mux scenarios against an isolated localhost server with mutual TLS."""

from __future__ import annotations

import argparse
import getpass
import json
import os
import secrets
import shutil
import signal
import socket
import ssl
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def generate_certificates(root: Path, username: str) -> None:
    """Create a short-lived CA and separate server/client identities in an owned directory."""
    openssl = shutil.which("openssl")
    if not openssl:
        raise RuntimeError("openssl is required to create the disposable TLS fixture")
    if not username or any(ord(char) < 32 or ord(char) == 127 for char in username):
        raise ValueError("TLS client username must contain printable characters")
    root.mkdir(mode=0o700, parents=True, exist_ok=False)

    def run(*arguments):
        subprocess.run([openssl, *arguments], check=True, capture_output=True, timeout=30)

    run(
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-days",
        "1",
        "-subj",
        "/CN=WezTerm fixture CA",
        "-addext",
        "basicConstraints=critical,CA:TRUE",
        "-addext",
        "keyUsage=critical,keyCertSign,cRLSign",
        "-keyout",
        str(root / "ca.key"),
        "-out",
        str(root / "ca.pem"),
    )
    names = {
        "server": ("localhost", "serverAuth", "subjectAltName=DNS:localhost,IP:127.0.0.1\n"),
        "client": (username, "clientAuth", ""),
    }
    for name, (common_name, usage, alternatives) in names.items():
        subject = "/CN=" + common_name.replace("\\", "\\\\").replace("/", "\\/")
        run(
            "req",
            "-new",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-subj",
            subject,
            "-keyout",
            str(root / f"{name}.key"),
            "-out",
            str(root / f"{name}.csr"),
        )
        extensions = root / f"{name}.ext"
        extensions.write_text(
            "basicConstraints=critical,CA:FALSE\n"
            "keyUsage=critical,digitalSignature,keyEncipherment\n"
            f"extendedKeyUsage={usage}\n{alternatives}",
            encoding="utf-8",
        )
        run(
            "x509",
            "-req",
            "-in",
            str(root / f"{name}.csr"),
            "-CA",
            str(root / "ca.pem"),
            "-CAkey",
            str(root / "ca.key"),
            "-set_serial",
            str(secrets.randbits(128) or 1),
            "-days",
            "1",
            "-extfile",
            str(extensions),
            "-out",
            str(root / f"{name}.pem"),
        )
    for key in root.glob("*.key"):
        key.chmod(0o600)


class LocalTlsMux:
    def __init__(self, root: Path, server: Path):
        self.root, self.server = root.resolve(), server.resolve()
        self.certificates = self.root / "certificates"
        self.domain = self.root / "tls.json"
        self.process = None
        self.log = None
        self.owns_certificates = False

    def start(self) -> Path:
        if not self.server.is_file():
            raise RuntimeError(f"mux server binary is unavailable: {self.server}")
        self.root.mkdir(mode=0o700, parents=True, exist_ok=True)
        if any(self.root.iterdir()):
            raise FileExistsError(f"TLS fixture requires an empty output directory: {self.root}")
        self.root.chmod(0o700)
        username = getpass.getuser()
        self.owns_certificates = True
        try:
            generate_certificates(self.certificates, username)
        except FileExistsError:
            self.owns_certificates = False
            raise
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        self.address = ("127.0.0.1", port)
        ca = str(self.certificates / "ca.pem")
        self.domain.write_text(
            json.dumps(
                {
                    "name": "scenario-tls",
                    "remote_address": f"127.0.0.1:{port}",
                    "pem_cert": str(self.certificates / "client.pem"),
                    "pem_private_key": str(self.certificates / "client.key"),
                    "pem_root_certs": [ca],
                    "expected_cn": "localhost",
                    "accept_invalid_hostnames": False,
                    "connect_automatically": False,
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )

        def quote(value):
            return json.dumps(str(value), ensure_ascii=False)

        self.config = self.root / "mux.lua"
        shell = os.environ.get("COMSPEC", "cmd.exe") if os.name == "nt" else "/bin/sh"
        self.config.write_text(
            "return {check_for_updates=false,automatically_reload_config=false,"
            "default_prog={"
            + quote(shell)
            + "},unix_domains={{name='tls-fixture',socket_path="
            + quote(self.root / "mux.sock")
            + ",no_serve_automatically=true}},tls_servers={{bind_address="
            + quote(f"127.0.0.1:{port}")
            + ",pem_cert="
            + quote(self.certificates / "server.pem")
            + ",pem_private_key="
            + quote(self.certificates / "server.key")
            + ",pem_root_certs={"
            + quote(ca)
            + "}}}}\n",
            encoding="utf-8",
        )
        env = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith(("WEZTERM_", "WEZ_VTABS_"))
        }
        for name in ("config", "cache", "data", "state", "runtime"):
            directory = self.root / name
            directory.mkdir(mode=0o700)
            variable = "XDG_RUNTIME_DIR" if name == "runtime" else f"XDG_{name.upper()}_HOME"
            env[variable] = str(directory)
        env["USER"] = username
        self.log = (self.root / "mux.log").open("wb")
        self.process = subprocess.Popen(
            [str(self.server), "--config-file", str(self.config)],
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=self.log,
            stderr=subprocess.STDOUT,
            start_new_session=os.name != "nt",
        )
        context = ssl.create_default_context(cafile=ca)
        context.load_cert_chain(
            str(self.certificates / "client.pem"), str(self.certificates / "client.key")
        )
        deadline = time.monotonic() + 15
        while True:
            if self.process.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError(
                    "isolated TLS mux startup failed: "
                    + (self.root / "mux.log").read_text(encoding="utf-8")
                )
            try:
                with socket.create_connection(self.address, timeout=0.5) as connection:
                    with context.wrap_socket(connection, server_hostname="localhost") as secure:
                        self.protocol = secure.version()
                return self.domain
            except (ConnectionRefusedError, TimeoutError):
                time.sleep(0.05)

    def close(self) -> None:
        if self.process is not None and self.process.poll() is None:
            if os.name == "nt":
                self.process.terminate()
            else:
                os.killpg(self.process.pid, signal.SIGTERM)
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                if os.name == "nt":
                    self.process.kill()
                else:
                    os.killpg(self.process.pid, signal.SIGKILL)
                self.process.wait(timeout=5)
        if self.log is not None:
            self.log.close()
        if self.owns_certificates:
            for key in self.certificates.glob("*.key"):
                key.unlink(missing_ok=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mux-server", required=True, type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument(
        "scenario",
        nargs=argparse.REMAINDER,
        help="scenarios.py arguments after --; omit to probe TLS only",
    )
    args = parser.parse_args()
    root = args.output or Path(tempfile.mkdtemp(prefix="vtabs-localhost-tls-"))
    fixture = LocalTlsMux(root, args.mux_server)
    try:
        config = fixture.start()
        print(
            f"TLS-mux authenticated using {fixture.protocol}; fixture: {fixture.root}", flush=True
        )
        scenario = args.scenario[1:] if args.scenario[:1] == ["--"] else args.scenario
        if scenario:
            subprocess.run(
                [
                    sys.executable,
                    str(Path(__file__).with_name("scenarios.py")),
                    "--domain",
                    "tls",
                    "--tls-config",
                    str(config),
                ]
                + scenario,
                check=True,
            )
    finally:
        fixture.close()


if __name__ == "__main__":
    main()
