"""Fail-closed lifecycle bookkeeping for the evidence-host monitor.

This module deliberately does not read ``/proc`` or sysfs, open cgroups, or
invoke Docker.  A privileged collection adapter supplies snapshots and event
notifications to :class:`MonitorLifecycle`; the lifecycle core turns them
into the ordered host observations consumed by the future controller.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Sequence


_HEX64 = re.compile(r"[0-9a-f]{64}\Z")
_U32_MAX = (1 << 32) - 1
_U64_MAX = (1 << 64) - 1
_HOST_PHASES = (
    "pre-build",
    "child-start",
    "child-sample",
    "child-end",
    "between-children",
    "post-run",
)
_CHILD_PHASES = ("child-start", "child-sample", "child-end")
_COUNTER_FIELDS = (
    "throttle_events",
    "thermal_events",
    "foreign_schedule_events",
    "foreign_container_events",
    "monitor_lost_events",
)
_MONOTONIC_TOTAL_FIELDS = (
    "cpu_pressure_total_us",
    "memory_pressure_total_us",
    "swap_read_bytes",
    "swap_write_bytes",
)
_EVENT_KINDS = {
    "throttle": "throttle_events",
    "thermal": "thermal_events",
    "foreign_schedule": "foreign_schedule_events",
    "foreign_container": "foreign_container_events",
    "monitor_lost": "monitor_lost_events",
    "cpu_migration": None,
    "frequency_limit": None,
    "pressure_limit": None,
    "swap": None,
    "memory_limit": None,
    "source_overflow": None,
    "monitor_delay": None,
    "monitor_death": None,
}


class MonitorLifecycleError(ValueError):
    """The monitor stream cannot produce accepted evidence."""


def _uint(value: object, label: str, maximum: int) -> int:
    if type(value) is not int or value < 0 or value > maximum:
        raise MonitorLifecycleError(f"{label} must be an unsigned integer in range")
    return value


def _hex64(value: object, label: str) -> str:
    if not isinstance(value, str) or _HEX64.fullmatch(value) is None:
        raise MonitorLifecycleError(f"{label} must be lowercase SHA-256")
    return value


def _child_id(value: object, label: str) -> str:
    if value == "":
        return ""
    return _hex64(value, label)


@dataclass(frozen=True)
class MonitorSnapshot:
    """One trusted sample from the host monitor's source adapters."""

    monotonic_ns: int
    load_milli: int
    cpu_pressure_total_us: int
    memory_pressure_total_us: int
    free_memory_bytes: int
    swap_read_bytes: int
    swap_write_bytes: int
    throttle_events: int
    thermal_events: int
    foreign_schedule_events: int
    foreign_container_events: int
    monitor_lost_events: int
    frequency_khz: int
    temperature_millic: int
    container_manifest_sha256: str

    def __post_init__(self) -> None:
        for field in (
            "monotonic_ns",
            "load_milli",
            "cpu_pressure_total_us",
            "memory_pressure_total_us",
            "free_memory_bytes",
            "swap_read_bytes",
            "swap_write_bytes",
            "throttle_events",
            "thermal_events",
            "foreign_schedule_events",
            "foreign_container_events",
            "monitor_lost_events",
            "frequency_khz",
            "temperature_millic",
        ):
            _uint(getattr(self, field), f"snapshot.{field}", _U64_MAX)
        _hex64(self.container_manifest_sha256, "snapshot.container_manifest_sha256")

    def counters(self) -> tuple[int, ...]:
        return tuple(getattr(self, field) for field in _COUNTER_FIELDS)


