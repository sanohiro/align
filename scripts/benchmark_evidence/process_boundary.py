"""Fail-closed process, descriptor, environment, and capture primitives.

The controller supplies the actual pipes and child process.  This module keeps
the boundary deterministic: argv and environment are explicit, the child fd
set is exact, and output overflow or a non-zero termination can never be
mistaken for a sample.
"""

from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass
from typing import Mapping, Sequence


class ProcessBoundaryError(ValueError):
    """A child cannot cross the fixed process boundary."""


class CaptureOverflow(ProcessBoundaryError):
    """A child produced more output than the profile permits."""


_ENV_ORDER = (
    "PATH",
    "LC_ALL",
    "TZ",
    "HOME",
    "CARGO_NET_OFFLINE",
    "CARGO_HOME",
    "CARGO_TARGET_DIR",
    "TMPDIR",
    "ALIGN_BENCH_WORK_DIR",
)
_NATIVE_ENV_ORDER = _ENV_ORDER + ("ALIGN_BENCH_ARTIFACT_MANIFEST_SHA256",)
_ENV_ORDERS = {"prepare": _ENV_ORDER, "native": _NATIVE_ENV_ORDER}
_HEX64 = re.compile(r"[0-9a-f]{64}\Z")
_ABS_GUEST_PATH = re.compile(r"/(?!/)[A-Za-z0-9._/-]+\Z")


def _guest_path(value: object, label: str) -> str:
    if not isinstance(value, str) or _ABS_GUEST_PATH.fullmatch(value) is None:
        raise ProcessBoundaryError(f"{label} must be an absolute path")
    if value.endswith("/") or any(part in ("", ".", "..") for part in value.split("/")[1:]):
        raise ProcessBoundaryError(f"{label} must not contain path aliases")
    return value


def validate_argv(argv: Sequence[str]) -> tuple[str, ...]:
    """Return an immutable argv after rejecting shell/encoding ambiguity."""

    if not isinstance(argv, (tuple, list)) or not argv:
        raise ProcessBoundaryError("child argv must be a non-empty sequence")
    result = tuple(argv)
    for index, value in enumerate(result):
        if not isinstance(value, str) or value == "" or "\x00" in value:
            raise ProcessBoundaryError(f"child argv[{index}] must be non-empty and NUL-free")
        if any(ord(char) < 0x20 or ord(char) > 0x7e for char in value):
            raise ProcessBoundaryError(f"child argv[{index}] must be printable ASCII")
    return result


@dataclass(frozen=True)
class FixedEnvironment:
    """The complete environment passed to one evidence child."""

    values: tuple[tuple[str, str], ...]
    phase: str = "prepare"

    def __post_init__(self) -> None:
        if not isinstance(self.phase, str) or self.phase not in _ENV_ORDERS:
            raise ProcessBoundaryError("child environment phase is not fixed")
        if tuple(key for key, _ in self.values) != _ENV_ORDERS[self.phase]:
            raise ProcessBoundaryError("child environment has the wrong fixed key order")
        for key, value in self.values:
            if (
                not isinstance(value, str)
                or not value.isascii()
                or "\x00" in value
                or "=" in value
            ):
                raise ProcessBoundaryError(f"child environment value for {key} is invalid")

    def as_dict(self) -> dict[str, str]:
        return dict(self.values)

    def as_bytes(self) -> bytes:
        return b"".join(
            f"{key}={value}\n".encode("ascii") for key, value in self.values
        )

    def sha256(self) -> str:
        return hashlib.sha256(self.as_bytes()).hexdigest()


def fixed_environment(
    *,
    phase: str = "prepare",
    manifest_sha256: str | None = None,
    home: str = "/nonexistent",
    cargo_home: str = "/cargo",
    target: str = "/target",
    tmpdir: str = "/tmp",
    work: str = "/work",
) -> FixedEnvironment:
    """Build the only environment accepted by a prepare or native child."""

    if not isinstance(phase, str) or phase not in _ENV_ORDERS:
        raise ProcessBoundaryError("child environment phase is not fixed")
    if phase == "native":
        if not isinstance(manifest_sha256, str) or _HEX64.fullmatch(manifest_sha256) is None:
            raise ProcessBoundaryError("native child requires a lowercase artifact manifest digest")
    elif manifest_sha256 is not None:
        raise ProcessBoundaryError("prepare child cannot receive an artifact manifest digest")

    for label, value in (
        ("HOME", home),
        ("CARGO_HOME", cargo_home),
        ("CARGO_TARGET_DIR", target),
        ("TMPDIR", tmpdir),
        ("ALIGN_BENCH_WORK_DIR", work),
    ):
        _guest_path(value, label)
    if (
        home != "/nonexistent"
        or cargo_home != "/cargo"
        or target != "/target"
        or tmpdir != "/tmp"
        or work != "/work"
    ):
        raise ProcessBoundaryError("child environment paths are fixed by the container profile")
    values = (
        ("PATH", "/toolchain/bin:/usr/bin:/bin"),
        ("LC_ALL", "C"),
        ("TZ", "UTC"),
        ("HOME", home),
        ("CARGO_NET_OFFLINE", "true"),
        ("CARGO_HOME", cargo_home),
        ("CARGO_TARGET_DIR", target),
        ("TMPDIR", tmpdir),
        ("ALIGN_BENCH_WORK_DIR", work),
    )
    if phase == "native":
        values += (("ALIGN_BENCH_ARTIFACT_MANIFEST_SHA256", manifest_sha256),)
    return FixedEnvironment(values, phase=phase)


