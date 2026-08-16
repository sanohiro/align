#!/usr/bin/env bash
# Deterministic owner for the evidence monitor lifecycle and child ranges.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - <<'PY'
from dataclasses import replace

from scripts.benchmark_evidence import monitor


H64 = "0" * 64
CHILD_A = "1" * 64
CHILD_B = "2" * 64


def snapshot(t, *, throttle=0, lost=0, swap_read=0, swap_write=0):
    return monitor.MonitorSnapshot(
        monotonic_ns=t,
        load_milli=10,
        cpu_pressure_total_us=20,
        memory_pressure_total_us=30,
        free_memory_bytes=4 * 1024 * 1024 * 1024,
        swap_read_bytes=swap_read,
        swap_write_bytes=swap_write,
        throttle_events=throttle,
        thermal_events=0,
        foreign_schedule_events=0,
        foreign_container_events=0,
        monitor_lost_events=lost,
        frequency_khz=3_500_000,
        temperature_millic=42_000,
        container_manifest_sha256=H64,
    )


def valid_session():
    lifecycle = monitor.MonitorLifecycle((CHILD_A, CHILD_B), max_sample_gap_ns=100)
    lifecycle.open()
    lifecycle.record_pre_build(snapshot(0))
    lifecycle.start_child(CHILD_A, snapshot(10))
    lifecycle.sample_child(snapshot(20))
    lifecycle.end_child(snapshot(30))
    lifecycle.record_between_children(snapshot(40))
    lifecycle.start_child(CHILD_B, snapshot(50))
    lifecycle.end_child(snapshot(60))
    lifecycle.record_post_run(snapshot(70))
    return lifecycle.finish()


def rejected(label, operation, fragment):
    try:
        operation()
    except monitor.MonitorLifecycleError as exc:
        assert fragment in str(exc), (label, fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid monitor lifecycle: {label}")


def clean_lifecycle():
    lifecycle = monitor.MonitorLifecycle((CHILD_A,), max_sample_gap_ns=100)
    lifecycle.open()
    lifecycle.record_pre_build(snapshot(0))
    lifecycle.start_child(CHILD_A, snapshot(10))
    lifecycle.end_child(snapshot(20))
    lifecycle.record_post_run(snapshot(30))
    return lifecycle


def reuse_child():
    lifecycle = monitor.MonitorLifecycle()
    lifecycle.open()
    lifecycle.record_pre_build(snapshot(0))
    lifecycle.start_child(CHILD_A, snapshot(10))
    lifecycle.end_child(snapshot(20))
    lifecycle.record_between_children(snapshot(30))
    lifecycle.start_child(CHILD_A, snapshot(40))


def rollback_clock():
    lifecycle = monitor.MonitorLifecycle((CHILD_A,))
    lifecycle.open()
    lifecycle.record_pre_build(snapshot(10))
    lifecycle.start_child(CHILD_A, snapshot(20))
    lifecycle.end_child(snapshot(19))


result = valid_session()
assert [item.ordinal for item in result.observations] == list(range(8))
assert [(item.child_id, item.first, item.last) for item in result.child_ranges] == [
    (CHILD_A, 1, 3),
    (CHILD_B, 5, 6),
]
assert result.observations[0].as_dict()["phase"] == "pre-build"
assert list(result.observations[0].as_dict()) == [
    "ordinal",
    "phase",
    "monotonic_ns",
    "child_id",
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
    "container_manifest_sha256",
]

rejected(
    "start before open",
    lambda: monitor.MonitorLifecycle().record_pre_build(snapshot(0)),
    "out of order",
)
rejected(
    "sample without child",
    lambda: clean_lifecycle().sample_child(snapshot(40)),
    "active child",
)
rejected(
    "child ID reuse",
    reuse_child,
    "reused",
)
rejected(
    "clock rollback",
    lambda: monitor.MonitorLifecycle((CHILD_A,)).start_child(CHILD_A, snapshot(10)),
    "out of order",
)
rejected("clock rollback", rollback_clock, "backwards")

delayed = monitor.MonitorLifecycle((CHILD_A,), max_sample_gap_ns=10)
delayed.open()
delayed.record_pre_build(snapshot(0))
delayed.start_child(CHILD_A, snapshot(11))
delayed.end_child(snapshot(12))
delayed.record_post_run(snapshot(13))
rejected("monitor delay", delayed.finish, "delay")

counter = monitor.MonitorLifecycle((CHILD_A,))
counter.open()
counter.record_pre_build(snapshot(0))
counter.start_child(CHILD_A, snapshot(10))
counter.end_child(snapshot(20, throttle=1))
counter.record_post_run(snapshot(30, throttle=1))
rejected("latched event counter", counter.finish, "counter changed")

swap = monitor.MonitorLifecycle((CHILD_A,))
swap.open()
swap.record_pre_build(snapshot(0))
swap.start_child(CHILD_A, snapshot(10, swap_read=1))
swap.end_child(snapshot(20, swap_read=1))
swap.record_post_run(snapshot(30, swap_read=1))
rejected("swap event", swap.finish, "monitor event: swap")

event = clean_lifecycle()
event.record_event("monitor_lost", at_monotonic_ns=30)
rejected("latched event", event.finish, "monitor_lost")

mutated = list(result.observations)
mutated[3] = replace(mutated[3], phase="between-children", child_id="")
rejected(
    "range mutation",
    lambda: monitor.validate_child_ranges(mutated, result.child_ranges),
    "child range",
)

print("monitor lifecycle checks passed")
PY
