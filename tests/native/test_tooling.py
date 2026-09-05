import json
import os
import subprocess
from pathlib import Path
from unittest.mock import patch

import pytest

from scripts import native


@pytest.fixture
def bundle(tmp_path):
    def create(name):
        root = tmp_path / name
        root.mkdir()
        native.write_json(root / "build.json", {"id": name, "capability": 1})
        for path in (
            native.gui_path(root),
            native.gui_path(root).parent
            / ("wez-vtabs-store.exe" if os.name == "nt" else "wez-vtabs-store"),
            root / "source/Cargo.toml",
            root / "source/scripts/native.py",
        ):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(name, encoding="utf-8")
        return root

    return create


def test_install_stages_new_version_without_changing_executing_binary(tmp_path, bundle):
    root = tmp_path / "install"
    first = native.install(bundle("first"), root)
    first_bytes = native.gui_path(first).read_bytes()
    native.install(bundle("second"), root, stage_only=True)
    assert native.current_bundle(root, promote=False) == first
    assert native.gui_path(first).read_bytes() == first_bytes
    assert native.current_bundle(root).name == "second"
    assert native.gui_path(first).read_bytes() == first_bytes
    assert not (root / "pending.json").exists()


def test_pending_bundle_must_be_complete(tmp_path, bundle):
    root = tmp_path / "install"
    first = native.install(bundle("first"), root)
    native.write_json(root / "pending.json", {"id": "missing"})
    with pytest.raises(RuntimeError, match="incomplete"):
        native.current_bundle(root)
    assert native.current_bundle(root, promote=False) == first


def test_install_rejects_incomplete_bundle_and_unsafe_identifiers(tmp_path, bundle):
    bundle = bundle("incomplete")
    native.gui_path(bundle).unlink()
    with pytest.raises(RuntimeError, match="incomplete"):
        native.install(bundle, tmp_path / "install")
    for identifier in ("../outside", "a/b", "/tmp", "", "a\\b"):
        with pytest.raises(RuntimeError):
            native.safe_id(identifier)


def test_prepare_checks_each_patch_in_order_and_never_guesses_repairs(tmp_path):
    project = tmp_path / "project"
    patches = project / "native/patches"
    patches.mkdir(parents=True)
    for name in ("0002-ui.patch", "0001-layout.patch"):
        (patches / name).write_text("patch", encoding="utf-8")
    cache = tmp_path / "cache"
    cache.mkdir()
    calls = []

    def execute(*args, **kwargs):
        calls.append(tuple(str(arg) for arg in args))
        if args[:3] == ("git", "apply", "--check") and "0002" in str(args[3]):
            raise subprocess.CalledProcessError(1, args)
        return ""

    with (
        patch.object(native, "ROOT", project),
        patch.object(native, "run", side_effect=execute),
        patch.object(native, "stage_adapter") as stage,
    ):
        with pytest.raises(subprocess.CalledProcessError):
            native.prepare(cache, cache / "upstream", "current-main")
        stage.assert_not_called()
    apply = [args for args in calls if args[:2] == ("git", "apply")]
    assert [args[2] for args in apply] == [
        "--check",
        str(patches / "0001-layout.patch"),
        "--check",
    ]
    assert not (cache / "prepared.json").exists()


def test_stage_adapter_wires_only_project_dependencies(tmp_path):
    project = tmp_path / "project"
    adapter = project / "native/adapter"
    adapter.mkdir(parents=True)
    (adapter / "mod.rs").write_text("mod storage;", encoding="utf-8")
    (adapter / "storage.rs").write_text("storage", encoding="utf-8")
    gui = tmp_path / "checkout/wezterm-gui"
    (gui / "src").mkdir(parents=True)
    (gui / "Cargo.toml").write_text(
        '[package]\nname = "wezterm-gui"\n[dependencies]\ntermwiz.workspace = true\n',
        encoding="utf-8",
    )
    with patch.object(native, "ROOT", project):
        native.stage_adapter(gui.parent)
    manifest = (gui / "Cargo.toml").read_text(encoding="utf-8")
    assert "default-features = false" in manifest
    assert "vtabs-store" in manifest
    assert "termwiz.workspace = true" in manifest
    assert manifest.count("termwiz") == 1
    assert (gui / "src/native_vtabs/storage.rs").read_text(encoding="utf-8") == "storage"