def validate_environment(value: Mapping[str, str], expected: FixedEnvironment) -> None:
    """Reject ambient additions, deletions, and value substitutions."""

    if not isinstance(expected, FixedEnvironment):
        raise ProcessBoundaryError("expected environment is invalid")
    if not isinstance(value, Mapping) or dict(value) != expected.as_dict():
        raise ProcessBoundaryError("child environment is not the fixed environment")


@dataclass(frozen=True)
class DescriptorMap:
    """The descriptor mapping visible at the child entrypoint."""

    stdin: int
    stdout: int
    stderr: int
    inherited: tuple[int, ...] = ()
    fd_cloexec: Mapping[int, bool] | Sequence[tuple[int, bool]] | None = None

    def validate(self) -> None:
        if any(type(fd) is not int for fd in (self.stdin, self.stdout, self.stderr)):
            raise ProcessBoundaryError("child standard descriptors must be integers")
        if (self.stdin, self.stdout, self.stderr) != (0, 1, 2):
            raise ProcessBoundaryError("child standard descriptors must be 0, 1, and 2")
        if any(type(fd) is not int or fd < 0 for fd in self.inherited):
            raise ProcessBoundaryError("inherited descriptors must be non-negative integers")
        if self.inherited:
            raise ProcessBoundaryError("child inherited unexpected descriptors")
        validate_fd_inventory(self.fd_cloexec)


def validate_fd_inventory(
    fds: Mapping[int, bool] | Sequence[tuple[int, bool]] | None,
) -> None:
    """Validate all pre-exec descriptors and their FD_CLOEXEC state."""

    if fds is None:
        raise ProcessBoundaryError("child descriptor inventory requires CLOEXEC state")
    if isinstance(fds, Mapping):
        items = tuple(fds.items())
    elif isinstance(fds, (tuple, list)):
        items = tuple(fds)
    else:
        raise ProcessBoundaryError("child descriptor inventory must be fd/CLOEXEC pairs")
    normalized: list[tuple[int, bool]] = []
    for item in items:
        if not isinstance(item, (tuple, list)) or len(item) != 2:
            raise ProcessBoundaryError("child descriptor inventory must be fd/CLOEXEC pairs")
        fd, cloexec = item
        if type(fd) is not int or fd < 0 or type(cloexec) is not bool:
            raise ProcessBoundaryError("child descriptor inventory has an invalid fd or CLOEXEC flag")
        normalized.append((fd, cloexec))
    if len({fd for fd, _ in normalized}) != len(normalized):
        raise ProcessBoundaryError("child descriptor inventory repeats an fd")
    flags = dict(normalized)
    if not {0, 1, 2}.issubset(flags):
        raise ProcessBoundaryError("child descriptor inventory must include 0, 1, and 2")
    if any(flags[fd] for fd in (0, 1, 2)):
        raise ProcessBoundaryError("standard descriptors must not have FD_CLOEXEC")
    if any(not cloexec for fd, cloexec in normalized if fd not in (0, 1, 2)):
        raise ProcessBoundaryError("controller descriptors must have FD_CLOEXEC")


@dataclass(frozen=True)
class CaptureResult:
    """Bounded output metadata retained in the evidence report."""

    size: int
    sha256: str
    tail_hex: str


class BoundedCapture:
    """Hash and retain a bounded diagnostic tail without an unbounded buffer."""

    def __init__(self, limit: int, tail_limit: int = 4096) -> None:
        if type(limit) is not int or limit <= 0:
            raise ProcessBoundaryError("capture limit must be positive")
        if type(tail_limit) is not int or tail_limit < 0 or tail_limit > limit:
            raise ProcessBoundaryError("capture tail limit is invalid")
        self._limit = limit
        self._tail_limit = tail_limit
        self._hash = hashlib.sha256()
        self._tail = bytearray()
        self._size = 0
        self._closed = False

    def feed(self, chunk: bytes) -> None:
        if self._closed:
            raise ProcessBoundaryError("capture is already closed")
        if not isinstance(chunk, bytes):
            raise ProcessBoundaryError("capture chunk must be bytes")
        if self._size + len(chunk) > self._limit:
            self._closed = True
            raise CaptureOverflow("child output exceeded the capture limit")
        self._hash.update(chunk)
        self._size += len(chunk)
        if self._tail_limit:
            self._tail.extend(chunk)
            del self._tail[:-self._tail_limit]

    def finish(self) -> CaptureResult:
        if self._closed:
            raise ProcessBoundaryError("capture cannot finish after overflow or close")
        self._closed = True
        return CaptureResult(self._size, self._hash.hexdigest(), bytes(self._tail).hex())


@dataclass(frozen=True)
class ChildExit:
    """The only termination facts the controller may accept as a result."""

    exit_code: int | None
    signal: int | None = None
    timed_out: bool = False
    truncated: bool = False

    def validate(self) -> None:
        if self.exit_code is not None and (
            type(self.exit_code) is not int or self.exit_code < 0 or self.exit_code > 255
        ):
            raise ProcessBoundaryError("child exit code is invalid")
        if self.signal is not None and (
            type(self.signal) is not int or self.signal <= 0 or self.signal > 255
        ):
            raise ProcessBoundaryError("child signal is invalid")
        if self.exit_code is not None and self.signal is not None:
            raise ProcessBoundaryError("child termination cannot contain both exit and signal")
        if self.exit_code is None and self.signal is None:
            raise ProcessBoundaryError("child termination has no exit or signal status")

    def accepted(self) -> bool:
        self.validate()
        return (
            self.exit_code == 0
            and self.signal is None
            and not self.timed_out
            and not self.truncated
        )


def require_success(status: ChildExit) -> None:
    """Reject timeout, signal, truncation, and every non-zero exit."""

    if not status.accepted():
        raise ProcessBoundaryError("child did not complete as an accepted zero-exit run")
