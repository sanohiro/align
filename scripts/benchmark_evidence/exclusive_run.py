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
        self._reservation_raw: bytes | None = None

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

    def _acquire_finalization_lock(self) -> None:
        if self._closed or self._locked:
            raise ExclusiveRunError("run lease is not available for finalization")
        flags = os.O_RDWR | os.O_CREAT
        if hasattr(os, "O_CLOEXEC"):
            flags |= os.O_CLOEXEC
        if hasattr(os, "O_NOFOLLOW"):
            flags |= os.O_NOFOLLOW
        try:
            fd = os.open(self.lock_path, flags, 0o600)
        except OSError as exc:
            raise ExclusiveRunError(f"cannot reopen host lock: {exc}") from exc
        try:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError as exc:
                raise ExclusiveRunError("host lock is held during finalization") from exc
            self._lock_fd = fd
            self._locked = True
        except BaseException:
            os.close(fd)
            raise

    def _write_reservation_file(self) -> None:
        if self._reservation_raw is None:
            raise ExclusiveRunError("reservation contents are not available")
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
            while written < len(self._reservation_raw):
                count = os.write(fd, self._reservation_raw[written:])
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

    def _restore_reservation(self) -> None:
        if os.path.lexists(self.reservation_path):
            self._reserved = True
            return
        self._write_reservation_file()
        _fsync_parent(self.reservation_path)
        self._reserved = True

    def create_reservation(self, run_id: str, output_dir: str) -> None:
        if not self._locked or self._reserved:
            raise ExclusiveRunError("reservation requires the held host lock")
        if _HEX64.fullmatch(run_id) is None:
            raise ExclusiveRunError("reservation run ID must be lowercase 64-hex")
        output_dir = _path(output_dir, "reservation output")
        self._reservation_raw = f"run_id={run_id}\noutput_dir={output_dir}\n".encode("ascii")
        try:
            self._write_reservation_file()
        except BaseException:
            try:
                os.unlink(self.reservation_path)
            except OSError:
                pass
            raise
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

    def _release_finalization_lock(self) -> None:
        """Release the reacquired flock before best-effort descriptor close."""

        if self._lock_fd < 0 or not self._locked:
            raise ExclusiveRunError("finalization lock is not held")
        # LOCK_UN is the transactional guard transition.  A close error after
        # it succeeds cannot mean that another run was admitted by this
        # lease; treating that best-effort descriptor cleanup as a failed
        # finalization would falsely report a fail-closed reservation.
        fcntl.flock(self._lock_fd, fcntl.LOCK_UN)
        try:
            os.close(self._lock_fd)
        except OSError:
            pass

    def finalize_publication(self) -> None:
        if not self._published or not self._reserved:
            raise ExclusiveRunError("publication reservation is not ready for removal")
        # Reacquire the host lock while the reservation is removed.  The
        # reservation is the first guard checked by a new process, and the
        # lock closes the brief path-absent interval before its directory
        # fsync completes or a failed removal can be restored.
        self._acquire_finalization_lock()
        try:
            os.unlink(self.reservation_path)
            _fsync_parent(self.reservation_path)
        except BaseException:
            self._restore_reservation()
            raise
        try:
            self._release_finalization_lock()
        except BaseException:
            self._restore_reservation()
            raise
        self._lock_fd = -1
        self._locked = False
        self._reserved = False
        self._closed = True

    def abort(self, *, remove_reservation: bool) -> None:
        """Close local state; leaving the reservation blocks later runs."""

        removed = False
        if remove_reservation and self._reserved:
            if not self._locked or self._lock_fd < 0:
                raise ExclusiveRunError("reservation removal requires the held host lock")
            try:
                os.unlink(self.reservation_path)
                _fsync_parent(self.reservation_path)
            except BaseException:
                self._restore_reservation()
                raise
            removed = True
        if self._lock_fd >= 0:
            try:
                fcntl.flock(self._lock_fd, fcntl.LOCK_UN)
            except BaseException:
                if removed:
                    self._restore_reservation()
                raise
            try:
                os.close(self._lock_fd)
            except OSError:
                pass
            self._lock_fd = -1
        self._locked = False
        if removed:
            self._reserved = False
        self._closed = True
