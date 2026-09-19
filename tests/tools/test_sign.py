"""Release bundles sign every helper with a stable identifier under the keychain's Developer ID."""

import json
import sys
from pathlib import Path

import pytest

pytestmark = [
    pytest.mark.rust,
    pytest.mark.skipif(sys.platform != "darwin", reason="macOS code signing"),
]

APPLE_DEVELOPMENT = ("A" * 40, "Apple Development: Fixture Person (DEV1234567)")
DEVELOPER_ID = "Developer ID Application: Fixture Person (TEAM123456)"
HELPERS = ("strip-ansi-escapes", "wez-vtabs", "wez-vtabs-store", "wezterm", "wezterm-mux-server")


def keychain(recording_cargo: Path, *identities: tuple[str, str]) -> None:
    lines = [f'  {index}) {digest} "{name}"' for index, (digest, name) in enumerate(identities, 1)]
    lines.append(f"     {len(identities)} valid identities found")
    (recording_cargo.parent / "identities.txt").write_text("\n".join(lines) + "\n")


def signing_calls(recording_cargo: Path) -> list[list[str]]:
    calls = []
    for line in (recording_cargo.parent / "codesign.jsonl").read_text().splitlines():
        *arguments, target = json.loads(line)
        parts = Path(target).parts
        calls.append([*arguments, "/".join(parts[parts.index("WezTerm.app") :])])
    return calls


@pytest.mark.parametrize(
    ("identities", "signer"),
    [
        ([APPLE_DEVELOPMENT], "-"),
        ([APPLE_DEVELOPMENT, ("B" * 40, DEVELOPER_ID)], "B" * 40),
        ([("B" * 40, DEVELOPER_ID), ("C" * 40, DEVELOPER_ID)], "B" * 40),
    ],
    ids=["ad-hoc-without-developer-id", "developer-id", "renewed-developer-id"],
)
def test_package_signs_helpers_before_the_app(
    tools_sandbox, local_upstream, recording_cargo, identities, signer
):
    _, revision = local_upstream
    keychain(recording_cargo, *identities)

    tools_sandbox.run("--upstream", revision, "package")

    sign = ["--force", "--timestamp=none", "--sign", signer]
    assert signing_calls(recording_cargo) == [
        *(
            [
                *sign,
                "--identifier",
                f"com.github.wez.wezterm.{name}",
                f"WezTerm.app/Contents/MacOS/{name}",
            ]
            for name in HELPERS
        ),
        [*sign, "WezTerm.app"],
        ["--verify", "--deep", "--strict", "WezTerm.app"],
    ]


def test_package_refuses_to_choose_between_developer_ids(
    tools_sandbox, local_upstream, recording_cargo
):
    _, revision = local_upstream
    other = "Developer ID Application: Other Person (OTHER12345)"
    keychain(recording_cargo, ("B" * 40, DEVELOPER_ID), ("C" * 40, other))

    result = tools_sandbox.run("--upstream", revision, "package", check=False)

    assert result.returncode != 0
    assert "multiple Developer ID identities" in result.stderr
    assert DEVELOPER_ID in result.stderr
    assert other in result.stderr
    assert not (recording_cargo.parent / "codesign.jsonl").exists()
