"""Strict identity codec for raw Git objects.

The future controller reads raw objects through a pinned Git boundary.  This
module deliberately stays below that process boundary: it parses one exact
uncompressed Git object record, validates its canonical header and payload
length, and binds the bytes to the supplied SHA-1 object ID plus a SHA-256
digest of the complete raw record.  Repository traversal and revision-policy
checks remain controller responsibilities.
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass


class GitObjectError(ValueError):
    """A raw Git object record or identity is malformed."""


_KINDS = frozenset(("blob", "tree", "commit", "tag"))
_OID_LENGTH = 40


@dataclass(frozen=True)
class ParsedObject:
    """The canonical kind and payload from one raw Git object record."""

    kind: str
    payload: bytes


@dataclass(frozen=True)
class VerifiedObject:
    """A raw object whose SHA-1 OID and SHA-256 identity were verified."""

    kind: str
    oid: str
    raw_sha256: str
    payload: bytes


def _kind(kind: object) -> str:
    if not isinstance(kind, str) or kind not in _KINDS:
        raise GitObjectError("object kind is not a supported Git kind")
    return kind


def _oid(oid: object) -> str:
    if not isinstance(oid, str) or len(oid) != _OID_LENGTH:
        raise GitObjectError("object ID must be 40 lowercase hexadecimal characters")
    if any(char not in "0123456789abcdef" for char in oid):
        raise GitObjectError("object ID must be 40 lowercase hexadecimal characters")
    return oid


def _payload(payload: object) -> bytes:
    if not isinstance(payload, bytes):
        raise GitObjectError("object payload must be bytes")
    return payload


def encode(kind: object, payload: object) -> bytes:
    """Encode one canonical uncompressed Git object record."""

    kind = _kind(kind)
    payload = _payload(payload)
    return kind.encode("ascii") + b" " + str(len(payload)).encode("ascii") + b"\0" + payload


def parse(raw: object) -> ParsedObject:
    """Parse one exact Git object record and reject all trailing bytes."""

    if not isinstance(raw, bytes):
        raise GitObjectError("raw object must be bytes")
    separator = raw.find(b"\0")
    if separator < 0:
        raise GitObjectError("raw object header has no NUL terminator")
    header = raw[:separator]
    fields = header.split(b" ")
    if len(fields) != 2:
        raise GitObjectError("raw object header has the wrong shape")
    try:
        kind = fields[0].decode("ascii")
        size_text = fields[1].decode("ascii")
    except UnicodeDecodeError as exc:
        raise GitObjectError("raw object header is not ASCII") from exc
    _kind(kind)
    if not size_text or (len(size_text) > 1 and size_text[0] == "0") or not size_text.isdecimal():
        raise GitObjectError("raw object size is not canonical decimal")
    size = int(size_text, 10)
    payload = raw[separator + 1 :]
    if len(payload) != size:
        raise GitObjectError("raw object payload length does not match its header")
    if encode(kind, payload) != raw:
        raise GitObjectError("raw object is not canonical")
    return ParsedObject(kind=kind, payload=payload)


def _sha1(raw: bytes) -> str:
    try:
        return hashlib.sha1(raw, usedforsecurity=False).hexdigest()
    except TypeError:
        return hashlib.sha1(raw).hexdigest()


def verify(oid: object, raw: object) -> VerifiedObject:
    """Verify the supplied lowercase SHA-1 OID against one raw object."""

    expected_oid = _oid(oid)
    if not isinstance(raw, bytes):
        raise GitObjectError("raw object must be bytes")
    parsed = parse(raw)
    observed_oid = _sha1(raw)
    if observed_oid != expected_oid:
        raise GitObjectError("raw object does not match its object ID")
    return VerifiedObject(
        kind=parsed.kind,
        oid=observed_oid,
        raw_sha256=hashlib.sha256(raw).hexdigest(),
        payload=parsed.payload,
    )
