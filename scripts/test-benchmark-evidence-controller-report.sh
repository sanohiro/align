#!/usr/bin/env bash
# Deterministic owner for the native measurement/controller/report handoff.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - <<'PY'
from dataclasses import replace
import hashlib

from scripts.benchmark_evidence import canonical_json as cj
from scripts.benchmark_evidence import controller
from scripts.benchmark_evidence import measurement_report
from scripts.benchmark_evidence import monitor
from scripts.benchmark_evidence import native_measurement as nm
from scripts.benchmark_evidence import report_schema as rs
from scripts.benchmark_evidence import schedule


H = "0" * 64
MANIFESTS = {
    product: f"{index + 1:064x}"
    for index, product in enumerate(nm.PRODUCTS)
}


def expect_error(call, fragment, error_type):
    try:
        call()
    except error_type as exc:
        assert fragment in str(exc), (fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid input: {fragment}")


def parsed(benchmark, revision, *, overflow=False):
    if benchmark == "json_decode":
        fields = ("A-full", "A-proj")
        values = (1_000, 800)
        ratios = ("1.00x", "1.00x")
    else:
        fields = ("soa ms", "aos ms", "proj ms")
        values = (1_000, 1_200, 800)
        ratios = ("1.00x", "1.00x", "1.00x")
    if revision == "candidate":
        values = tuple(value + 1 for value in values)
    if overflow:
        values = ((1 << 64) - 1, *values[1:])
    rows = tuple(
        nm.ParsedRow(
            records=records,
            json_kb=1,
            duration_us=values,
            ratios=ratios,
        )
        for records in (10_000, 100_000, 1_000_000)
    )
    return nm.ParsedBenchmark(
        benchmark=benchmark,
        rows=rows,
        fields=tuple(zip(fields, values)),
    )


def execution_for(plan, child_id, *, overflow=False):
    digest = MANIFESTS[(plan.benchmark, plan.revision)]
    return nm.ChildExecution(
        child_id=child_id,
        phase=plan.phase,
        benchmark=plan.benchmark,
        revision=plan.revision,
        artifact_manifest_sha256=digest,
        build_attempted=plan.phase == "prepare",
        stdout_sha256=H,
        stderr_sha256=H,
        stderr_tail_hex="",
        elapsed_ns=1_000,
        measurement=None
        if plan.phase == "prepare"
        else parsed(plan.benchmark, plan.revision, overflow=overflow),
    )


def transcript(*, missing_execution=None, overflow=False):
    records = []
    for plan in schedule.full_schedule():
        child_id = hashlib.sha256(f"child-{plan.sequence}".encode()).hexdigest()
        execution = None if plan.sequence == missing_execution else execution_for(
            plan, child_id, overflow=overflow
        )
        result = controller.ChildResult(
            child_id=child_id,
            artifact_manifest_sha256=MANIFESTS[(plan.benchmark, plan.revision)],
            build_attempted=plan.phase == "prepare",
            execution=execution,
        )
        records.append(controller.ChildExecutionRecord(plan, result))
    return controller.ExecutionTranscript(tuple(records), MANIFESTS)


def observations(transcript):
    lifecycle = monitor.MonitorLifecycle(
        [record.result.child_id for record in transcript.children]
    )
    lifecycle.open()
    clock = 1

    def snapshot():
        nonlocal clock
        value = monitor.MonitorSnapshot(
            monotonic_ns=clock,
            load_milli=0,
            cpu_pressure_total_us=0,
            memory_pressure_total_us=0,
            free_memory_bytes=1 << 30,
            swap_read_bytes=0,
            swap_write_bytes=0,
            throttle_events=0,
            thermal_events=0,
            foreign_schedule_events=0,
            foreign_container_events=0,
            monitor_lost_events=0,
            frequency_khz=1,
            temperature_millic=1,
            container_manifest_sha256=H,
        )
        clock += 1
        return value

    lifecycle.record_pre_build(snapshot())
    for index, record in enumerate(transcript.children):
        child_id = record.result.child_id
        lifecycle.start_child(child_id, snapshot())
        lifecycle.sample_child(snapshot())
        lifecycle.end_child(snapshot())
        if index + 1 < len(transcript.children):
            lifecycle.record_between_children(snapshot())
    lifecycle.record_post_run(snapshot())
    return lifecycle.finish().observations


def profile_config():
    profile = {
        "image": {
            "registry_digest": "sha256:" + "1" * 64,
            "local_image_id": H,
            "platform": "linux/amd64",
        },
        "machine": {
            "architecture": "x86_64",
            "benchmark_cpu_set": "0-7",
            "numa_set": "0",
        },
        "docker": {"cgroup_driver": "cgroupfs", "cgroup_parent": "/"},
        "capture_limits": {
            "stdout_max_bytes": 65536,
            "stderr_max_bytes": 32768,
            "stderr_tail_max_bytes": 4096,
        },
        "phase_timeouts": {
            "prepare_ns": 60_000_000_000,
            "warmup_ns": 60_000_000_000,
            "sample_ns": 60_000_000_000,
        },
    }
    return nm.NativeMeasurementConfig(
        profile=profile,
        docker_client_sha256=H,
        workspaces={
            product: nm.ChildWorkspace(
                source=f"/evidence/{index}/source",
                target=f"/evidence/{index}/target",
                work=f"/evidence/{index}/work",
                cargo_home=f"/evidence/{index}/cargo",
                toolchain=f"/evidence/{index}/toolchain",
            )
            for index, product in enumerate(nm.PRODUCTS)
        },
    )


# The stateful session is the real prepare/native join.  Replace only the
# Docker call in this owner; all schedule, digest, and no-retry state remains
# production code.
original_execute_child = nm.execute_child
session_digests = {}


def fake_execute_child(config, plan, child_id, *, runner, clock):
    result = execution_for(plan, child_id)
    session_digests[(plan.benchmark, plan.revision)] = result.artifact_manifest_sha256
    return result


nm.execute_child = fake_execute_child
try:
    session = nm.NativeMeasurementSession(profile_config())
    for plan in schedule.full_schedule():
        child_id = hashlib.sha256(f"session-{plan.sequence}".encode()).hexdigest()
        session.execute(plan, child_id)
    assert len(session.finish()) == 48
    assert all(
        session.config.workspaces[product].artifact_manifest_sha256 == MANIFESTS[product]
        for product in nm.PRODUCTS
    )
    expect_error(
        lambda: session.execute(schedule.full_schedule()[-1], "f" * 64),
        "already finished",
        nm.NativeMeasurementError,
    )

    out_of_order = nm.NativeMeasurementSession(profile_config())
    expect_error(
        lambda: out_of_order.execute(
            schedule.full_schedule()[1], "1" * 64
        ),
        "fixed schedule order",
        nm.NativeMeasurementError,
    )

    duplicate = nm.NativeMeasurementSession(profile_config())
    first_plan = schedule.full_schedule()[0]
    duplicate.execute(first_plan, "2" * 64)
    expect_error(
        lambda: duplicate.execute(schedule.full_schedule()[1], "2" * 64),
        "child_id was reused",
        nm.NativeMeasurementError,
    )

    incomplete = nm.NativeMeasurementSession(profile_config())
    incomplete.execute(first_plan, "3" * 64)
    expect_error(
        incomplete.finish,
        "before every child",
        nm.NativeMeasurementError,
    )
finally:
    nm.execute_child = original_execute_child


TRANSCRIPT = transcript()
FRAGMENTS = measurement_report.assemble(TRANSCRIPT, observations(TRANSCRIPT))
assert len(FRAGMENTS.host_observations) > 48
assert [benchmark["name"] for benchmark in FRAGMENTS.benchmarks] == list(schedule.BENCHMARKS)
assert [field["field"] for field in FRAGMENTS.fields] == list(schedule.FIELDS)
assert FRAGMENTS.verdict == "pass"
assert FRAGMENTS.first_failed_field == ""
assert FRAGMENTS.benchmarks[0]["preparations"][0]["artifact_manifest_sha256"] == MANIFESTS[("json_decode", "baseline")]
assert FRAGMENTS.benchmarks[0]["pairs"][0]["first"]["samples"][0]["token"] == "1.000"
assert FRAGMENTS.fields[0]["baseline_middle_sum"] == 2_000
assert FRAGMENTS.fields[0]["candidate_middle_sum"] == 2_002
assert FRAGMENTS.fields[0]["passed"] is True

# Every nested result is independently accepted by the typed schema owner.
for index, observation in enumerate(FRAGMENTS.host_observations):
    rs._host_observation(observation, f"host[{index}]")
for index, benchmark in enumerate(FRAGMENTS.benchmarks):
    rs._benchmark(benchmark, f"benchmark[{index}]")
for index, field in enumerate(FRAGMENTS.fields):
    rs._field_result(field, f"field[{index}]")

expect_error(
    lambda: measurement_report.assemble(
        transcript(missing_execution=0), observations(TRANSCRIPT)
    ),
    "missing native execution",
    measurement_report.MeasurementReportError,
)
bad_observations = list(observations(TRANSCRIPT))
bad_observations[1] = replace(bad_observations[1], child_id="f" * 64)
expect_error(
    lambda: measurement_report.assemble(TRANSCRIPT, bad_observations),
    "monitor lifecycle rejected",
    measurement_report.MeasurementReportError,
)
expect_error(
    lambda: measurement_report.assemble(
        transcript(overflow=True), observations(TRANSCRIPT)
    ),
    "overflows u64",
    measurement_report.MeasurementReportError,
)
bad_manifests = dict(MANIFESTS)
bad_manifests[("json_decode", "baseline")] = "f" * 64
expect_error(
    lambda: measurement_report.assemble(
        controller.ExecutionTranscript(TRANSCRIPT.children, bad_manifests),
        observations(TRANSCRIPT),
    ),
    "artifact manifests",
    measurement_report.MeasurementReportError,
)

bad_execution = replace(
    TRANSCRIPT.children[0].result.execution,
    artifact_manifest_sha256="f" * 64,
)
expect_error(
    lambda: controller.ChildResult(
        child_id=TRANSCRIPT.children[0].result.child_id,
        artifact_manifest_sha256=MANIFESTS[("json_decode", "baseline")],
        build_attempted=True,
        execution=bad_execution,
    ),
    "manifest digest",
    controller.ControllerError,
)

print("controller/report handoff checks passed")
PY
