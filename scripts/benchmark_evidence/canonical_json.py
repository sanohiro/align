#!/usr/bin/env python3
"""Strict canonical JSON primitives for evidence records.

Evidence records contain only ASCII enum/name/hex strings and unsigned integer
scalars. This module makes those restrictions executable before the complete
report schemas are layered on top: object member order is preserved, duplicate
members are rejected, and decoding requires byte-for-byte canonical re-encode.
"""

from __future__ import annotations

import hashlib
import json
from typing import Any, Sequence


MAX_U64 = (1 << 64) - 1


class CanonicalJsonError(ValueError):
    """Input is not a valid canonical evidence JSON value."""


class Object(dict):
    """JSON object retaining source order and rejecting duplicate members."""

    def __init__(self, pairs: Sequence[tuple[str, Any]]) -> None:
        super().__init__(pairs)
        self.pairs = tuple(pairs)


def _object(pairs: Sequence[tuple[str, Any]]) -> Object:
    keys = [key for key, _ in pairs]
    if len(keys) != len(set(keys)):
        raise CanonicalJsonError("duplicate JSON object member")
    return Object(pairs)


def _reject_float(_: str) -> Any:
    raise CanonicalJsonError("floating-point JSON numbers are not permitted")


def _reject_constant(_: str) -> Any:
    raise CanonicalJsonError("non-finite JSON numbers are not permitted")


def _validate_string(value: str, label: str) -> None:
    for character in value:
        codepoint = ord(character)
        if codepoint < 0x20 or codepoint > 0x7E or character in ('"', "\\"):
            raise CanonicalJsonError(f"{label} contains a prohibited character")


def _validate(value: Any, label: str = "value") -> None:
    if isinstance(value, Object):
        keys = [key for key, _ in value.pairs]
        if len(keys) != len(set(keys)):
            raise CanonicalJsonError(f"{label} contains a duplicate member")
        for key, member in value.pairs:
            if not isinstance(key, str):
                raise CanonicalJsonError(f"{label} has a non-string member name")
            _validate_string(key, f"{label} member name")
            _validate(member, f"{label}.{key}")
        return
    if isinstance(value, dict):
        keys = list(value.keys())
        if any(not isinstance(key, str) for key in keys):
            raise CanonicalJsonError(f"{label} has a non-string member name")
        for key, member in value.items():
            _validate_string(key, f"{label} member name")
            _validate(member, f"{label}.{key}")
        return
    if isinstance(value, list):
        for index, member in enumerate(value):
            _validate(member, f"{label}[{index}]")
        return
    if isinstance(value, str):
        _validate_string(value, label)
        return
    if isinstance(value, bool) or not isinstance(value, int):
        raise CanonicalJsonError(f"{label} is not an allowed JSON scalar")
    if value < 0 or value > MAX_U64:
        raise CanonicalJsonError(f"{label} is outside u64 range")


def encode(value: Any) -> bytes:
    """Encode one validated value with canonical member order and one LF."""

    _validate(value)
    try:
        return (
            json.dumps(
                value,
                ensure_ascii=True,
                allow_nan=False,
                separators=(",", ":"),
            ).encode("ascii")
            + b"\n"
        )
    except (TypeError, ValueError) as exc:
        raise CanonicalJsonError(f"cannot encode canonical JSON: {exc}") from exc


def decode(raw: bytes) -> Object | list[Any] | str | int:
    """Decode bytes and require exact canonical re-encoding."""

    if not isinstance(raw, bytes):
        raise CanonicalJsonError("canonical JSON input must be bytes")
    try:
        value = json.loads(
            raw.decode("utf-8"),
            object_pairs_hook=_object,
            parse_float=_reject_float,
            parse_constant=_reject_constant,
        )
    except (UnicodeDecodeError, json.JSONDecodeError, CanonicalJsonError) as exc:
        raise CanonicalJsonError(f"invalid canonical JSON: {exc}") from exc
    canonical = encode(value)
    if raw != canonical:
        raise CanonicalJsonError("JSON is not canonical")
    return value


def require_object(value: Any, keys: Sequence[str], label: str) -> Object:
    """Require an object with exactly the declared member order."""

    if not isinstance(value, Object):
        raise CanonicalJsonError(f"{label} is not an object")
    actual = tuple(key for key, _ in value.pairs)
    if actual != tuple(keys):
        raise CanonicalJsonError(f"{label} has the wrong member order or shape")
    return value


def require_string(value: Any, label: str) -> str:
    if not isinstance(value, str):
        raise CanonicalJsonError(f"{label} is not a string")
    _validate_string(value, label)
    return value


def require_uint(value: Any, label: str, maximum: int = MAX_U64) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0 or value > maximum:
        raise CanonicalJsonError(f"{label} is not an unsigned integer in range")
    return value


def sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()
