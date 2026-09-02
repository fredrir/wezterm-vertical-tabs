"""Decode the public wez-vtabs protocol from an asciicast recording."""

from __future__ import annotations

import base64
import binascii
import json
import re
from collections.abc import Iterator
from dataclasses import dataclass
from typing import Any

# OSC may use its 7-bit or 8-bit introducer and BEL, ST, or 8-bit ST terminators.
_OSC = re.compile(r"(?:\x1b\]|\x9d)(.*?)(?:\x07|\x1b\\|\x9c)", re.DOTALL)
_SET_USER_VAR = "1337;SetUserVar="


@dataclass(frozen=True, slots=True)
class UserVar:
    """One decoded SetUserVar notification in emission order."""

    name: str
    value: str

    def json(self) -> dict[str, Any]:
        decoded = json.loads(self.value)
        if not isinstance(decoded, dict):
            raise ValueError(f"{self.name} did not contain a JSON object")
        return decoded


def cast_output(recording: str) -> str:
    """Join terminal-output records from an asciicast v2 document.

    A live recording can end between writes. An incomplete final JSON line is
    ignored until the next poll; malformed complete records are rejected.
    """

    lines = recording.splitlines()
    if not lines:
        return ""

    try:
        header = json.loads(lines[0])
    except json.JSONDecodeError as error:
        raise ValueError("recording has an invalid asciicast header") from error
    if not isinstance(header, dict) or header.get("version") != 2:
        raise ValueError("recording is not an asciicast v2 document")

    output: list[str] = []
    complete = recording.endswith(("\n", "\r"))
    for index, line in enumerate(lines[1:], start=2):
        if not line:
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError as error:
            if index == len(lines) and not complete:
                break
            raise ValueError(f"invalid asciicast record on line {index}") from error
        if (
            isinstance(record, list)
            and len(record) >= 3
            and record[1] == "o"
            and isinstance(record[2], str)
        ):
            output.append(record[2])
    return "".join(output)


def user_vars(recording: str) -> list[UserVar]:
    """Return every valid SetUserVar from a recording, including split writes."""

    decoded: list[UserVar] = []
    for match in _OSC.finditer(cast_output(recording)):
        payload = match.group(1)
        if not payload.startswith(_SET_USER_VAR):
            continue
        assignment = payload[len(_SET_USER_VAR) :]
        name, separator, encoded = assignment.partition("=")
        if not separator or not name:
            continue
        padded = encoded + "=" * (-len(encoded) % 4)
        try:
            raw = base64.b64decode(padded, validate=True)
            value = raw.decode("utf-8")
        except binascii.Error, UnicodeDecodeError:
            continue
        decoded.append(UserVar(name=name, value=value))
    return decoded


def protocol_events(recording: str, variable: str = "vtabs") -> list[dict[str, Any]]:
    """Decode JSON event objects sent through the protocol user variable."""

    events: list[dict[str, Any]] = []
    for user_var in user_vars(recording):
        if user_var.name != variable:
            continue
        try:
            event = user_var.json()
        except json.JSONDecodeError, ValueError:
            continue
        events.append(event)
    return events


def osc_user_var(name: str, value: str, *, terminator: str = "\x07") -> str:
    """Build an OSC notification for decoder tests."""

    encoded = base64.b64encode(value.encode()).decode()
    return f"\x1b]1337;SetUserVar={name}={encoded}{terminator}"


def asciicast(*chunks: str) -> str:
    """Build the smallest valid asciicast containing the supplied output chunks."""

    header = {"version": 2, "width": 28, "height": 20, "timestamp": 0}
    records: Iterator[str] = (
        json.dumps([index / 1000, "o", chunk]) for index, chunk in enumerate(chunks)
    )
    return "\n".join([json.dumps(header), *records]) + "\n"
