"""Desktop application replacement through the production CLI."""

import json
import sys

import pytest

from tests.tools.support import binary_dir, executable_name

pytestmark = pytest.mark.rust


def state(root, name):
    return json.loads((root / f"{name}.json").read_text())


def installed_gui(sandbox, name):
    return (
        binary_dir(sandbox.install / f"versions/{name}") / executable_name("wezterm-gui")
    ).read_bytes()


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
    assert gui.read_bytes() == installed_gui(tools_sandbox, "first")
    assert (app / "Contents/Resources/native-bundle.json").is_file()

    result = tools_sandbox.json("deploy", "--bundle", bundle_factory("second"), "--app", app)

    assert state(tools_sandbox.install, "active")["id"] == "second"
    assert result["app"]["replaced"]["kind"] == "previous"
    assert gui.read_bytes() == installed_gui(tools_sandbox, "second")
    assert (preserved / "Contents/Info.plist").read_text() == "stock"
    assert [path.name for path in applications.iterdir()] == ["WezTerm.app"]


def test_deploy_without_an_application_only_installs(tools_sandbox, bundle_factory):
    result = tools_sandbox.json("deploy", "--bundle", bundle_factory("only"), "--no-app")

    assert state(tools_sandbox.install, "active")["id"] == "only"
    assert result["app"] is None
    assert not (tools_sandbox.install / "replaced").exists()