@dataclass(frozen=True)
class MonitorObservation:
    """A report-shaped observation with its dense local ordinal."""

    ordinal: int
    phase: str
    monotonic_ns: int
    child_id: str
    load_milli: int
    cpu_pressure_total_us: int
    memory_pressure_total_us: int
    free_memory_bytes: int
    swap_read_bytes: int
    swap_write_bytes: int
    throttle_events: int
    thermal_events: int
    foreign_schedule_events: int
    foreign_container_events: int
    monitor_lost_events: int
    frequency_khz: int
    temperature_millic: int
    container_manifest_sha256: str

    def __post_init__(self) -> None:
        _uint(self.ordinal, "observation.ordinal", _U32_MAX)
        if self.phase not in _HOST_PHASES:
            raise MonitorLifecycleError("observation.phase is not a declared host phase")
        _uint(self.monotonic_ns, "observation.monotonic_ns", _U64_MAX)
        _child_id(self.child_id, "observation.child_id")
        for field in (
            "load_milli",
            "cpu_pressure_total_us",
            "memory_pressure_total_us",
            "free_memory_bytes",
            "swap_read_bytes",
            "swap_write_bytes",
            "throttle_events",
            "thermal_events",
            "foreign_schedule_events",
            "foreign_container_events",
            "monitor_lost_events",
            "frequency_khz",
            "temperature_millic",
        ):
            _uint(getattr(self, field), f"observation.{field}", _U64_MAX)
        _hex64(self.container_manifest_sha256, "observation.container_manifest_sha256")
        if self.phase in _CHILD_PHASES and self.child_id == "":
            raise MonitorLifecycleError("child observation must carry a child_id")
        if self.phase not in _CHILD_PHASES and self.child_id != "":
            raise MonitorLifecycleError("non-child observation must have an empty child_id")

    @classmethod
    def from_snapshot(
        cls, ordinal: int, phase: str, child_id: str, snapshot: MonitorSnapshot
    ) -> "MonitorObservation":
        return cls(
            ordinal=ordinal,
            phase=phase,
            monotonic_ns=snapshot.monotonic_ns,
            child_id=child_id,
            load_milli=snapshot.load_milli,
            cpu_pressure_total_us=snapshot.cpu_pressure_total_us,
            memory_pressure_total_us=snapshot.memory_pressure_total_us,
            free_memory_bytes=snapshot.free_memory_bytes,
            swap_read_bytes=snapshot.swap_read_bytes,
            swap_write_bytes=snapshot.swap_write_bytes,
            throttle_events=snapshot.throttle_events,
            thermal_events=snapshot.thermal_events,
            foreign_schedule_events=snapshot.foreign_schedule_events,
            foreign_container_events=snapshot.foreign_container_events,
            monitor_lost_events=snapshot.monitor_lost_events,
            frequency_khz=snapshot.frequency_khz,
            temperature_millic=snapshot.temperature_millic,
            container_manifest_sha256=snapshot.container_manifest_sha256,
        )

    def as_dict(self) -> dict[str, object]:
        """Return the exact report-schema member order for this observation."""

        return {
            "ordinal": self.ordinal,
            "phase": self.phase,
            "monotonic_ns": self.monotonic_ns,
            "child_id": self.child_id,
            "load_milli": self.load_milli,
            "cpu_pressure_total_us": self.cpu_pressure_total_us,
            "memory_pressure_total_us": self.memory_pressure_total_us,
            "free_memory_bytes": self.free_memory_bytes,
            "swap_read_bytes": self.swap_read_bytes,
            "swap_write_bytes": self.swap_write_bytes,
            "throttle_events": self.throttle_events,
            "thermal_events": self.thermal_events,
            "foreign_schedule_events": self.foreign_schedule_events,
            "foreign_container_events": self.foreign_container_events,
            "monitor_lost_events": self.monitor_lost_events,
            "frequency_khz": self.frequency_khz,
            "temperature_millic": self.temperature_millic,
            "container_manifest_sha256": self.container_manifest_sha256,
        }

    def to_snapshot(self) -> MonitorSnapshot:
        """Reconstruct the source snapshot represented by this observation."""

        return MonitorSnapshot(
            monotonic_ns=self.monotonic_ns,
            load_milli=self.load_milli,
            cpu_pressure_total_us=self.cpu_pressure_total_us,
            memory_pressure_total_us=self.memory_pressure_total_us,
            free_memory_bytes=self.free_memory_bytes,
            swap_read_bytes=self.swap_read_bytes,
            swap_write_bytes=self.swap_write_bytes,
            throttle_events=self.throttle_events,
            thermal_events=self.thermal_events,
            foreign_schedule_events=self.foreign_schedule_events,
            foreign_container_events=self.foreign_container_events,
            monitor_lost_events=self.monitor_lost_events,
            frequency_khz=self.frequency_khz,
            temperature_millic=self.temperature_millic,
            container_manifest_sha256=self.container_manifest_sha256,
        )


