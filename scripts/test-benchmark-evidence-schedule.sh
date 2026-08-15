#!/usr/bin/env bash
# Deterministic owner for benchmark_evidence_schedule_matrix.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - <<'PY'
from dataclasses import replace

from scripts.benchmark_evidence import schedule


def expect_error(call, fragment):
    try:
        call()
    except schedule.ScheduleError as exc:
        assert fragment in str(exc), (fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid schedule transition: {fragment}")


plans = schedule.full_schedule()
assert len(plans) == 48
assert [plan.sequence for plan in plans] == list(range(48))
assert [(plan.benchmark, plan.revision) for plan in plans[:4]] == [
    ("json_decode", "baseline"),
    ("json_decode", "candidate"),
    ("json_soa", "baseline"),
    ("json_soa", "candidate"),
]
assert [(plan.phase, plan.revision) for plan in plans[4:8]] == [
    ("warmup", "baseline"),
    ("warmup", "candidate"),
    ("sample", "baseline"),
    ("sample", "candidate"),
]
assert [tuple(plan.revision for plan in plans[6 + 2 * pair : 8 + 2 * pair]) for pair in range(10)] == [
    ("baseline", "candidate") if pair % 2 == 0 else ("candidate", "baseline")
    for pair in range(10)
]
assert plans[6].pair == 1
assert plans[7].pair == 1
assert plans[8].pair == 2
assert plans[25].benchmark == "json_decode"
assert plans[26].benchmark == "json_soa"
assert plans[-1].pair == 10

state = schedule.ScheduleState(plans)
manifests = {
    ("json_decode", "baseline"): "a" * 64,
    ("json_decode", "candidate"): "b" * 64,
    ("json_soa", "baseline"): "c" * 64,
    ("json_soa", "candidate"): "d" * 64,
}
for plan in plans:
    child_id = f"{plan.sequence + 1:064x}"
    state.start(plan, child_id)
    state.finish(
        artifact_manifest_sha256=manifests[(plan.benchmark, plan.revision)],
        build_attempted=plan.phase == "prepare",
    )
state.finish_all()
assert schedule.manifest_map(state) == manifests

short = schedule.ScheduleState(plans)
expect_error(lambda: short.start(plans[1], "1" * 64), "fixed schedule order")
short.start(plans[0], "1" * 64)
expect_error(lambda: short.start(plans[1], "2" * 64), "overlap")
short.finish(
    artifact_manifest_sha256=manifests[("json_decode", "baseline")],
    build_attempted=True,
)
expect_error(lambda: short.start(plans[1], "1" * 64), "reused")
short.start(plans[1], "2" * 64)
expect_error(lambda: short.start(plans[1], "3" * 64), "overlap")
short.finish(
    artifact_manifest_sha256=manifests[("json_decode", "candidate")],
    build_attempted=True,
)

failed_build = schedule.ScheduleState(plans)
for plan in plans[:4]:
    failed_build.start(plan, f"{plan.sequence + 1:064x}")
    failed_build.finish(
        artifact_manifest_sha256=manifests[(plan.benchmark, plan.revision)],
        build_attempted=True,
    )
failed_build.start(plans[4], "5" * 64)
expect_error(
    lambda: failed_build.finish(
        artifact_manifest_sha256=manifests[("json_decode", "baseline")],
        build_attempted=True,
    ),
    "cannot be accepted",
)
expect_error(lambda: failed_build.finish_all(), "already failed")

drift = schedule.ScheduleState(plans)
for plan in plans[:4]:
    drift.start(plan, f"{plan.sequence + 1:064x}")
    drift.finish(
        artifact_manifest_sha256=manifests[(plan.benchmark, plan.revision)],
        build_attempted=True,
    )
drift.start(plans[4], "5" * 64)
expect_error(
    lambda: drift.finish(artifact_manifest_sha256="e" * 64),
    "changed during measurement",
)

expect_error(lambda: replace(plans[0], argv=("unexpected",)), "argv")
expect_error(lambda: schedule.ChildPlan(0, "sample", "json_decode", "baseline", 11, plans[6].argv), "range")

print("schedule checks passed")
PY