def test_daily_update_is_skipped_after_recent_attempt(tmp_path):
    install = tmp_path / "install"
    native.write_json(
        install / "update.json",
        {"last_attempt": native.time.time(), "status": "failed"},
    )
    with patch.object(native, "build") as build:
        native.update(tmp_path / "cache", install, True, True, tmp_path / "dist")
        build.assert_not_called()


def test_installed_update_fetches_project_in_cache_and_uses_its_updater(tmp_path):
    source = tmp_path / "versions/current/source"
    source.mkdir(parents=True)
    native.write_json(source.parent / "build.json", {"capability": 1})
    project = tmp_path / "cache/project"
    with (
        patch.object(native, "ROOT", source),
        patch.object(native, "refresh_project", return_value=project) as refresh,
        patch.object(native, "run") as run,
        patch.object(native, "build") as build,
        patch.dict(os.environ, {"WEZ_VTABS_UPDATE_SOURCE_SYNCED": "0"}),
    ):
        native.update(tmp_path / "cache", tmp_path / "install", True, True, tmp_path / "bundles")
    refresh.assert_called_once_with(tmp_path / "cache", "native")
    build.assert_not_called()
    assert str(project / "scripts/native.py") in run.call_args.args
    assert run.call_args.kwargs["env"]["WEZ_VTABS_UPDATE_SOURCE_SYNCED"] == "1"
    assert run.call_args.kwargs["env"]["WEZ_VTABS_PROJECT_BRANCH"] == "native"
    assert (source.parent / "build.json").is_file()


def test_project_refresh_refuses_an_existing_unowned_checkout(tmp_path):
    cache = tmp_path / "cache"
    project = cache / "project"
    project.mkdir(parents=True)
    sentinel = project / "uncommitted.rs"
    sentinel.write_text("user edits", encoding="utf-8")
    with patch.object(native, "run") as run:
        with pytest.raises(RuntimeError, match="unowned"):
            native.refresh_project(cache)
        run.assert_not_called()
    assert sentinel.read_text(encoding="utf-8") == "user edits"


def test_installed_update_preserves_the_recorded_project_branch(tmp_path):
    source = tmp_path / "versions/current/source"
    source.mkdir(parents=True)
    native.write_json(
        source.parent / "build.json",
        {
            "capability": 1,
            "project_source": {
                "remote": native.PROJECT_URL,
                "branch": "release/native",
                "revision": "a" * 40,
            },
        },
    )
    project = tmp_path / "cache/project"
    with (
        patch.object(native, "ROOT", source),
        patch.object(native, "refresh_project", return_value=project) as refresh,
        patch.object(native, "run") as run,
        patch.dict(
            os.environ,
            {"WEZ_VTABS_UPDATE_SOURCE_SYNCED": "0", "WEZ_VTABS_PROJECT_BRANCH": ""},
        ),
    ):
        native.update(tmp_path / "cache", tmp_path / "install", False, True, tmp_path / "bundles")
    refresh.assert_called_once_with(tmp_path / "cache", "release/native")
    assert run.call_args.kwargs["env"]["WEZ_VTABS_PROJECT_BRANCH"] == "release/native"


def test_project_source_records_explicit_branch_and_current_revision(tmp_path):
    project = tmp_path / "project"
    (project / ".git").mkdir(parents=True)
    with (
        patch.object(native, "ROOT", project),
        patch.object(native, "run", return_value="b" * 40),
        patch.dict(os.environ, {"WEZ_VTABS_PROJECT_BRANCH": "release/native"}),
    ):
        assert native.project_source() == {
            "remote": native.PROJECT_URL,
            "branch": "release/native",
            "revision": "b" * 40,
        }


