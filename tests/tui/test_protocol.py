from __future__ import annotations

import json

import pytest

from .protocol import asciicast, cast_output, osc_user_var, protocol_events, user_vars


def test_decodes_user_variable_split_across_terminal_writes() -> None:
    event = '{"t":"pong","echo":42,"n":2}'
    sequence = osc_user_var("vtabs", event)

    recording = asciicast(sequence[:17], sequence[17:])

    assert protocol_events(recording) == [{"t": "pong", "echo": 42, "n": 2}]


def test_decodes_bel_and_string_terminated_user_variables() -> None:
    recording = asciicast(
        osc_user_var("vtabs_role", "sidebar"),
        osc_user_var("vtabs_role", "settings", terminator="\x1b\\"),
    )

    assert [item.value for item in user_vars(recording)] == ["sidebar", "settings"]


def test_ignores_invalid_user_variable_without_losing_later_events() -> None:
    valid = osc_user_var("vtabs", '{"t":"ready","n":1}')
    recording = asciicast("\x1b]1337;SetUserVar=vtabs=%%%\x07", valid)

    assert protocol_events(recording) == [{"t": "ready", "n": 1}]


def test_rejects_non_asciicast_input_with_a_useful_error() -> None:
    recording = json.dumps({"version": 1}) + "\n"

    with pytest.raises(ValueError, match="asciicast v2"):
        cast_output(recording)

