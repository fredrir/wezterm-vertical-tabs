"""Desktop application replacement through the production CLI."""

import json
import sys

import pytest

from tests.tools.support import binary_dir, executable_name

pytestmark = pytest.mark.rust


def state(root, name):
    return json.loads((root / f"{name}.json").read_text())


def installed(sandbox, version, name):
    return binary_dir(sandbox.install / f"versions/{version}") / executable_name(name)


@pytest.mark.skipif(sys.platform != "darwin", reason="macOS application bundle placement")
def test_deploy_preserves_a_foreign_application_once_and_refreshes_its_own_copies(
    tools_sandbox, bundle_factory, tmp_path
):
    applications = tmp_path / "Applications"
    app = applications / "WezTerm.app"
    (app / "Contents/MacOS").mkdir(parents=True)
    (app / "Contents/Info.plist").write_text("stock")
    (app / "Contents/MacOS/wezterm-gui").write_text("stock binary")

    result = tools_sandbox.json("deploy", "--bundle", bundle_factory("first"), "--app", app)

    assert state(tools_sandbox.install, "active")["id"] == "first"
    assert result["app"]["replaced"]["kind"] == "foreign"
    preserved = tools_sandbox.install / "replaced/WezTerm.app"
    assert (preserved / "Contents/Info.plist").read_text() == "stock"
    gui = app / "Contents/MacOS" / executable_name("wezterm-gui")
    assert gui.read_bytes() == installed(tools_sandbox, "first", "wezterm-gui").read_bytes()
    assert (app / "Contents/Resources/bundle.json").is_file()

    result = tools_sandbox.json("deploy", "--bundle", bundle_factory("second"), "--app", app)

    assert state(tools_sandbox.install, "active")["id"] == "second"
    assert result["app"]["replaced"]["kind"] == "previous"
    assert gui.read_bytes() == installed(tools_sandbox, "second", "wezterm-gui").read_bytes()
    assert (preserved / "Contents/Info.plist").read_text() == "stock"
    assert [path.name for path in applications.iterdir()] == ["WezTerm.app"]


@pytest.mark.skipif(sys.platform != "linux", reason="XDG desktop entry and PATH link placement")
def test_deploy_shadows_the_packaged_desktop_entry_and_cli_once(
    tools_sandbox, bundle_factory, tmp_path
):
    entry = tmp_path / "share/applications/org.wezfurlong.wezterm.desktop"
    entry.parent.mkdir(parents=True)
    entry.write_text("[Desktop Entry]\nName=Stock\n")
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    (bin_dir / "wezterm").write_text("stock cli")

    result = tools_sandbox.json(
        "deploy", "--bundle", bundle_factory("first"), "--app", entry, "--bin", bin_dir
    )

    assert state(tools_sandbox.install, "active")["id"] == "first"
    text = entry.read_text()
    assert "Name=WezTerm\n" in text
    assert f'Exec="{tools_sandbox.install / "wez-vtabs-launcher"}" launch\n' in text
    assert "StartupWMClass=org.wezfurlong.wezterm" in text
    assert result["app"]["replaced"]["kind"] == "foreign"
    replaced = tools_sandbox.install / "replaced"
    assert (
        replaced / "org.wezfurlong.wezterm.desktop"
    ).read_text() == "[Desktop Entry]\nName=Stock\n"
    assert (replaced / "wezterm").read_text() == "stock cli"
    for name in ("wezterm", "wezterm-gui", "wezterm-mux-server"):
        link = bin_dir / name
        assert link.is_symlink()
        assert link.resolve() == installed(tools_sandbox, "first", name).resolve()
    assert [link["replaced"] for link in result["app"]["links"]] == [
        {"kind": "foreign", "preserved": str(replaced / "wezterm")},
        None,
        None,
    ]

    result = tools_sandbox.json(
        "deploy", "--bundle", bundle_factory("second"), "--app", entry, "--bin", bin_dir
    )

    assert state(tools_sandbox.install, "active")["id"] == "second"
    assert result["app"]["replaced"]["kind"] == "previous"
    assert all(link["replaced"] == {"kind": "previous"} for link in result["app"]["links"])
    for name in ("wezterm", "wezterm-gui", "wezterm-mux-server"):
        assert (bin_dir / name).resolve() == installed(tools_sandbox, "second", name).resolve()
    assert (replaced / "wezterm").read_text() == "stock cli"
    assert sorted(path.name for path in bin_dir.iterdir()) == [
        "wezterm",
        "wezterm-gui",
        "wezterm-mux-server",
    ]
    # update-desktop-database may add its cache beside the entry; no retired copies remain.
    assert not [path.name for path in entry.parent.iterdir() if ".retired-" in path.name]


def test_deploy_without_an_application_only_installs(tools_sandbox, bundle_factory):
    result = tools_sandbox.json("deploy", "--bundle", bundle_factory("only"), "--no-app")

    assert state(tools_sandbox.install, "active")["id"] == "only"
    assert result["app"] is None
    assert not (tools_sandbox.install / "replaced").exists()
