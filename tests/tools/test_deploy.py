"""Desktop application replacement through the production CLI."""

import json
import subprocess
import sys

import pytest

from tests.tools.support import binary_dir, executable_name

pytestmark = pytest.mark.rust

LINKED_BINARIES = (
    "wezterm",
    "wezterm-gui",
    "wezterm-mux-server",
    "wez-vtabs",
    "wez-vtabs-store",
    "strip-ansi-escapes",
)


@pytest.fixture
def sandbox_home(tools_sandbox, tmp_path):
    """The default `~/.local/bin` placement must not touch the real home directory."""
    tools_sandbox.env["HOME"] = str(tmp_path / "home")
    return tools_sandbox


def state(root, name):
    return json.loads((root / f"{name}.json").read_text())


def installed(sandbox, version, name):
    return binary_dir(sandbox.install / f"versions/{version}") / executable_name(name)


@pytest.mark.skipif(sys.platform != "darwin", reason="macOS application bundle placement")
def test_deploy_preserves_a_foreign_application_once_and_refreshes_its_own_copies(
    sandbox_home, bundle_factory, tmp_path
):
    applications = tmp_path / "Applications"
    app = applications / "WezTerm.app"
    (app / "Contents/MacOS").mkdir(parents=True)
    (app / "Contents/Info.plist").write_text("stock")
    (app / "Contents/MacOS/wezterm-gui").write_text("stock binary")

    result = sandbox_home.json("deploy", "--bundle", bundle_factory("first"), "--app", app)

    assert state(sandbox_home.install, "active")["id"] == "first"
    assert result["app"]["replaced"]["kind"] == "foreign"
    preserved = sandbox_home.install / "replaced/WezTerm.app"
    assert (preserved / "Contents/Info.plist").read_text() == "stock"
    gui = app / "Contents/MacOS" / executable_name("wezterm-gui")
    assert gui.read_bytes() == installed(sandbox_home, "first", "wezterm-gui").read_bytes()
    assert (app / "Contents/Resources/bundle.json").is_file()

    result = sandbox_home.json("deploy", "--bundle", bundle_factory("second"), "--app", app)

    assert state(sandbox_home.install, "active")["id"] == "second"
    assert result["app"]["replaced"]["kind"] == "previous"
    assert gui.read_bytes() == installed(sandbox_home, "second", "wezterm-gui").read_bytes()
    assert (preserved / "Contents/Info.plist").read_text() == "stock"
    assert [path.name for path in applications.iterdir()] == ["WezTerm.app"]


@pytest.mark.skipif(sys.platform != "linux", reason="XDG desktop entry and PATH link placement")
def test_deploy_shadows_the_packaged_desktop_entry_and_cli_once(
    sandbox_home, bundle_factory, tmp_path
):
    entry = tmp_path / "share/applications/org.wezfurlong.wezterm.desktop"
    entry.parent.mkdir(parents=True)
    entry.write_text("[Desktop Entry]\nName=Stock\n")
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    (bin_dir / "wezterm").write_text("stock cli")

    result = sandbox_home.json(
        "deploy", "--bundle", bundle_factory("first"), "--app", entry, "--bin", bin_dir
    )

    assert state(sandbox_home.install, "active")["id"] == "first"
    text = entry.read_text()
    assert "Name=WezTerm\n" in text
    assert f'Exec="{sandbox_home.install / "wez-vtabs-launcher"}" launch\n' in text
    assert "StartupWMClass=org.wezfurlong.wezterm" in text
    assert result["app"]["replaced"]["kind"] == "foreign"
    replaced = sandbox_home.install / "replaced"
    assert (
        replaced / "org.wezfurlong.wezterm.desktop"
    ).read_text() == "[Desktop Entry]\nName=Stock\n"
    assert (replaced / "wezterm").read_text() == "stock cli"
    for name in LINKED_BINARIES:
        link = bin_dir / name
        assert link.is_symlink()
        assert link.resolve() == installed(sandbox_home, "first", name).resolve()
    assert [link["replaced"] for link in result["links"]] == [
        {"kind": "foreign", "preserved": str(replaced / "wezterm")},
        *[None] * (len(LINKED_BINARIES) - 1),
    ]

    result = sandbox_home.json(
        "deploy", "--bundle", bundle_factory("second"), "--app", entry, "--bin", bin_dir
    )

    assert state(sandbox_home.install, "active")["id"] == "second"
    assert result["app"]["replaced"]["kind"] == "previous"
    assert all(link["replaced"] == {"kind": "previous"} for link in result["links"])
    for name in LINKED_BINARIES:
        assert (bin_dir / name).resolve() == installed(sandbox_home, "second", name).resolve()
    assert (replaced / "wezterm").read_text() == "stock cli"
    assert sorted(path.name for path in bin_dir.iterdir()) == sorted(LINKED_BINARIES)
    # update-desktop-database may add its cache beside the entry; no retired copies remain.
    assert not [path.name for path in entry.parent.iterdir() if ".retired-" in path.name]


@pytest.mark.skipif(sys.platform != "darwin", reason="macOS application bundle placement")
def test_deploy_links_every_binary_into_the_default_user_directory(
    sandbox_home, bundle_factory, tmp_path
):
    app = tmp_path / "Applications/WezTerm.app"
    (app / "Contents/MacOS").mkdir(parents=True)

    result = sandbox_home.json("deploy", "--bundle", bundle_factory("first"), "--app", app)

    for name in LINKED_BINARIES:
        link = tmp_path / "home/.local/bin" / name
        assert link.is_symlink()
        assert link.resolve() == installed(sandbox_home, "first", name).resolve()
    assert [link["replaced"] for link in result["links"]] == [None] * len(LINKED_BINARIES)
    version = subprocess.run(
        [str(tmp_path / "home/.local/bin/wez-vtabs"), "--version"],
        env=sandbox_home.env,
        text=True,
        capture_output=True,
        timeout=20,
        check=False,
    )
    assert version.returncode == 0, version.stderr
    assert version.stdout.startswith("wez-vtabs ")


def test_deploy_without_an_application_only_installs(tools_sandbox, bundle_factory):
    result = tools_sandbox.json("deploy", "--bundle", bundle_factory("only"), "--no-app")

    assert state(tools_sandbox.install, "active")["id"] == "only"
    assert result["app"] is None
    assert not (tools_sandbox.install / "replaced").exists()