@dataclass(frozen=True)
class ChildRange:
    """The inclusive observation range owned by one preparation or run child."""

    child_id: str
    first: int
    last: int

    def __post_init__(self) -> None:
        _hex64(self.child_id, "child_range.child_id")
        _uint(self.first, "child_range.first", _U32_MAX)
        _uint(self.last, "child_range.last", _U32_MAX)
        if self.first > self.last:
            raise MonitorLifecycleError("child range must be nonempty")


@dataclass(frozen=True)
class MonitorResult:
    """Validated observations and ranges emitted by a successful lifecycle."""

    observations: tuple[MonitorObservation, ...]
    child_ranges: tuple[ChildRange, ...]


def validate_report_observations(
    observations: Sequence[MonitorObservation],
    expected_child_ids: Sequence[str],
    max_sample_gap_ns: int = 100_000_000,
) -> MonitorResult:
    """Replay report observations through the complete monitor lifecycle."""

    observations = tuple(observations)
    lifecycle = MonitorLifecycle(expected_child_ids, max_sample_gap_ns)
    lifecycle.open()
    for ordinal, observation in enumerate(observations):
        if not isinstance(observation, MonitorObservation):
            raise MonitorLifecycleError("report observations have the wrong type")
        if observation.ordinal != ordinal:
            raise MonitorLifecycleError("observation ordinals must be dense and ordered")
        snapshot = observation.to_snapshot()
        if observation.phase == "pre-build":
            lifecycle.record_pre_build(snapshot)
        elif observation.phase == "child-start":
            lifecycle.start_child(observation.child_id, snapshot)
        elif observation.phase == "child-sample":
            lifecycle.sample_child(snapshot)
        elif observation.phase == "child-end":
            lifecycle.end_child(snapshot)
        elif observation.phase == "between-children":
            lifecycle.record_between_children(snapshot)
        elif observation.phase == "post-run":
            lifecycle.record_post_run(snapshot)
        else:
            raise MonitorLifecycleError("report observation has an unknown phase")
    result = lifecycle.finish()
    if result.observations != tuple(observations):
        raise MonitorLifecycleError("replayed observations do not match the report")
    return result


def validate_child_ranges(
    observations: Sequence[MonitorObservation], child_ranges: Sequence[ChildRange]
) -> None:
    """Validate the dense, disjoint partition required by the report contract."""

    if not observations:
        raise MonitorLifecycleError("monitor must contain at least one observation")
    for ordinal, observation in enumerate(observations):
        if observation.ordinal != ordinal:
            raise MonitorLifecycleError("observation ordinals must be dense and ordered")

    by_child: dict[str, list[int]] = {}
    for ordinal, observation in enumerate(observations):
        if observation.phase in _CHILD_PHASES:
            if observation.child_id == "":
                raise MonitorLifecycleError("child observation is missing its child_id")
            by_child.setdefault(observation.child_id, []).append(ordinal)
        elif observation.child_id != "":
            raise MonitorLifecycleError("non-child observation carries a child_id")

    actual: list[ChildRange] = []
    covered: set[int] = set()
    for child_id, ordinals in by_child.items():
        expected = list(range(ordinals[0], ordinals[-1] + 1))
        if ordinals != expected:
            raise MonitorLifecycleError("one child owns a non-contiguous observation range")
        phases = [observations[index].phase for index in ordinals]
        if phases[0] != "child-start" or phases[-1] != "child-end":
            raise MonitorLifecycleError("child range must start and end with its boundary phases")
        if any(phase != "child-sample" for phase in phases[1:-1]):
            raise MonitorLifecycleError("child range interior must contain only samples")
        covered.update(ordinals)
        actual.append(ChildRange(child_id, ordinals[0], ordinals[-1]))

    actual.sort(key=lambda item: item.first)
    supplied = tuple(child_ranges)
    if supplied != tuple(actual):
        raise MonitorLifecycleError("child ranges do not exactly match observations")
    previous_last = -1
    for child_range in actual:
        if child_range.first <= previous_last:
            raise MonitorLifecycleError("child ranges overlap or are not ordered")
        previous_last = child_range.last
    if covered != {
        ordinal
        for ordinal, observation in enumerate(observations)
        if observation.phase in _CHILD_PHASES
    }:
        raise MonitorLifecycleError("child ranges do not cover every child observation")


