"""Release bundles contain runtime assets and reject escaping symlinks."""

import json
import os
import sys
from pathlib import Path

import pytest

from tests.tools.conftest import write_file
from tests.tools.support import binary_dir, executable_name, write_manifest

pytestmark = pytest.mark.rust


@pytest.mark.skipif(os.name == "nt", reason="POSIX symlink fixture")
def test_verify_rejects_a_symlink_escaping_the_bundle(tools_sandbox, bundle_factory, tmp_path):
    bundle = bundle_factory("escaping-link")
    target = tmp_path / "outside.txt"
    target.write_text("external file\n")
    (bundle / "outside-link").symlink_to("../outside.txt")
    write_manifest(bundle)

    result = tools_sandbox.run("verify", "--bundle", bundle, check=False)

    assert result.returncode != 0
    assert "symlink escapes root" in result.stderr
    assert target.read_text() == "external file\n"


def test_package_assembles_platform_runtime_assets_tool_and_reproducible_source(
    tools_sandbox, local_upstream, recording_cargo
):
    _, revision = local_upstream
    fixtures = {
        "pyproject.toml": '[project]\nname = "fixture-tests"\n',
        "uv.lock": "version = 1\n",
        "tests/containers/ssh/Dockerfile": "FROM fixture\n",
        "tests/lua/configuration.lua": "return {}\n",
        "tests/tools/test_example.py": "def test_example(): pass\n",
    }
    for name, contents in fixtures.items():
        write_file(tools_sandbox.root, name, contents)
    write_file(tools_sandbox.root, "tests/__pycache__/generated.pyc", "generated\n")
    signer = "B" * 40
    if sys.platform == "darwin":
        (recording_cargo.parent / "identities.txt").write_text(
            f'  1) {signer} "Developer ID Application: Fixture Person (TEAM123456)"\n'
        )

    result = tools_sandbox.json("--upstream", revision, "package", "--no-archive")

    bundle = Path(result["bundle"])
    binaries = binary_dir(bundle)
    names = (
        "wezterm-gui",
        "wezterm",
        "wezterm-mux-server",
        "strip-ansi-escapes",
        "wez-vtabs-store",
        "wez-vtabs",
    )
    for name in names:
        assert (binaries / executable_name(name)).is_file()
    resources = (
        bundle / "WezTerm.app/Contents/Resources" if sys.platform == "darwin" else bundle / "share"
    )
    marker_dir = resources if sys.platform == "darwin" else binaries
    marker = json.loads((marker_dir / "bundle.json").read_text())
    assert (marker_dir / marker["root"]).resolve() == bundle.resolve()
    assert bundle / marker["tool"] == binaries / executable_name("wez-vtabs")
    assert marker["mux_digest"] == json.loads((bundle / "build.json").read_text())["mux_digest"]
    assert marker["mux_digest"]
    assert (resources / "licenses/WezTerm-LICENSE.md").is_file()
    assert (resources / "plugin/init.lua").is_file()
    for name, contents in fixtures.items():
        assert (bundle / "source" / name).read_text() == contents
    assert not (bundle / "source/tests/__pycache__").exists()
    if sys.platform == "darwin":
        assert (resources / "wezterm.sh").is_file()
        assert (resources / "shell-completion/bash").is_file()
        calls = [
            json.loads(line)
            for line in (recording_cargo.parent / "codesign.jsonl").read_text().splitlines()
        ]
        signed = [call for call in calls if "--sign" in call]
        app = Path(calls[-1][-1])
        assert app.name == "WezTerm.app"
        assert [Path(call[-1]).relative_to(app) for call in signed] == [
            *(Path("Contents/MacOS") / name for name in sorted(names) if name != "wezterm-gui"),
            Path("."),
        ]
        assert all(call[call.index("--sign") + 1] == signer for call in signed)
        assert calls[-1] == ["--verify", "--deep", "--strict", str(app)]
    elif sys.platform.startswith("linux"):
        assert (bundle / "etc/profile.d/wezterm.sh").is_file()
        assert (bundle / "share/bash-completion/completions/wezterm").is_file()
        assert (bundle / "share/zsh/site-functions/_wezterm").is_file()
    tools_sandbox.run("verify", "--bundle", bundle)