def test_project_branch_rejects_revision_expressions_and_unsafe_refs(tmp_path):
    for branch in (
        "",
        "--upload-pack=bad",
        "../main",
        "native^{commit}",
        "a..b",
        "a.lock",
        "a//b",
        "a/.b",
        "a/",
        "a.",
        "a b",
        "a\\b",
    ):
        with patch.object(native, "run") as run:
            with pytest.raises(RuntimeError, match="invalid project update branch"):
                native.refresh_project(tmp_path / "cache", branch)
            run.assert_not_called()


def test_project_refresh_switches_an_owned_legacy_cache_with_an_explicit_refspec(
    tmp_path,
):
    cache = tmp_path / "cache"
    project = cache / "project"
    (project / ".git").mkdir(parents=True)
    native.write_json(
        project / ".git/wez-vtabs-native.json",
        {
            "path": str(project.resolve()),
            "remote": native.PROJECT_URL,
            "capability": native.CAPABILITY,
        },
    )
    for name in (
        "Cargo.toml",
        "crates/vtabs-app/Cargo.toml",
        "scripts/native.py",
        "native/patches/0001-native.patch",
    ):
        path = project / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("source", encoding="utf-8")
    with patch.object(native, "run", return_value=native.PROJECT_URL) as run:
        assert native.refresh_project(cache, "release/native") == project
    run.assert_any_call(
        "git",
        "fetch",
        "--prune",
        "origin",
        "+refs/heads/release/native:refs/remotes/origin/release/native",
        cwd=project,
    )
    run.assert_any_call("git", "reset", "--hard", "refs/remotes/origin/release/native", cwd=project)
    assert not any("origin/main" in str(call) for call in run.call_args_list)


def test_project_source_rejects_an_unexpected_recorded_remote(tmp_path):
    source = tmp_path / "source"
    source.mkdir()
    native.write_json(
        source.parent / "build.json",
        {
            "project_source": {
                "remote": "https://example.com/project.git",
                "branch": "native",
            }
        },
    )
    with patch.object(native, "ROOT", source):
        with pytest.raises(RuntimeError, match="unexpected recorded project source"):
            native.project_source()


def test_legacy_single_branch_cache_fetches_native_source_from_real_git_repository(
    tmp_path,
):
    repository = tmp_path / "upstream"
    repository.mkdir()

    def git(*arguments, cwd=repository):
        return subprocess.check_output(
            ["git", *arguments], cwd=cwd, text=True, stderr=subprocess.STDOUT
        )

    git("init", "--initial-branch=main")
    (repository / "README.md").write_text("Legacy source", encoding="utf-8")
    git("add", ".")
    git(
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "-m",
        "Legacy source",
    )
    git("switch", "-c", "native")
    for name in (
        "Cargo.toml",
        "crates/vtabs-app/Cargo.toml",
        "scripts/native.py",
        "native/patches/0001-native.patch",
    ):
        path = repository / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("Native source", encoding="utf-8")
    git("add", ".")
    git(
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "-m",
        "Native source",
    )
    expected = git("rev-parse", "HEAD").strip()
    cache = tmp_path / "cache"
    cache.mkdir()
    project = cache / "project"
    git("clone", "--single-branch", "--branch", "main", str(repository), str(project))
    remote = str(repository)
    native.write_json(
        project / ".git/wez-vtabs-native.json",
        {
            "path": str(project.resolve()),
            "remote": remote,
            "capability": native.CAPABILITY,
        },
    )
    with patch.object(native, "PROJECT_URL", remote):
        assert native.refresh_project(cache) == project
    assert git("rev-parse", "HEAD", cwd=project).strip() == expected
    assert (project / "crates/vtabs-app/Cargo.toml").is_file()


def test_source_digest_ignores_build_outputs_and_tracks_source(tmp_path):
    root = tmp_path / "source"
    (root / "crates/example/src").mkdir(parents=True)
    source = root / "crates/example/src/lib.rs"
    source.write_text("fn a() {}", encoding="utf-8")
    before = native.source_digest(root)
    (root / "crates/example/target").mkdir()
    (root / "crates/example/target/build").write_text("artifact", encoding="utf-8")
    assert native.source_digest(root) == before
    source.write_text("fn b() {}", encoding="utf-8")
    assert native.source_digest(root) != before


