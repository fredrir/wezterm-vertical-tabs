"""Cache cleanup preserves active versions and unowned files."""

import json
from pathlib import Path

import pytest
from filelock import FileLock

from tests.tools.conftest import write_file

pytestmark = pytest.mark.rust


def test_cache_gc_dry_run_preserves_active_pending_leased_and_unowned_paths(
    tools_sandbox, bundle_factory
):
    bundles = tools_sandbox.cache / "bundles"
    bundles.mkdir(parents=True)
    paths = {}
    for name in ("active", "pending", "running", "expired"):
        paths[name] = bundle_factory(name, directory=str(bundles / f"wez-vtabs-{name}"))
    tools_sandbox.install.mkdir()
    for name in ("active", "pending"):
        write_file(tools_sandbox.install, f"{name}.json", json.dumps({"id": name}))
    sentinel = write_file(bundles, "unowned/user-edits.rs", "user source\n")
    archives = [
        paths["expired"].with_name(paths["expired"].name + suffix)
        for suffix in (".zip", ".zip.manifest.json", ".tar.gz", ".tar.gz.manifest.json")
    ]
    for path in archives:
        path.write_text("archive fixture\n")
    lease = tools_sandbox.cache / "leases/wez-vtabs-running.lock"
    lease.parent.mkdir()
    with FileLock(lease):
        preview = tools_sandbox.json("cache", "gc", "--dry-run", "--keep", "0")
        entries = {Path(entry["path"]): entry for entry in preview["entries"]}
        assert entries[paths["expired"]]["action"] == "would_remove"
        assert all(entries[paths[name]]["protected"] for name in ("active", "pending", "running"))
        assert all(path.exists() for path in paths.values())
        assert all(path.exists() for path in archives)

        tools_sandbox.run("cache", "gc", "--keep", "0")

        assert not paths["expired"].exists()
        assert all(paths[name].exists() for name in ("active", "pending", "running"))
        assert not any(path.exists() for path in archives)
        assert sentinel.read_text() == "user source\n"
