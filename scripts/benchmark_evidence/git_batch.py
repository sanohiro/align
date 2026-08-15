"""Strict parser for one ``git cat-file --batch`` response.

The pinned Git process wrapper will feed this module one response at a time.
The parser is pure and performs no repository, configuration, network, or
process work.  A successful object is re-bound through the raw-object codec;
missing objects are represented explicitly and never become a fabricated
empty payload.
"""

from __future__ import annotations

from dataclasses import dataclass

from . import git_objects


class GitBatchError(ValueError):
    """A batch response violates the exact Git batch protocol boundary."""


@dataclass(frozen=True)
class BatchResult:
    """One response for the requested object ID."""

    oid: str
    object: git_objects.VerifiedObject | None


def _error(message: str) -> None:
    raise GitBatchError(message)


def parse(requested_oid: object, response: object) -> BatchResult:
    """Parse one exact batch response, including its final separator LF."""

    try:
        requested = git_objects.validate_oid(requested_oid)
    except git_objects.GitObjectError as exc:
        raise GitBatchError(str(exc)) from exc
    if not isinstance(response, bytes):
        _error("batch response must be bytes")
    line_end = response.find(b"\n")
    if line_end < 0:
        _error("batch response header has no LF")
    header = response[:line_end]
    fields = header.split(b" ")
    if len(fields) == 2 and fields[0] == requested.encode("ascii") and fields[1] == b"missing":
        if response[line_end + 1 :] != b"":
            _error("missing-object response has trailing bytes")
        return BatchResult(oid=requested, object=None)
    if len(fields) != 3:
        _error("batch response header has the wrong shape")
    response_oid, kind_bytes, size_text = fields
    try:
        response_oid_text = response_oid.decode("ascii")
        kind = kind_bytes.decode("ascii")
        size_text_text = size_text.decode("ascii")
    except UnicodeDecodeError as exc:
        raise GitBatchError("batch response header is not ASCII") from exc
    try:
        response_oid_text = git_objects.validate_oid(response_oid_text)
    except git_objects.GitObjectError as exc:
        raise GitBatchError(str(exc)) from exc
    if response_oid_text != requested:
        _error("batch response object ID does not match the request")
    if not size_text_text or (
        len(size_text_text) > 1 and size_text_text[0] == "0"
    ) or not size_text_text.isdecimal():
        _error("batch response size is not canonical decimal")
    payload = response[line_end + 1 :]
    if len(payload) == 0 or payload[-1:] != b"\n":
        _error("batch response is missing its payload separator LF")
    payload = payload[:-1]
    if size_text_text != str(len(payload)):
        _error("batch response payload length does not match its header")
    try:
        raw = git_objects.encode(kind, payload)
        verified = git_objects.verify(response_oid_text, raw)
    except git_objects.GitObjectError as exc:
        raise GitBatchError(str(exc)) from exc
    return BatchResult(oid=requested, object=verified)