def test_source_bundle_preserves_test_dependencies_and_fixtures_without_runtime_outputs(
    tmp_path,
):
    root = tmp_path / "project"
    sources = {
        "pyproject.toml": '[project]\nname = "test-suite"\n',
        "uv.lock": "version = 1\n",
        "ruff.toml": 'line-length = 100\n[format]\nquote-style = "double"\n',
        "rustfmt.toml": "max_width = 100\n",
        "stylua.toml": "column_width = 100\n",
        ".editorconfig": "root = true\n[*]\nend_of_line = lf\n",
        "tests/conftest.py": "import pytest\n",
        "tests/containers/sshd/Containerfile": "FROM test-image\n",
        "tests/lua/configuration.lua": "return {}\n",
    }
    for name, content in sources.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
    generated = root / "tests/__pycache__/conftest.pyc"
    generated.parent.mkdir()
    generated.write_bytes(b"generated bytecode")
    before = native.source_digest(root)
    destination = tmp_path / "bundle/source"
    with patch.object(native, "ROOT", root):
        native.copy_source(destination)
    for name, content in sources.items():
        assert (destination / name).read_text(encoding="utf-8") == content
    assert not (destination / "tests/__pycache__").exists()
    (root / "uv.lock").write_text("version = 2\n", encoding="utf-8")
    assert native.source_digest(root) != before


def test_changed_sources_cannot_reuse_build_metadata(tmp_path):
    root = tmp_path / "source"
    root.mkdir()
    source = root / "Cargo.toml"
    source.write_text("original source", encoding="utf-8")
    metadata = {"source_digest": native.source_digest(root)}
    with patch.object(native, "ROOT", root):
        native.verify_source(metadata)
        source.write_text("edited during compilation", encoding="utf-8")
        with pytest.raises(RuntimeError, match="source changed"):
            native.verify_source(metadata)


def test_packaging_existing_bundle_recreates_archive_after_interrupted_publication(
    tmp_path, bundle
):
    bundle = bundle("wez-vtabs-native-first")
    metadata = {
        "id": "first",
        "capability": 1,
        "source_digest": native.source_digest(native.ROOT),
    }
    native.write_json(bundle / "build.json", metadata)
    suffix = ".zip" if os.name == "nt" or native.sys.platform == "darwin" else ".tar.gz"
    archive = bundle.with_name(bundle.name + suffix)
    original = native.gui_path(bundle).read_bytes()
    with patch.object(native.os, "replace", side_effect=OSError("publication interrupted")):
        with pytest.raises(OSError, match="publication interrupted"):
            native.package(tmp_path / "cache", metadata, tmp_path)
    assert not archive.exists()
    assert not list(tmp_path.glob(".archive-*"))
    assert native.package(tmp_path / "cache", metadata, tmp_path) == bundle
    assert archive.is_file()
    archive.unlink()
    native.package(tmp_path / "cache", metadata, tmp_path)
    assert archive.is_file()
    assert native.gui_path(bundle).read_bytes() == original


def test_shell_assets_follow_upstream_platform_locations(tmp_path, bundle):
    source = tmp_path / "upstream"
    for name in (
        "shell-integration/wezterm.sh",
        "shell-completion/bash",
        "shell-completion/zsh",
    ):
        path = source / "assets" / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(name, encoding="utf-8")
    for system, expected in (
        ("darwin", ("resources/wezterm.sh", "resources/shell-completion/bash")),
        (
            "linux",
            (
                "etc/profile.d/wezterm.sh",
                "share/bash-completion/completions/wezterm",
                "share/zsh/site-functions/_wezterm",
            ),
        ),
        (
            "win32",
            (
                "resources/shell-integration/wezterm.sh",
                "resources/shell-completion/bash",
            ),
        ),
    ):
        bundle = tmp_path / system
        resources = bundle / "resources"
        resources.mkdir(parents=True)
        with patch.object(native.sys, "platform", system):
            native.shell_assets(source, bundle, resources)
        for path in expected:
            assert (bundle / path).is_file(), path


