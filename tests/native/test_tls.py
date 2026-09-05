"""Exercise TLS trust, hostname verification, and mutual client authentication."""

from __future__ import annotations

import asyncio
import concurrent.futures
import os
import shutil
import socket
import ssl
import sys
from pathlib import Path
from unittest.mock import patch

import pytest

from tests.native import tls_fixture


@pytest.fixture(scope="session")
def certificates(tmp_path_factory: pytest.TempPathFactory) -> Path:
    if shutil.which("openssl") is None:
        pytest.skip("Install OpenSSL to run the real TLS handshake tests")
    root = tmp_path_factory.mktemp("tls") / "certificates"
    tls_fixture.generate_certificates(root, "fixture-user")
    return root


def client_context(
    root: Path, *, authenticated: bool = True, trusted: bool = True
) -> ssl.SSLContext:
    context = ssl.create_default_context(cafile=str(root / "ca.pem") if trusted else None)
    if authenticated:
        context.load_cert_chain(str(root / "client.pem"), str(root / "client.key"))
    return context


def connect(
    root: Path,
    context: ssl.SSLContext,
    hostname: str = "localhost",
    *,
    capture_client_errors: bool = False,
):
    server = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    server.load_cert_chain(str(root / "server.pem"), str(root / "server.key"))
    server.load_verify_locations(cafile=str(root / "ca.pem"))
    server.verify_mode = ssl.CERT_REQUIRED
    with (
        socket.socket() as listener,
        concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor,
    ):
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        listener.settimeout(3)

        def accept():
            with listener.accept()[0] as connection:
                connection.settimeout(3)
                try:
                    with server.wrap_socket(connection, server_side=True) as secure:
                        peer = secure.getpeercert()
                        if secure.recv(8) == b"ping":
                            secure.sendall(b"accepted")
                        return peer
                except ssl.SSLError as error:
                    return error

        result = executor.submit(accept)
        try:
            with socket.create_connection(listener.getsockname(), timeout=3) as connection:
                with context.wrap_socket(connection, server_hostname=hostname) as secure:
                    secure.sendall(b"ping")
                    received = secure.recv(8)
            return received, result.result(timeout=3)
        except (ssl.SSLError, ConnectionResetError, BrokenPipeError) as error:
            if capture_client_errors:
                return error, result.result(timeout=3)
            if isinstance(error, ssl.SSLError):
                raise
            rejection = result.result(timeout=3)
            if isinstance(rejection, ssl.SSLError):
                raise rejection from error
            raise


async def test_mutual_tls_authenticates_the_expected_client(certificates: Path):
    response, certificate = await asyncio.to_thread(
        connect, certificates, client_context(certificates)
    )
    assert response == b"accepted"
    assert (("commonName", "fixture-user"),) in certificate["subject"]


@pytest.mark.parametrize("hostname", ["wrong.invalid", "localhost.invalid"])
async def test_connection_rejects_the_wrong_server_hostname(certificates: Path, hostname: str):
    with pytest.raises(ssl.SSLCertVerificationError, match="Hostname mismatch"):
        await asyncio.to_thread(connect, certificates, client_context(certificates), hostname)


async def test_peer_reset_does_not_mask_client_certificate_verification(
    certificates: Path,
):
    wrap_socket = ssl.SSLContext.wrap_socket

    def wrap(context, *args, **kwargs):
        try:
            return wrap_socket(context, *args, **kwargs)
        except ssl.SSLError as error:
            if kwargs.get("server_side"):
                raise ConnectionResetError(10054, "Peer closed the connection") from error
            raise

    with patch.object(ssl.SSLContext, "wrap_socket", wrap):
        with pytest.raises(ssl.SSLCertVerificationError, match="Hostname mismatch"):
            await asyncio.to_thread(
                connect, certificates, client_context(certificates), "wrong.invalid"
            )


async def test_connection_rejects_an_untrusted_server(certificates: Path):
    with pytest.raises(ssl.SSLCertVerificationError):
        await asyncio.to_thread(connect, certificates, client_context(certificates, trusted=False))


@pytest.mark.parametrize("version", [ssl.TLSVersion.TLSv1_2, ssl.TLSVersion.TLSv1_3])
async def test_server_requires_a_client_certificate(certificates: Path, version: ssl.TLSVersion):
    context = client_context(certificates, authenticated=False)
    context.minimum_version = context.maximum_version = version
    response, rejection = await asyncio.to_thread(
        connect, certificates, context, capture_client_errors=True
    )
    # TLS 1.3 may finish the client handshake before the server's rejection arrives.
    # An EOF is valid only when the server independently rejected the missing identity.
    assert isinstance(rejection, ssl.SSLError), (
        f"server accepted an unauthenticated peer: {rejection!r}"
    )
    assert rejection.reason == "PEER_DID_NOT_RETURN_A_CERTIFICATE"
    assert response == b"" or isinstance(
        response, (ssl.SSLError, ConnectionResetError, BrokenPipeError)
    ), f"unauthenticated client received application data: {response!r}"


@pytest.mark.skipif(os.name == "nt", reason="POSIX private key permissions")
def test_private_keys_are_owner_only(certificates: Path):
    assert certificates.stat().st_mode & 0o777 == 0o700
    for key in certificates.glob("*.key"):
        assert key.stat().st_mode & 0o777 == 0o600


def test_certificate_creation_refuses_to_overwrite_an_existing_identity(
    certificates: Path,
):
    certificate = (certificates / "ca.pem").read_bytes()
    with pytest.raises(FileExistsError):
        tls_fixture.generate_certificates(certificates, "fixture-user")
    assert (certificates / "ca.pem").read_bytes() == certificate


def test_cleanup_removes_all_owned_private_keys(tmp_path: Path):
    fixture = tls_fixture.LocalTlsMux(tmp_path / "fixture", Path("unused-server"))
    fixture.certificates.mkdir(parents=True)
    fixture.owns_certificates = True
    for name in ("ca.key", "client.key", "server.key"):
        (fixture.certificates / name).write_text("private fixture key", encoding="utf-8")
    fixture.close()
    fixture.close()
    assert list(fixture.certificates.glob("*.key")) == []


def test_failed_start_preserves_an_existing_identity(tmp_path: Path):
    fixture = tls_fixture.LocalTlsMux(tmp_path / "fixture", Path(sys.executable))
    fixture.certificates.mkdir(parents=True)
    key = fixture.certificates / "client.key"
    key.write_text("existing identity", encoding="utf-8")
    try:
        with pytest.raises(FileExistsError):
            fixture.start()
    finally:
        fixture.close()
    assert key.read_text(encoding="utf-8") == "existing identity"
