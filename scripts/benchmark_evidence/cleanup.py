"""Ordered cleanup and publication state for one evidence run."""

from __future__ import annotations

from dataclasses import dataclass


class CleanupError(ValueError):
    """A resource or publication transition is out of order."""


_RESOURCE_KINDS = ("children", "containers", "mounts", "fds", "private_dirs")


@dataclass(frozen=True)
class CleanupSnapshot:
    children_remaining: int
    containers_remaining: int
    mounts_remaining: int
    fds_remaining: int
    private_dirs_remaining: int
    host_lock_held_for_signing: bool
    source_manifests_unchanged: bool
    cache_manifests_unchanged: bool


@dataclass(frozen=True)
class CleanupResult:
    accepted: bool
    fail_closed: bool
    staging_present: bool
    output_present: bool
    reservation_present: bool


class CleanupTransaction:
    """Model every resource and publication edge before the controller exists."""

    def __init__(self) -> None:
        self._resources = {kind: set() for kind in _RESOURCE_KINDS}
        self._lock_held = True
        self._source_unchanged = True
        self._cache_unchanged = True
        self._staging = False
        self._reservation = False
        self._unlocked = False
        self._output = False
        self._accepted = False
        self._failed = False

    def _require_live(self) -> None:
        if self._accepted or self._failed:
            raise CleanupError("cleanup transaction is no longer live")

    def attach(self, kind: str, token: str) -> None:
        self._require_live()
        if self._staging:
            raise CleanupError("resources cannot be attached after report staging")
        if kind not in _RESOURCE_KINDS or not isinstance(token, str) or not token:
            raise CleanupError("resource kind or token is invalid")
        self._resources[kind].add(token)

    def remove(self, kind: str, token: str) -> None:
        self._require_live()
        if kind not in _RESOURCE_KINDS or token not in self._resources[kind]:
            raise CleanupError("resource was not owned by this run")
        self._resources[kind].remove(token)

    def set_manifest_state(self, *, source_unchanged: bool, cache_unchanged: bool) -> None:
        self._require_live()
        if self._staging:
            raise CleanupError("manifest state cannot change after report staging")
        self._source_unchanged = self._source_unchanged and source_unchanged
        self._cache_unchanged = self._cache_unchanged and cache_unchanged

    def snapshot(self) -> CleanupSnapshot:
        return CleanupSnapshot(
            *(len(self._resources[kind]) for kind in _RESOURCE_KINDS),
            self._lock_held,
            self._source_unchanged,
            self._cache_unchanged,
        )

    def _no_resources(self) -> None:
        if any(self._resources[kind] for kind in _RESOURCE_KINDS):
            raise CleanupError("all children, containers, mounts, fds, and private dirs must be gone")
        if not self._source_unchanged or not self._cache_unchanged:
            raise CleanupError("source and cache manifests changed")

    def stage_report(self) -> None:
        self._require_live()
        if not self._lock_held or self._unlocked:
            raise CleanupError("report staging requires the signing lock")
        if self._staging:
            raise CleanupError("report staging was already completed")
        self._no_resources()
        self._staging = True

    def create_reservation(self) -> None:
        self._require_live()
        if not self._staging or not self._lock_held:
            raise CleanupError("publication reservation requires durable locked staging")
        if self._reservation:
            raise CleanupError("publication reservation already exists")
        self._reservation = True

    def release_lock(self) -> None:
        self._require_live()
        if not self._reservation:
            raise CleanupError("lock cannot be released before durable reservation")
        if not self._lock_held:
            raise CleanupError("lock was already released")
        self._lock_held = False
        self._unlocked = True

    def publish_output(self) -> None:
        self._require_live()
        if not self._unlocked or not self._reservation or not self._staging:
            raise CleanupError("output publication is not in the locked-staging sequence")
        if self._output:
            raise CleanupError("output was already published")
        self._output = True
        self._staging = False

    def remove_reservation(self) -> None:
        self._require_live()
        if not self._output or not self._reservation:
            raise CleanupError("reservation removal requires published output")
        self._reservation = False

    def accept(self) -> CleanupResult:
        self._require_live()
        self._no_resources()
        if not self._output or self._reservation or self._lock_held:
            raise CleanupError("accepted output lacks the complete publication postcondition")
        self._accepted = True
        return self.result()

    def abort(self, *, cleanup_succeeded: bool, reservation_remove_succeeded: bool = True) -> CleanupResult:
        """Suppress publication; a failed cleanup stays fail-closed."""

        self._require_live()
        if cleanup_succeeded and any(self._resources[kind] for kind in _RESOURCE_KINDS):
            raise CleanupError("cleanup_succeeded is false while owned resources remain")
        if cleanup_succeeded:
            self._resources = {kind: set() for kind in _RESOURCE_KINDS}
            self._staging = False
            self._output = False
            if self._reservation and reservation_remove_succeeded:
                self._reservation = False
            self._lock_held = False
            self._unlocked = True
        self._failed = True
        return self.result()

    def result(self) -> CleanupResult:
        return CleanupResult(
            accepted=self._accepted,
            fail_closed=self._reservation or self._lock_held,
            staging_present=self._staging,
            output_present=self._output,
            reservation_present=self._reservation,
        )
