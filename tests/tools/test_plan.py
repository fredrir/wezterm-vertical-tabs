"""Cache decisions distinguish compilation, validation, and distributable sources."""

import pytest

from tests.tools.conftest import write_file

pytestmark = pytest.mark.rust


@pytest.mark.parametrize(
    ("path", "contents", "compile_changes", "validation_changes"),
    [
        ("README.md", "Updated documentation\n", False, False),
        ("docs/guide.md", "New guide\n", False, False),
        ("tests/tools/test_example.py", "def test_example(): pass\n", False, True),
        ("tools/src/main.rs", "fn main() {}\n", False, True),
        ("crates/vtabs-app/src/lib.rs", "pub fn changed() {}\n", True, True),
        ("native/adapter/storage.rs", "pub fn changed() {}\n", True, True),
    ],
)
def test_plan_separates_build_validation_and_bundle_inputs(
    tools_sandbox, path, contents, compile_changes, validation_changes
):
    before = tools_sandbox.json("plan")["inputs"]
    write_file(tools_sandbox.root, path, contents)

    after = tools_sandbox.json("plan")["inputs"]

    assert after["source"] != before["source"]
    assert (after["compile"] != before["compile"]) is compile_changes
    assert (after["validation"] != before["validation"]) is validation_changes
    assert not (tools_sandbox.cache / "upstream").exists()
    assert not (tools_sandbox.cache / "worktree").exists()


def test_plan_ignores_generated_files_without_hiding_test_fixtures(tools_sandbox):
    before = tools_sandbox.json("plan")["inputs"]
    for name in (
        "crates/vtabs-app/target/generated.rs",
        "tests/__pycache__/conftest.pyc",
        "tools/target/debug/wez-vtabs",
        "tests/.pytest_cache/state",
    ):
        write_file(tools_sandbox.root, name, "generated\n")
    assert tools_sandbox.json("plan")["inputs"] == before

    write_file(tools_sandbox.root, "tests/containers/ssh/Dockerfile", "FROM fixture\n")
    after = tools_sandbox.json("plan")["inputs"]
    assert after["source"] != before["source"]
    assert after["validation"] != before["validation"]
    assert after["compile"] == before["compile"]


def test_plan_explains_operations_and_keeps_explicit_revision(tools_sandbox):
    revision = "a" * 40
    plan = tools_sandbox.json("--upstream", revision, "plan", "dev")

    assert plan["operation"] == "dev"
    assert plan["upstream"] == revision
    assert plan["profile"] == "iterate"
    assert plan["steps"]
    assert all(
        step["reason"] and step["action"] in {"run", "reuse", "check"} for step in plan["steps"]
    )
    assert not (tools_sandbox.cache / "upstream").exists()
