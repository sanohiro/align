"""Host-global lock and durable publication reservation."""

from __future__ import annotations

import fcntl
import os
import re
from dataclasses import dataclass


class ExclusiveRunError(RuntimeError):
    """Another run owns the evidence host or publication reservation."""


_HEX64 = re.compile(r"[0-9a-f]{64}\Z")
_SERIALIZED_PATH = re.compile(r"/[A-Za-z0-9._/:+=@-]+\Z")


def _path(value: object, label: str) -> str:
    if not isinstance(value, str) or not os.path.isabs(value) or value == "/":
        raise ExclusiveRunError(f"{label} must be an absolute non-root path")
    if _SERIALIZED_PATH.fullmatch(value) is None or value.endswith("/"):
        raise ExclusiveRunError(f"{label} contains an invalid path")
    if any(part in ("", ".", "..") for part in value.split("/")[1:]):
        raise ExclusiveRunError(f"{label} contains a path alias")
    return value


def _fsync_parent(path: str) -> None:
    parent = os.path.dirname(path) or "/"
    fd = os.open(parent, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


@dataclass
class ExclusiveRun:
    """An exclusive lease that survives lock release until publication settles."""

    lock_path: str
    reservation_path: str

    def __post_init__(self) -> None:
        self.lock_path = _path(self.lock_path, "lock path")
        self.reservation_path = _path(self.reservation_path, "reservation path")
        if self.lock_path == self.reservation_path:
            raise ExclusiveRunError("lock and reservation paths must differ")
        self._lock_fd = -1
        self._locked = False
        self._reserved = False
        self._published = False
        self._closed = False

    @property
    def locked(self) -> bool:
        return self._locked

    @property
    def reserved(self) -> bool:
        return self._reserved or os.path.lexists(self.reservation_path)

    def acquire(self) -> None:
        if self._closed or self._locked:
            raise ExclusiveRunError("run lease is not available")
        if os.path.lexists(self.reservation_path):
            raise ExclusiveRunError("publication reservation already exists")
        flags = os.O_RDWR | os.O_CREAT
        if hasattr(os, "O_CLOEXEC"):
            flags |= os.O_CLOEXEC
        if hasattr(os, "O_NOFOLLOW"):
            flags |= os.O_NOFOLLOW
        try:
            fd = os.open(self.lock_path, flags, 0o600)
        except OSError as exc:
            raise ExclusiveRunError(f"cannot open host lock: {exc}") from exc
        try:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError as exc:
                raise ExclusiveRunError("host lock is held") from exc
            if os.path.lexists(self.reservation_path):
                raise ExclusiveRunError("publication reservation appeared while acquiring lock")
            self._lock_fd = fd
            self._locked = True
        except BaseException:
            os.close(fd)
            raise

    def create_reservation(self, run_id: str, output_dir: str) -> None:
        if not self._locked or self._reserved:
            raise ExclusiveRunError("reservation requires the held host lock")
        if _HEX64.fullmatch(run_id) is None:
            raise ExclusiveRunError("reservation run ID must be lowercase 64-hex")
        output_dir = _path(output_dir, "reservation output")
        raw = f"run_id={run_id}\noutput_dir={output_dir}\n".encode("ascii")
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
        if hasattr(os, "O_CLOEXEC"):
            flags |= os.O_CLOEXEC
        if hasattr(os, "O_NOFOLLOW"):
            flags |= os.O_NOFOLLOW
        try:
            fd = os.open(self.reservation_path, flags, 0o600)
        except OSError as exc:
            raise ExclusiveRunError(f"cannot create publication reservation: {exc}") from exc
        try:
            written = 0
            while written < len(raw):
                count = os.write(fd, raw[written:])
                if count <= 0:
                    raise ExclusiveRunError("publication reservation made no progress")
                written += count
            os.fsync(fd)
        except BaseException:
            try:
                os.unlink(self.reservation_path)
            except OSError:
                pass
            raise
        finally:
            os.close(fd)
        _fsync_parent(self.reservation_path)
        self._reserved = True

    def release_lock_for_publication(self) -> None:
        if not self._locked or not self._reserved:
            raise ExclusiveRunError("lock release requires the durable reservation")
        fcntl.flock(self._lock_fd, fcntl.LOCK_UN)
        os.close(self._lock_fd)
        self._lock_fd = -1
        self._locked = False

    def mark_published(self) -> None:
        if self._locked or not self._reserved or self._published:
            raise ExclusiveRunError("publication must follow lock release and precede finalization")
        self._published = True

    def finalize_publication(self) -> None:
        if not self._published or not self._reserved:
            raise ExclusiveRunError("publication reservation is not ready for removal")
        os.unlink(self.reservation_path)
        _fsync_parent(self.reservation_path)
        self._reserved = False
        self._closed = True

    def abort(self, *, remove_reservation: bool) -> None:
        """Close local state; leaving the reservation blocks later runs."""

        if self._lock_fd >= 0:
            fcntl.flock(self._lock_fd, fcntl.LOCK_UN)
            os.close(self._lock_fd)
            self._lock_fd = -1
        self._locked = False
        if remove_reservation and self._reserved:
            os.unlink(self.reservation_path)
            _fsync_parent(self.reservation_path)
            self._reserved = False
        self._closed = True