def test_windows_runtime_assets_follow_binary_test_and_bundle_destinations(tmp_path, bundle):
    source = tmp_path / "checkout"
    assets = source / "assets/windows"
    for directory, name in (*native.WINDOWS_RUNTIME, ("mesa", "opengl32.dll")):
        path = assets / directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"current runtime")
    binaries = tmp_path / "external-target/x86_64-pc-windows-msvc/release"
    tests = binaries / "deps"
    bundle = tmp_path / "bundle"
    tests.mkdir(parents=True)
    (tests / "conpty.dll").write_bytes(b"old runtime")
    (binaries / "wezterm-gui.exe").write_bytes(b"built application")
    native.copy_windows_runtime(source, (binaries, tests, bundle))
    for destination in (binaries, tests, bundle):
        for _, name in native.WINDOWS_RUNTIME:
            assert (destination / name).read_bytes() == b"current runtime"
        assert (destination / "mesa/opengl32.dll").read_bytes() == b"current runtime"
    for path in assets.rglob("*"):
        if path.is_file():
            path.write_bytes(b"updated runtime")
    native.copy_windows_runtime(source, (binaries, tests, bundle))
    for destination in (binaries, tests, bundle):
        for _, name in native.WINDOWS_RUNTIME:
            assert (destination / name).read_bytes() == b"updated runtime"
        assert (destination / "mesa/opengl32.dll").read_bytes() == b"updated runtime"
    assert (binaries / "wezterm-gui.exe").read_bytes() == b"built application"
    assert not (source / "target").exists()


@pytest.mark.skipif(os.name == "nt", reason="POSIX launcher execution")
def test_managed_launcher_preserves_paths_and_arguments(tmp_path, bundle):
    root = tmp_path / "install with 'quotes' and $symbols"
    root.mkdir()
    (root / "native.py").write_text(
        "import json, sys; print(json.dumps(sys.argv[1:]))", encoding="utf-8"
    )
    bundle = bundle("release")
    native.install_entry(root, bundle)
    output = subprocess.check_output([str(root / "wez-vtabs"), "--literal", "a b $()"], text=True)
    assert json.loads(output) == ["launch", "--", "--literal", "a b $()"]


def test_all_managed_entry_formats_use_the_stable_launcher(tmp_path, bundle):
    bundle = bundle("release")
    for system, filename in (
        ("darwin", "WezTerm Native.app/Contents/MacOS/launch"),
        ("linux", "wez-vtabs.desktop"),
        ("win32", "wez-vtabs.cmd"),
    ):
        root = tmp_path / system
        root.mkdir()
        with patch.object(native.sys, "platform", system):
            native.install_entry(root, bundle)
        text = (root / filename).read_text(encoding="utf-8")
        assert "native.py" in text
    assert native.desktop_quote('a\\b$"%') == '"a\\\\\\\\b\\\\$\\\\"%%"'


@pytest.mark.skipif(os.name == "nt", reason="POSIX fixture executable")
def test_custom_install_launcher_remembers_root_without_environment(tmp_path, bundle):
    bundle = bundle("custom")
    executable = native.gui_path(bundle)
    executable.write_text("#!/bin/sh\nexit 37\n", encoding="utf-8")
    executable.chmod(493)
    root = tmp_path / "custom install"
    (root.parent / "Cargo.toml").write_text("parent Rust project", encoding="utf-8")
    native.install(bundle, root)
    assert native.read_json(root / "runtime.json") == {
        "python": str(Path(native.sys.executable).resolve())
    }
    native.write_json(root / "update.json", {"last_attempt": native.time.time()})
    environment = {**os.environ, "XDG_DATA_HOME": str(tmp_path / "unrelated default")}
    environment.pop("WEZ_VTABS_INSTALL", None)
    result = subprocess.run(
        [native.sys.executable, str(root / "native.py"), "launch"], env=environment
    )
    assert result.returncode == 37
