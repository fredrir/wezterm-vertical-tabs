"""Verify the TLS fixture's trust, hostname checking, and client authentication."""
import concurrent.futures
import importlib.util
import os
from pathlib import Path
import shutil
import socket
import ssl
import sys
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location("tls_fixture", Path(__file__).with_name("tls_fixture.py"))
tls_fixture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(tls_fixture)


@unittest.skipUnless(shutil.which("openssl"), "openssl is required for TLS certificate tests")
class TlsFixtureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="vtabs-tls-tests-")
        cls.root = Path(cls.temporary.name) / "certificates"
        tls_fixture.generate_certificates(cls.root, "fixture-user")

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def client(self, authenticated=True, trusted=True):
        context = ssl.create_default_context(
            cafile=str(self.root / "ca.pem") if trusted else None)
        if authenticated:
            context.load_cert_chain(str(self.root / "client.pem"), str(self.root / "client.key"))
        return context

    def connect(self, context, hostname="localhost"):
        server = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        server.load_cert_chain(str(self.root / "server.pem"), str(self.root / "server.key"))
        server.load_verify_locations(cafile=str(self.root / "ca.pem"))
        server.verify_mode = ssl.CERT_REQUIRED
        with socket.socket() as listener, concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
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
            except (ConnectionResetError, BrokenPipeError) as error:
                rejection = result.result(timeout=3)
                if isinstance(rejection, ssl.SSLError):
                    raise rejection from error
                raise
            finally:
                result.result(timeout=3)

    def test_mutual_tls_authenticates_the_expected_client(self):
        response, certificate = self.connect(self.client())
        self.assertEqual(response, b"accepted")
        self.assertIn((("commonName", "fixture-user"),), certificate["subject"])

    def test_connection_rejects_the_wrong_server_hostname(self):
        with self.assertRaises(ssl.SSLCertVerificationError):
            self.connect(self.client(), "wrong.invalid")

    def test_connection_rejects_an_untrusted_server(self):
        with self.assertRaises(ssl.SSLCertVerificationError):
            self.connect(self.client(trusted=False))

    def test_server_requires_a_client_certificate(self):
        with self.assertRaises(ssl.SSLError):
            self.connect(self.client(authenticated=False))

    @unittest.skipIf(os.name == "nt", "POSIX private key permissions")
    def test_private_keys_are_owner_only(self):
        self.assertEqual(self.root.stat().st_mode & 0o777, 0o700)
        for key in self.root.glob("*.key"):
            self.assertEqual(key.stat().st_mode & 0o777, 0o600)

    def test_certificate_creation_refuses_to_overwrite_an_existing_identity(self):
        certificate = (self.root / "ca.pem").read_bytes()
        with self.assertRaises(FileExistsError):
            tls_fixture.generate_certificates(self.root, "fixture-user")
        self.assertEqual((self.root / "ca.pem").read_bytes(), certificate)

    def test_cleanup_removes_all_owned_private_keys(self):
        with tempfile.TemporaryDirectory(prefix="vtabs-tls-cleanup-") as directory:
            fixture = tls_fixture.LocalTlsMux(Path(directory), Path("unused-server"))
            fixture.certificates.mkdir()
            fixture.owns_certificates = True
            for name in ("ca.key", "client.key", "server.key"):
                (fixture.certificates / name).write_text("private fixture key", encoding="utf-8")
            fixture.close()
            fixture.close()
            self.assertEqual(list(fixture.certificates.glob("*.key")), [])

    def test_failed_start_preserves_an_existing_identity(self):
        with tempfile.TemporaryDirectory(prefix="vtabs-tls-existing-") as directory:
            fixture = tls_fixture.LocalTlsMux(Path(directory), Path(sys.executable))
            fixture.certificates.mkdir()
            key = fixture.certificates / "client.key"
            key.write_text("existing identity", encoding="utf-8")
            try:
                with self.assertRaises(FileExistsError):
                    fixture.start()
            finally:
                fixture.close()
            self.assertEqual(key.read_text(encoding="utf-8"), "existing identity")


if __name__ == "__main__":
    unittest.main()