class MonitorLifecycle:
    """Collect and validate one monitor session without touching host state."""

    def __init__(
        self,
        expected_child_ids: Sequence[str] = (),
        max_sample_gap_ns: int = 100_000_000,
    ) -> None:
        self._expected_child_ids = tuple(expected_child_ids)
        if len(set(self._expected_child_ids)) != len(self._expected_child_ids):
            raise MonitorLifecycleError("expected child IDs must be unique")
        for child_id in self._expected_child_ids:
            _hex64(child_id, "expected_child_id")
        self._max_sample_gap_ns = _uint(
            max_sample_gap_ns, "max_sample_gap_ns", _U64_MAX
        )
        if self._max_sample_gap_ns == 0:
            raise MonitorLifecycleError("max_sample_gap_ns must be positive")
        self._state = "new"
        self._observations: list[MonitorObservation] = []
        self._child_ranges: list[ChildRange] = []
        self._seen_child_ids: set[str] = set()
        self._child_order: list[str] = []
        self._active_child: str | None = None
        self._last_snapshot: MonitorSnapshot | None = None
        self._violations: list[str] = []

    @property
    def observations(self) -> tuple[MonitorObservation, ...]:
        return tuple(self._observations)

    @property
    def violations(self) -> tuple[str, ...]:
        return tuple(self._violations)

    def _error(self, message: str) -> None:
        self._state = "failed"
        raise MonitorLifecycleError(message)

    def _open_required(self) -> None:
        if self._state == "new":
            self._error("monitor lifecycle is not open")
        if self._state in ("closed", "failed"):
            self._error("monitor lifecycle is no longer active")

    def _latch(self, reason: str) -> None:
        if reason not in self._violations:
            self._violations.append(reason)

    def _append(self, phase: str, child_id: str, snapshot: MonitorSnapshot) -> None:
        self._open_required()
        if self._last_snapshot is not None:
            if snapshot.monotonic_ns <= self._last_snapshot.monotonic_ns:
                self._error("monitor monotonic clock moved backwards or stopped")
            if (
                snapshot.monotonic_ns - self._last_snapshot.monotonic_ns
                > self._max_sample_gap_ns
            ):
                self._latch("monitor delay exceeded profile ceiling")
            previous = self._last_snapshot.counters()
            current = snapshot.counters()
            for field, before, after in zip(_COUNTER_FIELDS, previous, current):
                if after < before:
                    self._error(f"monitor counter reset: {field}")
                if after > before:
                    self._latch(f"monitor event counter changed: {field}")
                    if self._active_child is None:
                        self._latch("unattributed monitor event")
            for field in _MONOTONIC_TOTAL_FIELDS:
                before = getattr(self._last_snapshot, field)
                after = getattr(snapshot, field)
                if after < before:
                    self._error(f"monitor counter reset: {field}")
            if (
                snapshot.swap_read_bytes > self._last_snapshot.swap_read_bytes
                or snapshot.swap_write_bytes > self._last_snapshot.swap_write_bytes
            ):
                self._latch("monitor event: swap")
        observation = MonitorObservation.from_snapshot(
            len(self._observations), phase, child_id, snapshot
        )
        self._observations.append(observation)
        self._last_snapshot = snapshot

    def open(self) -> None:
        if self._state != "new":
            self._error("monitor lifecycle can only be opened once")
        self._state = "open"

    def record_pre_build(self, snapshot: MonitorSnapshot) -> None:
        if self._state != "open" or self._observations:
            self._error("pre-build observation is out of order")
        self._append("pre-build", "", snapshot)

    def start_child(self, child_id: str, snapshot: MonitorSnapshot) -> None:
        _hex64(child_id, "child_id")
        if self._active_child is not None:
            self._error("cannot start a child while another child is active")
        if self._state not in ("open", "between"):
            self._error("child start is out of order")
        if not self._observations or self._observations[0].phase != "pre-build":
            self._error("child start requires the pre-build observation")
        if child_id in self._seen_child_ids:
            self._error("child_id was reused")
        expected_index = len(self._seen_child_ids)
        if self._expected_child_ids and (
            expected_index >= len(self._expected_child_ids)
            or child_id != self._expected_child_ids[expected_index]
        ):
            self._error("child_id does not match the expected execution order")
        self._seen_child_ids.add(child_id)
        self._child_order.append(child_id)
        self._active_child = child_id
        self._append("child-start", child_id, snapshot)

    def sample_child(self, snapshot: MonitorSnapshot) -> None:
        if self._active_child is None:
            self._error("child sample requires an active child")
        self._append("child-sample", self._active_child, snapshot)

    def end_child(self, snapshot: MonitorSnapshot) -> None:
        if self._active_child is None:
            self._error("child end requires an active child")
        child_id = self._active_child
        self._append("child-end", child_id, snapshot)
        self._child_ranges.append(
            ChildRange(child_id, self._observations[-1].ordinal, self._observations[-1].ordinal)
        )
        start = next(
            index
            for index in range(len(self._observations) - 1, -1, -1)
            if self._observations[index].child_id == child_id
            and self._observations[index].phase == "child-start"
        )
        self._child_ranges[-1] = ChildRange(child_id, start, self._observations[-1].ordinal)
        self._active_child = None
        self._state = "after-child"

    def record_between_children(self, snapshot: MonitorSnapshot) -> None:
        if self._active_child is not None or self._state != "after-child":
            self._error("between-children observation is out of order")
        self._append("between-children", "", snapshot)
        self._state = "between"

    def record_post_run(self, snapshot: MonitorSnapshot) -> None:
        if self._active_child is not None or self._state not in ("after-child", "between"):
            self._error("post-run observation is out of order")
        self._append("post-run", "", snapshot)
        self._state = "post"

    def record_event(self, kind: str, at_monotonic_ns: int | None = None) -> None:
        if kind not in _EVENT_KINDS:
            self._error("unknown monitor event kind")
        self._open_required()
        if at_monotonic_ns is not None:
            _uint(at_monotonic_ns, "event.monotonic_ns", _U64_MAX)
            if self._last_snapshot is None or at_monotonic_ns < self._last_snapshot.monotonic_ns:
                self._error("monitor event is outside the observed monotonic interval")
        if self._active_child is None:
            self._latch("unattributed monitor event")
        self._latch(f"monitor event: {kind}")

    def finish(self) -> MonitorResult:
        if self._state not in ("post",):
            self._error("monitor lifecycle did not reach post-run")
        if self._active_child is not None:
            self._error("monitor finished with an active child")
        if self._expected_child_ids and tuple(self._child_order) != self._expected_child_ids:
            self._error("monitor did not observe every expected child")
        validate_child_ranges(self._observations, self._child_ranges)
        if self._violations:
            self._state = "failed"
            raise MonitorLifecycleError("monitor evidence rejected: " + "; ".join(self._violations))
        self._state = "closed"
        return MonitorResult(tuple(self._observations), tuple(self._child_ranges))
