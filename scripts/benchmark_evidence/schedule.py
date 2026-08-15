"""The fixed, non-overlapping benchmark child schedule."""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Mapping


class ScheduleError(ValueError):
    """A child attempted to change the evidence schedule."""


BENCHMARKS = ("json_decode", "json_soa")
FIELDS = ("A-full", "A-proj", "soa ms", "aos ms", "proj ms")
REVISIONS = ("baseline", "candidate")
_HEX64 = re.compile(r"[0-9a-f]{64}\Z")


def _child_id(value: object) -> str:
    if not isinstance(value, str) or _HEX64.fullmatch(value) is None:
        raise ScheduleError("child ID must be lowercase 64-hex")
    return value


def _benchmark(value: object) -> str:
    if value not in BENCHMARKS:
        raise ScheduleError("benchmark is not in the fixed inventory")
    return str(value)


def _revision(value: object) -> str:
    if value not in REVISIONS:
        raise ScheduleError("revision must be baseline or candidate")
    return str(value)


@dataclass(frozen=True)
class ChildPlan:
    """One exact child invocation in the controller's global sequence."""

    sequence: int
    phase: str
    benchmark: str
    revision: str
    pair: int | None
    argv: tuple[str, ...]

    def __post_init__(self) -> None:
        if type(self.sequence) is not int or self.sequence < 0:
            raise ScheduleError("child sequence must be a non-negative integer")
        if self.phase not in ("prepare", "warmup", "sample"):
            raise ScheduleError("child phase is not fixed")
        _benchmark(self.benchmark)
        _revision(self.revision)
        expected = _argv(self.benchmark, self.phase)
        if self.argv != expected:
            raise ScheduleError("child argv does not match its phase")
        if self.phase == "sample":
            if type(self.pair) is not int or not 1 <= self.pair <= 10:
                raise ScheduleError("sample pair must be in the range 1..10")
        elif self.pair is not None:
            raise ScheduleError("prepare and warmup children cannot carry a pair")


def _argv(benchmark: str, phase: str) -> tuple[str, ...]:
    script = f"bench/{benchmark}/run.sh"
    return (script, "prepare", "native") if phase == "prepare" else (script, "native")


def _plans_for(benchmark: str, start: int, *, include_prepare: bool) -> list[ChildPlan]:
    plans: list[ChildPlan] = []
    sequence = start
    if include_prepare:
        for revision in REVISIONS:
            plans.append(
                ChildPlan(sequence, "prepare", benchmark, revision, None, _argv(benchmark, "prepare"))
            )
            sequence += 1
    for revision in REVISIONS:
        plans.append(
            ChildPlan(sequence, "warmup", benchmark, revision, None, _argv(benchmark, "warmup"))
        )
        sequence += 1
    for pair in range(1, 11):
        order = REVISIONS if pair % 2 else ("candidate", "baseline")
        for revision in order:
            plans.append(
                ChildPlan(sequence, "sample", benchmark, revision, pair, _argv(benchmark, "sample"))
            )
            sequence += 1
    return plans


def full_schedule() -> tuple[ChildPlan, ...]:
    """Return all four preparations, then each benchmark's measurements."""

    plans: list[ChildPlan] = []
    sequence = 0
    for benchmark in BENCHMARKS:
        for revision in REVISIONS:
            plans.append(
                ChildPlan(sequence, "prepare", benchmark, revision, None, _argv(benchmark, "prepare"))
            )
            sequence += 1
    for benchmark in BENCHMARKS:
        measured = _plans_for(benchmark, sequence, include_prepare=False)
        plans.extend(measured)
        sequence += len(measured)
    return tuple(plans)


@dataclass
class ScheduleState:
    """Consume the fixed schedule exactly once, without overlap or retries."""

    plans: tuple[ChildPlan, ...] = full_schedule()

    def __post_init__(self) -> None:
        if self.plans != full_schedule():
            raise ScheduleError("schedule must equal the fixed evidence child sequence")
        self._cursor = 0
        self._active: tuple[ChildPlan, str] | None = None
        self._seen_ids: set[str] = set()
        self._manifests: dict[tuple[str, str], str] = {}
        self._failed = False

    @property
    def next_plan(self) -> ChildPlan | None:
        return self.plans[self._cursor] if self._cursor < len(self.plans) else None

    @property
    def active(self) -> tuple[ChildPlan, str] | None:
        return self._active

    def start(self, plan: ChildPlan, child_id: str) -> None:
        if self._failed:
            raise ScheduleError("schedule is already failed")
        if self._active is not None:
            raise ScheduleError("a second child would overlap the active child")
        expected = self.next_plan
        if expected is None or plan != expected:
            raise ScheduleError("child is out of the fixed schedule order")
        child_id = _child_id(child_id)
        if child_id in self._seen_ids:
            raise ScheduleError("child ID was reused")
        if plan.phase in ("warmup", "sample") and (
            plan.benchmark,
            plan.revision,
        ) not in self._manifests:
            raise ScheduleError("sample started before its prepared artifact was sealed")
        self._seen_ids.add(child_id)
        self._active = (plan, child_id)

    def finish(
        self,
        *,
        exit_code: int | None = 0,
        signal: int | None = None,
        timed_out: bool = False,
        truncated: bool = False,
        artifact_manifest_sha256: str | None = None,
        build_attempted: bool = False,
    ) -> None:
        if self._failed:
            raise ScheduleError("schedule is already failed")
        if self._active is None:
            raise ScheduleError("no child is active")
        plan, _child = self._active
        if (
            exit_code != 0
            or signal is not None
            or timed_out
            or truncated
            or plan.phase == "prepare" and not build_attempted
            or plan.phase != "prepare" and build_attempted
        ):
            self._failed = True
            raise ScheduleError("child result cannot be accepted for this schedule")
        if not isinstance(artifact_manifest_sha256, str) or _HEX64.fullmatch(
            artifact_manifest_sha256
        ) is None:
            self._failed = True
            raise ScheduleError("child result requires a lowercase artifact manifest digest")
        key = (plan.benchmark, plan.revision)
        if plan.phase == "prepare":
            if key in self._manifests:
                self._failed = True
                raise ScheduleError("benchmark revision was prepared more than once")
            self._manifests[key] = artifact_manifest_sha256
        elif artifact_manifest_sha256 != self._manifests[key]:
            self._failed = True
            raise ScheduleError("prepared artifact manifest changed during measurement")
        self._active = None
        self._cursor += 1

    def finish_all(self) -> None:
        if self._failed:
            raise ScheduleError("schedule is already failed")
        if self._active is not None or self._cursor != len(self.plans):
            raise ScheduleError("schedule ended before every fixed child completed")
        expected = {(benchmark, revision) for benchmark in BENCHMARKS for revision in REVISIONS}
        if set(self._manifests) != expected:
            raise ScheduleError("not every benchmark revision has a sealed artifact")


def manifest_map(state: ScheduleState) -> Mapping[tuple[str, str], str]:
    """Expose a read-only copy for report construction."""

    return dict(state._manifests)
