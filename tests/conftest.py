from __future__ import annotations

import os
import subprocess
from pathlib import Path

import pytest
from filelock import FileLock

PROJECT_ROOT = Path(__file__).resolve().parents[1]


def pytest_addoption(parser: pytest.Parser) -> None:
    group = parser.getgroup("native behavior tests")
    group.addoption("--run-container", action="store_true", help="Run isolated container scenarios")
    group.addoption(
        "--run-native",
        action="store_true",
        help="Run prebuilt native integration scenarios",
    )
    group.addoption(
        "--run-luals",
        action="store_true",
        help="Run Lua language-server contract checks",
    )
    group.addoption(
        "--rust-bin-dir",
        type=Path,
        help="Use production Rust binaries from this directory",
    )
    group.addoption(
        "--native-bin-dir",
        type=Path,
        help="Use prebuilt native WezTerm binaries from this directory",
    )
    group.addoption(
        "--tools-bin",
        type=Path,
        help="Use this prebuilt wez-vtabs management CLI for tooling tests",
    )


def pytest_collection_modifyitems(config: pytest.Config, items: list[pytest.Item]) -> None:
    for marker, option in (
        ("container", "--run-container"),
        ("native", "--run-native"),
        ("luals", "--run-luals"),
    ):
        if not config.getoption(option):
            skip = pytest.mark.skip(reason=f"Requires explicit {option}")
            for item in items:
                if item.get_closest_marker(marker) is not None:
                    item.add_marker(skip)


@pytest.fixture(scope="session")
def project_root() -> Path:
    return PROJECT_ROOT


@pytest.fixture
def isolated_env(tmp_path: Path) -> dict[str, str]:
    root = tmp_path / ".environment"
    root.mkdir()
    environment = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith(("WEZTERM_", "WEZ_VTABS_"))
        and key not in {"DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY"}
    }
    for name in ("config", "cache", "data", "state", "runtime"):
        path = root / name
        path.mkdir(mode=0o700, exist_ok=True)
        variable = "XDG_RUNTIME_DIR" if name == "runtime" else f"XDG_{name.upper()}_HOME"
        environment[variable] = str(path)
    environment.update(
        WEZ_VTABS_CACHE=str(root / "native-cache"),
        WEZ_VTABS_INSTALL=str(root / "native-install"),
        PYTHONUTF8="1",
    )
    return environment


@pytest.fixture(autouse=True)
def isolate_application_environment(
    monkeypatch: pytest.MonkeyPatch, isolated_env: dict[str, str]
) -> None:
    for key in tuple(os.environ):
        if key.startswith(("WEZTERM_", "WEZ_VTABS_")) or key in {
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XAUTHORITY",
        }:
            monkeypatch.delenv(key, raising=False)
    for key, value in isolated_env.items():
        if key.startswith(("XDG_", "WEZ_VTABS_")) or key == "PYTHONUTF8":
            monkeypatch.setenv(key, value)


@pytest.fixture(scope="session")
def rust_binaries(
    pytestconfig: pytest.Config,
    tmp_path_factory: pytest.TempPathFactory,
    worker_id: str,
) -> dict[str, Path]:
    names = ("gen-schema", "wez-vtabs-store")
    binary_dir = pytestconfig.getoption("--rust-bin-dir")
    if binary_dir is None:
        target = PROJECT_ROOT / "target" / "pytest"
        target.mkdir(parents=True, exist_ok=True)
        run_root = tmp_path_factory.getbasetemp()
        if worker_id != "master":
            run_root = run_root.parent
        ready = run_root / "rust-binaries-ready"
        with FileLock(target / "build.lock", timeout=180):
            if not ready.is_file():
                result = subprocess.run(
                    [
                        "cargo",
                        "build",
                        "--locked",
                        "-p",
                        "vtabs-core",
                        "--bin",
                        "gen-schema",
                        "-p",
                        "vtabs-store",
                        "--bin",
                        "wez-vtabs-store",
                        "--features",
                        "vtabs-store/sqlite",
                    ],
                    cwd=PROJECT_ROOT,
                    env={**os.environ, "CARGO_TARGET_DIR": str(target)},
                    text=True,
                    capture_output=True,
                    timeout=180,
                    check=False,
                )
                if result.returncode:
                    pytest.fail(
                        f"Production Rust tool build failed:\n{result.stdout}\n{result.stderr}"
                    )
                ready.write_text("ready\n", encoding="utf-8")
        binary_dir = target / "debug"
    suffix = ".exe" if os.name == "nt" else ""
    binaries = {name: binary_dir.resolve() / f"{name}{suffix}" for name in names}
    for name, path in binaries.items():
        if not path.is_file():
            pytest.fail(f"Production Rust binary {name} is missing: {path}")
    return binaries


@pytest.fixture(scope="session")
def tools_binary(
    pytestconfig: pytest.Config,
    tmp_path_factory: pytest.TempPathFactory,
    worker_id: str,
) -> Path:
    """Build the production manager once across workers, without building WezTerm."""
    supplied = pytestconfig.getoption("--tools-bin")
    if supplied is not None:
        binary = supplied.resolve()
    else:
        target = PROJECT_ROOT / "target" / "pytest"
        target.mkdir(parents=True, exist_ok=True)
        run_root = tmp_path_factory.getbasetemp()
        if worker_id != "master":
            run_root = run_root.parent
        ready = run_root / "tools-binary-ready"
        with FileLock(target / "build.lock", timeout=180):
            if not ready.is_file():
                result = subprocess.run(
                    [
                        "cargo",
                        "build",
                        "--locked",
                        "--manifest-path",
                        str(PROJECT_ROOT / "tools/Cargo.toml"),
                        "--bin",
                        "wez-vtabs",
                    ],
                    cwd=PROJECT_ROOT,
                    env={**os.environ, "CARGO_TARGET_DIR": str(target)},
                    text=True,
                    capture_output=True,
                    timeout=180,
                    check=False,
                )
                if result.returncode:
                    pytest.fail(f"Management CLI build failed:\n{result.stdout}\n{result.stderr}")
                ready.write_text("ready\n", encoding="utf-8")
        suffix = ".exe" if os.name == "nt" else ""
        binary = target / "debug" / f"wez-vtabs{suffix}"
    if not binary.is_file():
        pytest.fail(f"Production management CLI is missing: {binary}")
    return binary


@pytest.fixture(scope="session")
def rust_host() -> str:
    result = subprocess.run(
        ["rustc", "-vV"], text=True, capture_output=True, timeout=20, check=True
    )
    for line in result.stdout.splitlines():
        if line.startswith("host: "):
            return line.removeprefix("host: ")
    pytest.fail(f"rustc did not report a host target:\n{result.stdout}")
