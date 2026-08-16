"""Turn the trusted child transcript into canonical benchmark report fragments.

The controller owns execution order and artifact manifests.  The monitor owns
the lifecycle replay.  This module joins those two trusted inputs and derives
only the nested benchmark portions of report v1; revision, identity, signing,
publication, and merge-verification fields stay with their existing owners.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Mapping, Sequence

from . import canonical_json as cj
from . import controller
from . import monitor
from . import native_measurement
from . import schedule


class MeasurementReportError(ValueError):
    """Trusted execution facts cannot form a report fragment."""


_U64_MAX = cj.MAX_U64


def _error(message: str) -> None:
    raise MeasurementReportError(message)


def _checked_add(left: int, right: int, label: str) -> int:
    if type(left) is not int or type(right) is not int or left < 0 or right < 0:
        _error(f"{label} has a non-u64 operand")
    if left > _U64_MAX - right:
        _error(f"{label} overflows u64")
    return left + right


def _checked_mul(left: int, right: int, label: str) -> int:
    if type(left) is not int or type(right) is not int or left < 0 or right < 0:
        _error(f"{label} has a non-u64 operand")
    if left and right > _U64_MAX // left:
        _error(f"{label} overflows u64")
    return left * right


def _token(microseconds: int) -> str:
    if type(microseconds) is not int or not 0 < microseconds <= _U64_MAX:
        _error("sample microseconds must be a positive u64")
    whole, fraction = divmod(microseconds, 1_000)
    return f"{whole}.{fraction:03d}"


def _sample_objects(execution: native_measurement.ChildExecution) -> tuple[cj.Object, ...]:
    measurement = execution.measurement
    if not isinstance(measurement, native_measurement.ParsedBenchmark):
        _error("warm-up or sample child is missing parsed measurement")
    expected_fields = (
        schedule.FIELDS[:2]
        if execution.benchmark == "json_decode"
        else schedule.FIELDS[2:]
    )
    if tuple(name for name, _value in measurement.fields) != expected_fields:
        _error("parsed measurement fields do not match the fixed benchmark order")
    return tuple(
        cj.Object(
            (
                ("field", field),
                ("token", _token(microseconds)),
                ("microseconds", microseconds),
            )
        )
        for field, microseconds in measurement.fields
    )


def _record(
    record: controller.ChildExecutionRecord,
    child_range: monitor.ChildRange,
) -> cj.Object:
    plan = record.plan
    result = record.result
    execution = result.execution
    if not isinstance(execution, native_measurement.ChildExecution):
        _error("controller transcript is missing native execution facts")
    if (
        execution.child_id != result.child_id
        or execution.phase != plan.phase
        or execution.benchmark != plan.benchmark
        or execution.revision != plan.revision
        or execution.artifact_manifest_sha256 != result.artifact_manifest_sha256
        or execution.build_attempted != result.build_attempted
    ):
        _error("child execution facts do not match the controller result")
    if (
        result.exit_code != 0
        or result.signal is not None
        or result.timed_out
        or result.truncated
    ):
        _error("a failed child cannot enter a report fragment")
    if child_range.child_id != result.child_id:
        _error("monitor range is bound to a different child")

    members: list[tuple[str, Any]] = [
        ("child_id", result.child_id),
        ("revision", plan.revision),
        ("sequence", plan.sequence),
        ("stdout_sha256", execution.stdout_sha256),
        ("stderr_sha256", execution.stderr_sha256),
        ("stderr_tail_hex", execution.stderr_tail_hex),
        ("exit_code", result.exit_code),
        ("elapsed_ns", execution.elapsed_ns),
        ("monitor_first", child_range.first),
        ("monitor_last", child_range.last),
    ]
    if plan.phase == "prepare":
        if execution.measurement is not None:
            _error("prepare child contains a native measurement")
        members.append(("artifact_manifest_sha256", result.artifact_manifest_sha256))
    else:
        members.append(("samples", list(_sample_objects(execution))))
    return cj.Object(members)


def _field_results(
    sample_records: Mapping[str, Mapping[str, list[cj.Object]]],
) -> tuple[cj.Object, ...]:
    result: list[cj.Object] = []
    for field in schedule.FIELDS:
        values = sample_records.get(field)
        if values is None:
            _error("report field is missing from sample records")
        baseline_records = values.get("baseline", [])
        candidate_records = values.get("candidate", [])
        if len(baseline_records) != 10 or len(candidate_records) != 10:
            _error("report field does not have ten samples per revision")
        baseline_tokens = [sample["token"] for sample in baseline_records]
        candidate_tokens = [sample["token"] for sample in candidate_records]
        baseline_samples = [sample["microseconds"] for sample in baseline_records]
        candidate_samples = [sample["microseconds"] for sample in candidate_records]
        baseline_sorted = sorted(baseline_samples)
        candidate_sorted = sorted(candidate_samples)
        baseline_middle = _checked_add(
            baseline_sorted[4], baseline_sorted[5], "baseline middle sum"
        )
        candidate_middle = _checked_add(
            candidate_sorted[4], candidate_sorted[5], "candidate middle sum"
        )
        left = _checked_mul(candidate_middle, 100, "candidate threshold product")
        right = _checked_mul(baseline_middle, 105, "baseline threshold product")
        result.append(
            cj.Object(
                (
                    ("field", field),
                    ("baseline_tokens", baseline_tokens),
                    ("candidate_tokens", candidate_tokens),
                    ("baseline_samples_us", baseline_samples),
                    ("candidate_samples_us", candidate_samples),
                    ("baseline_sorted_us", baseline_sorted),
                    ("candidate_sorted_us", candidate_sorted),
                    ("baseline_middle_sum", baseline_middle),
                    ("candidate_middle_sum", candidate_middle),
                    ("median_denominator", 2),
                    ("ratio_numerator", candidate_middle),
                    ("ratio_denominator", baseline_middle),
                    ("threshold_numerator", 105),
                    ("threshold_denominator", 100),
                    ("passed", left <= right),
                )
            )
        )
    return tuple(result)


@dataclass(frozen=True)
class MeasurementReportFragments:
    """Canonical nested report values derived from trusted execution facts."""

    host_observations: tuple[cj.Object, ...]
    benchmarks: tuple[cj.Object, ...]
    fields: tuple[cj.Object, ...]
    verdict: str
    first_failed_field: str

    def __post_init__(self) -> None:
        for label, value in (
            ("host_observations", self.host_observations),
            ("benchmarks", self.benchmarks),
            ("fields", self.fields),
        ):
            if not isinstance(value, tuple) or any(not isinstance(item, cj.Object) for item in value):
                _error(f"{label} has the wrong canonical object type")
        if len(self.benchmarks) != 2 or len(self.fields) != 5:
            _error("measurement fragments have the wrong fixed cardinality")
        if self.verdict not in ("pass", "regression"):
            _error("measurement verdict is not declared")
        if self.first_failed_field not in ("", *schedule.FIELDS):
            _error("first failed field is not declared")


def assemble(
    transcript: controller.ExecutionTranscript,
    observations: Sequence[monitor.MonitorObservation],
) -> MeasurementReportFragments:
    """Validate and assemble report fragments for one complete child run."""

    if not isinstance(transcript, controller.ExecutionTranscript):
        _error("measurement report requires a controller execution transcript")
    expected_ids = tuple(record.result.child_id for record in transcript.children)
    try:
        monitor_result = monitor.validate_report_observations(observations, expected_ids)
    except monitor.MonitorLifecycleError as exc:
        raise MeasurementReportError(f"monitor lifecycle rejected: {exc}") from exc
    ranges = {child_range.child_id: child_range for child_range in monitor_result.child_ranges}
    if len(ranges) != len(expected_ids):
        _error("monitor ranges do not cover every child")

    grouped: dict[str, dict[str, list[tuple[schedule.ChildPlan, cj.Object]]]] = {
        benchmark: {"preparations": [], "warmups": [], "samples": []}
        for benchmark in schedule.BENCHMARKS
    }
    sample_records: dict[str, dict[str, list[cj.Object]]] = {
        field: {"baseline": [], "candidate": []} for field in schedule.FIELDS
    }
    preparation_manifests: dict[tuple[str, str], str] = {}
    for record in transcript.children:
        product = (record.plan.benchmark, record.plan.revision)
        sealed_digest = transcript.manifests.get(product)
        if sealed_digest is None:
            _error("child product has no sealed artifact manifest")
        if record.result.artifact_manifest_sha256 != sealed_digest:
            _error("child artifact manifest changed after preparation")
        child_range = ranges.get(record.result.child_id)
        if child_range is None:
            _error("child has no monitor range")
        report_record = _record(record, child_range)
        grouped[record.plan.benchmark][
            "preparations" if record.plan.phase == "prepare" else
            "warmups" if record.plan.phase == "warmup" else "samples"
        ].append((record.plan, report_record))
        if record.plan.phase == "prepare":
            key = (record.plan.benchmark, record.plan.revision)
            if key in preparation_manifests:
                _error("benchmark revision was prepared more than once")
            preparation_manifests[key] = record.result.artifact_manifest_sha256
        if record.plan.phase == "sample":
            for sample in report_record["samples"]:
                sample_records[sample["field"]][record.plan.revision].append(sample)
    if preparation_manifests != dict(transcript.manifests):
        _error("prepared artifact manifests do not match the controller transcript")

    benchmarks: list[cj.Object] = []
    for benchmark in schedule.BENCHMARKS:
        groups = grouped[benchmark]
        if len(groups["preparations"]) != 2 or len(groups["warmups"]) != 2 or len(groups["samples"]) != 20:
            _error("benchmark child partition is incomplete")
        preparation_values = [record for _plan, record in groups["preparations"]]
        warmup_values = [record for _plan, record in groups["warmups"]]
        pairs: list[cj.Object] = []
        for ordinal in range(1, 11):
            pair_values = [
                record
                for plan, record in groups["samples"]
                if plan.pair == ordinal
            ]
            if len(pair_values) != 2:
                _error("benchmark pair does not contain exactly two children")
            pairs.append(
                cj.Object(
                    (
                        ("ordinal", ordinal),
                        ("first", pair_values[0]),
                        ("second", pair_values[1]),
                    )
                )
            )
        benchmarks.append(
            cj.Object(
                (
                    ("name", benchmark),
                    ("prepare_argv", f"bench/{benchmark}/run.sh prepare native"),
                    ("argv", f"bench/{benchmark}/run.sh native"),
                    ("preparations", preparation_values),
                    ("warmups", warmup_values),
                    ("pairs", pairs),
                )
            )
        )

    fields = _field_results(sample_records)
    failed = [field["field"] for field in fields if field["passed"] is False]
    observations_value = tuple(
        cj.Object(tuple(observation.as_dict().items()))
        for observation in monitor_result.observations
    )
    return MeasurementReportFragments(
        host_observations=observations_value,
        benchmarks=tuple(benchmarks),
        fields=fields,
        verdict="regression" if failed else "pass",
        first_failed_field=failed[0] if failed else "",
    )
