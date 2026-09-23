"""Exercise the public Lua entrypoint and its generated editor contract."""

import json
import shutil
import subprocess

import pytest


@pytest.mark.lua
def test_production_plugin_boundary(project_root, isolated_env):
    lua = shutil.which("lua")
    if lua is None:
        pytest.skip("install Lua to exercise the production plugin boundary")
    result = subprocess.run(
        [
            lua,
            str(project_root / "tests/lua/plugin_boundary.lua"),
            str(project_root / "plugin/init.lua"),
        ],
        env=isolated_env,
        capture_output=True,
        text=True,
        timeout=10,
        check=True,
    )
    assert result.stdout.strip() == "production Lua boundary passed"


CONSUMERS = {
    "options": "local tabs = require 'plugin.init'\n"
    "tabs.apply_to_config({}, {settings = {width = 280, side = 'left'}, settings_file = '/tmp/x.lua'})\n",
    "invalid-options": "local tabs = require 'plugin.init'\n"
    "tabs.apply_to_config({}, {settings = {width = 'wide', side = 'up'}})\n",
    "managed": "---@type TabsManaged\n"
    "return { settings = { width = 300 }, spaces = { { id = 'notes', name = 'Notes' } } }\n",
    "invalid-managed": "---@type TabsManaged\nreturn { settings = { side = 'up' } }\n",
    "action": "local tabs = require 'plugin.init'\n"
    "tabs.action('navigator')\ntabs.action({ ResetSetting = 'width' })\n",
    "invalid-action": "local tabs = require 'plugin.init'\ntabs.action('navigate')\n",
}


@pytest.mark.luals
@pytest.mark.parametrize("consumer", sorted(CONSUMERS))
def test_public_contracts_are_checked_by_luals(project_root, isolated_env, tmp_path, consumer):
    valid = not consumer.startswith("invalid")
    server = shutil.which("lua-language-server")
    if server is None:
        pytest.fail("--run-luals requires lua-language-server on PATH")
    workspace = tmp_path / "lua-workspace"
    workspace.mkdir()
    shutil.copytree(project_root / "plugin", workspace / "plugin")
    (workspace / "wezterm.lua").write_text(
        "---@type any\nlocal wezterm = {}\nreturn wezterm\n", encoding="utf-8"
    )
    example = workspace / "consumer.lua"
    example.write_text(CONSUMERS[consumer], encoding="utf-8")
    (workspace / ".luarc.json").write_text(
        json.dumps(
            {
                "runtime.version": "Lua 5.4",
                "workspace.checkThirdParty": False,
                "diagnostics.disable": ["unused-local", "unused-function"],
            }
        ),
        encoding="utf-8",
    )
    logs = tmp_path / "luals-logs"
    report = tmp_path / "diagnostics.json"
    result = subprocess.run(
        [
            server,
            f"--check={workspace}",
            "--checklevel=Warning",
            "--check_format=json",
            f"--check_out_path={report}",
            f"--logpath={logs}",
        ],
        env=isolated_env,
        capture_output=True,
        text=True,
        timeout=45,
    )
    diagnostics = json.loads(report.read_text(encoding="utf-8")) if report.exists() else {}
    if valid:
        assert result.returncode == 0, result.stdout + result.stderr + json.dumps(diagnostics)
        assert not diagnostics
    else:
        errors = [
            entry
            for uri, entries in diagnostics.items()
            if uri.endswith("consumer.lua")
            for entry in entries
        ]
        assert result.returncode != 0
        assert any(
            entry.get("code")
            in {"assign-type-mismatch", "param-type-mismatch", "return-type-mismatch"}
            for entry in errors
        ), diagnostics
