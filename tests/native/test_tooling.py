import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

PROJECT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("native", PROJECT / "scripts/native.py")
native = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(native)


class ToolingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="vtabs-tooling-")
        self.root = Path(self.temporary.name)

    def tearDown(self):
        self.temporary.cleanup()

    def bundle(self, name):
        root = self.root / name
        root.mkdir()
        native.write_json(root / "build.json", {"id": name, "capability": 1})
        for path in (
            native.gui_path(root),
            native.gui_path(root).parent / ("wez-vtabs-store.exe" if os.name == "nt" else "wez-vtabs-store"),
            root / "source/Cargo.toml",
            root / "source/scripts/native.py",
        ):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(name, encoding="utf-8")
        return root

    def test_install_stages_new_version_without_changing_executing_binary(self):
        root = self.root / "install"
        first = native.install(self.bundle("first"), root)
        first_bytes = native.gui_path(first).read_bytes()
        native.install(self.bundle("second"), root, stage_only=True)
        self.assertEqual(native.current_bundle(root, promote=False), first)
        self.assertEqual(native.gui_path(first).read_bytes(), first_bytes)
        self.assertEqual(native.current_bundle(root).name, "second")
        self.assertEqual(native.gui_path(first).read_bytes(), first_bytes)
        self.assertFalse((root / "pending.json").exists())

    def test_pending_bundle_must_be_complete(self):
        root = self.root / "install"
        first = native.install(self.bundle("first"), root)
        native.write_json(root / "pending.json", {"id": "missing"})
        with self.assertRaisesRegex(RuntimeError, "incomplete"):
            native.current_bundle(root)
        self.assertEqual(native.current_bundle(root, promote=False), first)

    def test_install_rejects_incomplete_bundle_and_unsafe_identifiers(self):
        bundle = self.bundle("incomplete")
        native.gui_path(bundle).unlink()
        with self.assertRaisesRegex(RuntimeError, "incomplete"):
            native.install(bundle, self.root / "install")
        for identifier in ("../outside", "a/b", "/tmp", "", "a\\b"):
            with self.assertRaises(RuntimeError):
                native.safe_id(identifier)

    def test_prepare_checks_each_patch_in_order_and_never_guesses_repairs(self):
        project = self.root / "project"
        patches = project / "native/patches"
        patches.mkdir(parents=True)
        for name in ("0002-ui.patch", "0001-layout.patch"):
            (patches / name).write_text("patch", encoding="utf-8")
        cache = self.root / "cache"
        cache.mkdir()
        calls = []
        def execute(*args, **kwargs):
            calls.append(tuple(str(arg) for arg in args))
            if args[:3] == ("git", "apply", "--check") and "0002" in str(args[3]):
                raise subprocess.CalledProcessError(1, args)
            return ""
        with patch.object(native, "ROOT", project), patch.object(native, "run", side_effect=execute), patch.object(native, "stage_adapter") as stage:
            with self.assertRaises(subprocess.CalledProcessError):
                native.prepare(cache, cache / "upstream", "current-main")
            stage.assert_not_called()
        apply = [args for args in calls if args[:2] == ("git", "apply")]
        self.assertEqual([args[2] for args in apply], ["--check", str(patches / "0001-layout.patch"), "--check"])
        self.assertFalse((cache / "prepared.json").exists())

    def test_stage_adapter_wires_only_project_dependencies(self):
        project = self.root / "project"
        adapter = project / "native/adapter"
        adapter.mkdir(parents=True)
        (adapter / "mod.rs").write_text("mod storage;", encoding="utf-8")
        (adapter / "storage.rs").write_text("storage", encoding="utf-8")
        gui = self.root / "checkout/wezterm-gui"
        (gui / "src").mkdir(parents=True)
        (gui / "Cargo.toml").write_text('[package]\nname = "wezterm-gui"\n[dependencies]\ntermwiz.workspace = true\n', encoding="utf-8")
        with patch.object(native, "ROOT", project):
            native.stage_adapter(gui.parent)
        manifest = (gui / "Cargo.toml").read_text(encoding="utf-8")
        self.assertIn("default-features = false", manifest)
        self.assertIn("vtabs-store", manifest)
        self.assertIn("termwiz.workspace = true", manifest)
        self.assertEqual(manifest.count("termwiz"), 1)
        self.assertEqual((gui / "src/native_vtabs/storage.rs").read_text(encoding="utf-8"), "storage")

    def test_daily_update_is_skipped_after_recent_attempt(self):
        install = self.root / "install"
        native.write_json(install / "update.json", {"last_attempt": native.time.time(), "status": "failed"})
        with patch.object(native, "build") as build:
            native.update(self.root / "cache", install, True, True, self.root / "dist")
            build.assert_not_called()

    def test_installed_update_fetches_project_in_cache_and_uses_its_updater(self):
        source = self.root / "versions/current/source"
        source.mkdir(parents=True)
        native.write_json(source.parent / "build.json", {"capability": 1})
        project = self.root / "cache/project"
        with patch.object(native, "ROOT", source), patch.object(native, "refresh_project", return_value=project) as refresh, patch.object(native, "run") as run, patch.object(native, "build") as build, patch.dict(os.environ, {"WEZ_VTABS_UPDATE_SOURCE_SYNCED": "0"}):
            native.update(self.root / "cache", self.root / "install", True, True, self.root / "bundles")
        refresh.assert_called_once_with(self.root / "cache", "native")
        build.assert_not_called()
        self.assertIn(str(project / "scripts/native.py"), run.call_args.args)
        self.assertEqual(run.call_args.kwargs["env"]["WEZ_VTABS_UPDATE_SOURCE_SYNCED"], "1")
        self.assertEqual(run.call_args.kwargs["env"]["WEZ_VTABS_PROJECT_BRANCH"], "native")
        self.assertTrue((source.parent / "build.json").is_file())

    def test_project_refresh_refuses_an_existing_unowned_checkout(self):
        cache = self.root / "cache"
        project = cache / "project"
        project.mkdir(parents=True)
        sentinel = project / "uncommitted.rs"
        sentinel.write_text("user edits", encoding="utf-8")
        with patch.object(native, "run") as run:
            with self.assertRaisesRegex(RuntimeError, "unowned"):
                native.refresh_project(cache)
            run.assert_not_called()
        self.assertEqual(sentinel.read_text(encoding="utf-8"), "user edits")

    def test_installed_update_preserves_the_recorded_project_branch(self):
        source = self.root / "versions/current/source"
        source.mkdir(parents=True)
        native.write_json(source.parent / "build.json", {
            "capability": 1,
            "project_source": {"remote": native.PROJECT_URL, "branch": "release/native", "revision": "a" * 40},
        })
        project = self.root / "cache/project"
        with patch.object(native, "ROOT", source), patch.object(native, "refresh_project", return_value=project) as refresh, patch.object(native, "run") as run, patch.dict(os.environ, {"WEZ_VTABS_UPDATE_SOURCE_SYNCED": "0", "WEZ_VTABS_PROJECT_BRANCH": ""}):
            native.update(self.root / "cache", self.root / "install", False, True, self.root / "bundles")
        refresh.assert_called_once_with(self.root / "cache", "release/native")
        self.assertEqual(run.call_args.kwargs["env"]["WEZ_VTABS_PROJECT_BRANCH"], "release/native")

    def test_project_source_records_explicit_branch_and_current_revision(self):
        project = self.root / "project"
        (project / ".git").mkdir(parents=True)
        with patch.object(native, "ROOT", project), patch.object(native, "run", return_value="b" * 40), patch.dict(os.environ, {"WEZ_VTABS_PROJECT_BRANCH": "release/native"}):
            self.assertEqual(native.project_source(), {"remote": native.PROJECT_URL, "branch": "release/native", "revision": "b" * 40})

    def test_project_branch_rejects_revision_expressions_and_unsafe_refs(self):
        for branch in ("", "--upload-pack=bad", "../main", "native^{commit}", "a..b", "a.lock", "a//b", "a/.b", "a/", "a.", "a b", "a\\b"):
            with self.subTest(branch=branch), patch.object(native, "run") as run:
                with self.assertRaisesRegex(RuntimeError, "invalid project update branch"):
                    native.refresh_project(self.root / "cache", branch)
                run.assert_not_called()

    def test_project_refresh_switches_an_owned_legacy_cache_with_an_explicit_refspec(self):
        cache = self.root / "cache"
        project = cache / "project"
        (project / ".git").mkdir(parents=True)
        native.write_json(project / ".git/wez-vtabs-native.json", {
            "path": str(project.resolve()), "remote": native.PROJECT_URL, "capability": native.CAPABILITY,
        })
        for name in ("Cargo.toml", "crates/vtabs-app/Cargo.toml", "scripts/native.py", "native/patches/0001-native.patch"):
            path = project / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("source", encoding="utf-8")
        with patch.object(native, "run", return_value=native.PROJECT_URL) as run:
            self.assertEqual(native.refresh_project(cache, "release/native"), project)
        run.assert_any_call("git", "fetch", "--prune", "origin", "+refs/heads/release/native:refs/remotes/origin/release/native", cwd=project)
        run.assert_any_call("git", "reset", "--hard", "refs/remotes/origin/release/native", cwd=project)
        self.assertFalse(any("origin/main" in str(call) for call in run.call_args_list))

    def test_project_source_rejects_an_unexpected_recorded_remote(self):
        source = self.root / "source"
        source.mkdir()
        native.write_json(source.parent / "build.json", {
            "project_source": {"remote": "https://example.com/project.git", "branch": "native"},
        })
        with patch.object(native, "ROOT", source):
            with self.assertRaisesRegex(RuntimeError, "unexpected recorded project source"):
                native.project_source()

    def test_legacy_single_branch_cache_fetches_native_source_from_real_git_repository(self):
        repository = self.root / "upstream"
        repository.mkdir()

        def git(*arguments, cwd=repository):
            return subprocess.check_output(["git", *arguments], cwd=cwd, text=True, stderr=subprocess.STDOUT)

        git("init", "--initial-branch=main")
        (repository / "README.md").write_text("Legacy source", encoding="utf-8")
        git("add", ".")
        git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "Legacy source")
        git("switch", "-c", "native")
        for name in ("Cargo.toml", "crates/vtabs-app/Cargo.toml", "scripts/native.py", "native/patches/0001-native.patch"):
            path = repository / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("Native source", encoding="utf-8")
        git("add", ".")
        git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "Native source")
        expected = git("rev-parse", "HEAD").strip()
        cache = self.root / "cache"
        cache.mkdir()
        project = cache / "project"
        git("clone", "--single-branch", "--branch", "main", str(repository), str(project))
        remote = str(repository)
        native.write_json(project / ".git/wez-vtabs-native.json", {
            "path": str(project.resolve()), "remote": remote, "capability": native.CAPABILITY,
        })
        with patch.object(native, "PROJECT_URL", remote):
            self.assertEqual(native.refresh_project(cache), project)
        self.assertEqual(git("rev-parse", "HEAD", cwd=project).strip(), expected)
        self.assertTrue((project / "crates/vtabs-app/Cargo.toml").is_file())

    def test_source_digest_ignores_build_outputs_and_tracks_source(self):
        root = self.root / "source"
        (root / "crates/example/src").mkdir(parents=True)
        source = root / "crates/example/src/lib.rs"
        source.write_text("fn a() {}", encoding="utf-8")
        before = native.source_digest(root)
        (root / "crates/example/target").mkdir()
        (root / "crates/example/target/build").write_text("artifact", encoding="utf-8")
        self.assertEqual(native.source_digest(root), before)
        source.write_text("fn b() {}", encoding="utf-8")
        self.assertNotEqual(native.source_digest(root), before)

    def test_changed_sources_cannot_reuse_build_metadata(self):
        root = self.root / "source"
        root.mkdir()
        source = root / "Cargo.toml"
        source.write_text("original source", encoding="utf-8")
        metadata = {"source_digest": native.source_digest(root)}
        with patch.object(native, "ROOT", root):
            native.verify_source(metadata)
            source.write_text("edited during compilation", encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "source changed"):
                native.verify_source(metadata)

    def test_packaging_existing_bundle_recreates_archive_after_interrupted_publication(self):
        bundle = self.bundle("wez-vtabs-native-first")
        metadata = {"id": "first", "capability": 1, "source_digest": native.source_digest(native.ROOT)}
        native.write_json(bundle / "build.json", metadata)
        suffix = ".zip" if os.name == "nt" or native.sys.platform == "darwin" else ".tar.gz"
        archive = bundle.with_name(bundle.name + suffix)
        original = native.gui_path(bundle).read_bytes()
        with patch.object(native.os, "replace", side_effect=OSError("publication interrupted")):
            with self.assertRaisesRegex(OSError, "publication interrupted"):
                native.package(self.root / "cache", metadata, self.root)
        self.assertFalse(archive.exists())
        self.assertFalse(list(self.root.glob(".archive-*")))
        self.assertEqual(native.package(self.root / "cache", metadata, self.root), bundle)
        self.assertTrue(archive.is_file())
        archive.unlink()
        native.package(self.root / "cache", metadata, self.root)
        self.assertTrue(archive.is_file())
        self.assertEqual(native.gui_path(bundle).read_bytes(), original)

    def test_shell_assets_follow_upstream_platform_locations(self):
        source = self.root / "upstream"
        for name in ("shell-integration/wezterm.sh", "shell-completion/bash", "shell-completion/zsh"):
            path = source / "assets" / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(name, encoding="utf-8")
        for system, expected in (
            ("darwin", ("resources/wezterm.sh", "resources/shell-completion/bash")),
            ("linux", ("etc/profile.d/wezterm.sh", "share/bash-completion/completions/wezterm", "share/zsh/site-functions/_wezterm")),
            ("win32", ("resources/shell-integration/wezterm.sh", "resources/shell-completion/bash")),
        ):
            bundle = self.root / system
            resources = bundle / "resources"
            resources.mkdir(parents=True)
            with patch.object(native.sys, "platform", system):
                native.shell_assets(source, bundle, resources)
            for path in expected:
                self.assertTrue((bundle / path).is_file(), path)

    def test_windows_runtime_assets_follow_binary_test_and_bundle_destinations(self):
        source = self.root / "checkout"
        assets = source / "assets/windows"
        for directory, name in (*native.WINDOWS_RUNTIME, ("mesa", "opengl32.dll")):
            path = assets / directory / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"current runtime")
        binaries = self.root / "external-target/x86_64-pc-windows-msvc/release"
        tests = binaries / "deps"
        bundle = self.root / "bundle"
        tests.mkdir(parents=True)
        (tests / "conpty.dll").write_bytes(b"old runtime")
        (binaries / "wezterm-gui.exe").write_bytes(b"built application")
        native.copy_windows_runtime(source, (binaries, tests, bundle))
        for destination in (binaries, tests, bundle):
            for _, name in native.WINDOWS_RUNTIME:
                self.assertEqual((destination / name).read_bytes(), b"current runtime")
            self.assertEqual((destination / "mesa/opengl32.dll").read_bytes(), b"current runtime")
        for path in assets.rglob("*"):
            if path.is_file():
                path.write_bytes(b"updated runtime")
        native.copy_windows_runtime(source, (binaries, tests, bundle))
        for destination in (binaries, tests, bundle):
            for _, name in native.WINDOWS_RUNTIME:
                self.assertEqual((destination / name).read_bytes(), b"updated runtime")
            self.assertEqual((destination / "mesa/opengl32.dll").read_bytes(), b"updated runtime")
        self.assertEqual((binaries / "wezterm-gui.exe").read_bytes(), b"built application")
        self.assertFalse((source / "target").exists())

    @unittest.skipIf(os.name == "nt", "POSIX launcher execution")
    def test_managed_launcher_preserves_paths_and_arguments(self):
        root = self.root / "install with 'quotes' and $symbols"
        root.mkdir()
        (root / "native.py").write_text("import json, sys; print(json.dumps(sys.argv[1:]))", encoding="utf-8")
        bundle = self.bundle("release")
        native.install_entry(root, bundle)
        output = subprocess.check_output([str(root / "wez-vtabs"), "--literal", "a b $()"], text=True)
        self.assertEqual(json.loads(output), ["launch", "--", "--literal", "a b $()"])

    def test_all_managed_entry_formats_use_the_stable_launcher(self):
        bundle = self.bundle("release")
        for system, filename in (("darwin", "WezTerm Native.app/Contents/MacOS/launch"), ("linux", "wez-vtabs.desktop"), ("win32", "wez-vtabs.cmd")):
            root = self.root / system
            root.mkdir()
            with patch.object(native.sys, "platform", system):
                native.install_entry(root, bundle)
            text = (root / filename).read_text(encoding="utf-8")
            self.assertIn("native.py", text)
        self.assertEqual(native.desktop_quote('a\\b$"%'), '"a\\\\\\\\b\\\\$\\\\"%%"')

    @unittest.skipIf(os.name == "nt", "POSIX fixture executable")
    def test_custom_install_launcher_remembers_root_without_environment(self):
        bundle = self.bundle("custom")
        executable = native.gui_path(bundle)
        executable.write_text("#!/bin/sh\nexit 37\n", encoding="utf-8")
        executable.chmod(0o755)
        root = self.root / "custom install"
        (root.parent / "Cargo.toml").write_text("parent Rust project", encoding="utf-8")
        native.install(bundle, root)
        self.assertEqual(native.read_json(root / "runtime.json"), {"python": str(Path(native.sys.executable).resolve())})
        native.write_json(root / "update.json", {"last_attempt": native.time.time()})
        environment = {**os.environ, "XDG_DATA_HOME": str(self.root / "unrelated default")}
        environment.pop("WEZ_VTABS_INSTALL", None)
        result = subprocess.run([native.sys.executable, str(root / "native.py"), "launch"], env=environment)
        self.assertEqual(result.returncode, 37)


if __name__ == "__main__":
    unittest.main()
