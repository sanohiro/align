"""Typed validation and encoding for the benchmark evidence report v1.

The controller and verifier are not part of this module.  This layer fixes the
wire shape above :mod:`canonical_json`: every member order, scalar grammar,
closed enum, and declared array cardinality is checked before later semantic
and signature checks consume a report.
"""

from __future__ import annotations

import re
from typing import Any, Callable, Sequence

from . import canonical_json as cj


class ReportSchemaError(cj.CanonicalJsonError):
    """A canonical JSON value does not satisfy report schema v1."""


BODY_DIGEST_DOMAIN = b"align-json-escape-benchmark-evidence-body-v1\0"
BODY_SCHEMA = "align.json_escape_benchmark_evidence/v1"

_NAME = re.compile(r"[A-Za-z0-9._/:+=@-]{1,255}\Z")
_HEX40 = re.compile(r"[0-9a-f]{40}\Z")
_HEX64 = re.compile(r"[0-9a-f]{64}\Z")
_BYTES = re.compile(r"[0-9a-f]*\Z")
_TOKEN = re.compile(r"[0-9]+\.[0-9]{3}\Z")
_TIME = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{9}Z\Z")

_BODY_KEYS = (
    "schema",
    "profile_id",
    "profile_sha256",
    "producer",
    "verifier",
    "monitor",
    "run_id",
    "started_at",
    "ended_at",
    "baseline",
    "candidate",
    "target",
    "review",
    "protected_inputs",
    "execution",
    "host_observations",
    "benchmarks",
    "fields",
    "cleanup",
    "verdict",
    "first_failed_field",
)
_TOOL_IDENTITY_KEYS = (
    "version",
    "source_commit",
    "source_manifest_blob",
    "source_manifest_sha256",
    "executable_sha256",
)
_EXECUTABLE_IDENTITY_KEYS = ("version", "executable_sha256")
_REVISION_KEYS = (
    "commit_oid",
    "commit_sha256",
    "tree_oid",
    "tree_manifest_sha256",
    "parents",
    "commits",
    "changed_paths",
)
_COMMIT_IDENTITY_KEYS = ("oid", "raw_sha256", "tree_oid", "parents")
_PATH_IDENTITY_KEYS = ("path_hex", "mode", "kind", "oid", "size", "sha256")
_PATH_CHANGE_KEYS = ("path_hex", "status", "old", "new")
_PATH_SIDE_KEYS = ("presence", "mode", "kind", "oid", "size", "sha256")
_TARGET_KEYS = (
    "local_ref",
    "run_oid",
    "expected_merge_base",
    "expected_merge_head",
    "expected_merge_tree",
)
_REVIEW_KEYS = ("log_sha256", "review_head", "review_base", "state", "reviewer", "repair_commits")
_PROTECTED_INPUTS_KEYS = ("baseline_manifest_sha256", "candidate_manifest_sha256", "entries")
_EXECUTION_KEYS = (
    "host_id",
    "kernel",
    "cpu",
    "microcode",
    "cpu_set",
    "numa_set",
    "memory_bytes",
    "docker_client",
    "docker_daemon",
    "oci_runtime",
    "image_digest",
    "image_id",
    "image_config",
    "cargo",
    "rustc",
    "llvm",
    "cc",
    "linker",
    "cargo_cache_manifest_sha256",
    "cargo_config_sha256",
    "environment_sha256",
    "mount_manifest_sha256",
    "limit_manifest_sha256",
    "descriptor_manifest_sha256",
)
_HOST_OBSERVATION_KEYS = (
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
)
_BENCHMARK_KEYS = ("name", "prepare_argv", "argv", "preparations", "warmups", "pairs")
_PREPARATION_KEYS = (
    "child_id",
    "revision",
    "sequence",
    "stdout_sha256",
    "stderr_sha256",
    "stderr_tail_hex",
    "exit_code",
    "elapsed_ns",
    "monitor_first",
    "monitor_last",
    "artifact_manifest_sha256",
)
_PAIR_KEYS = ("ordinal", "first", "second")
_RUN_KEYS = (
    "child_id",
    "revision",
    "sequence",
    "stdout_sha256",
    "stderr_sha256",
    "stderr_tail_hex",
    "exit_code",
    "elapsed_ns",
    "monitor_first",
    "monitor_last",
    "samples",
)
_SAMPLE_KEYS = ("field", "token", "microseconds")
_FIELD_RESULT_KEYS = (
    "field",
    "baseline_tokens",
    "candidate_tokens",
    "baseline_samples_us",
    "candidate_samples_us",
    "baseline_sorted_us",
    "candidate_sorted_us",
    "baseline_middle_sum",
    "candidate_middle_sum",
    "median_denominator",
    "ratio_numerator",
    "ratio_denominator",
    "threshold_numerator",
    "threshold_denominator",
    "passed",
)
_CLEANUP_KEYS = (
    "children_remaining",
    "containers_remaining",
    "mounts_remaining",
    "fds_remaining",
    "private_dirs_remaining",
    "host_lock_held_for_signing",
    "source_manifests_unchanged",
    "cache_manifests_unchanged",
)

_PATH_KINDS = ("blob", "symlink")
_GIT_MODES = ("100644", "100755", "120000")
_CHANGE_STATUSES = ("added", "deleted", "modified")
_PRESENCES = ("absent", "present")
_REVIEW_STATES = ("clean", "fixed")
_VERDICTS = ("pass", "regression")
_REVISION_ARMS = ("baseline", "candidate")
_BENCHMARKS = ("json_decode", "json_soa")
_PREPARE_ARGV = (
    "bench/json_decode/run.sh prepare native",
    "bench/json_soa/run.sh prepare native",
)
_ARGV = ("bench/json_decode/run.sh native", "bench/json_soa/run.sh native")
_FIELDS = ("A-full", "A-proj", "soa ms", "aos ms", "proj ms")
_HOST_PHASES = ("pre-build", "child-start", "child-sample", "child-end", "between-children", "post-run")


def _error(label: str, message: str) -> None:
    raise ReportSchemaError(f"{label}: {message}")


def _object(value: Any, keys: Sequence[str], label: str) -> cj.Object:
    try:
        return cj.require_object(value, keys, label)
    except cj.CanonicalJsonError as exc:
        raise ReportSchemaError(str(exc)) from exc


def _string(value: Any, pattern: re.Pattern[str], label: str) -> str:
    try:
        value = cj.require_string(value, label)
    except cj.CanonicalJsonError as exc:
        raise ReportSchemaError(str(exc)) from exc
    if pattern.fullmatch(value) is None:
        _error(label, "has the wrong scalar grammar")
    return value


def _literal(value: Any, expected: Any, label: str) -> Any:
    if value != expected or type(value) is not type(expected):
        _error(label, f"must be {expected!r}")
    return value


def _enum(value: Any, values: Sequence[str], label: str) -> str:
    value = _string(value, re.compile(r"[\x20-\x7e]+\Z"), label)
    if value not in values:
        _error(label, "is not a declared enum value")
    return value


def _name(value: Any, label: str) -> str:
    return _string(value, _NAME, label)


def _hex(value: Any, length: int, label: str) -> str:
    return _string(value, _HEX40 if length == 40 else _HEX64, label)


def _bytes(value: Any, label: str) -> str:
    value = _string(value, _BYTES, label)
    if len(value) % 2 != 0:
        _error(label, "must contain an even number of hexadecimal digits")
    return value


def _token(value: Any, label: str) -> str:
    return _string(value, _TOKEN, label)


def _time(value: Any, label: str) -> str:
    return _string(value, _TIME, label)


def _uint(value: Any, maximum: int, label: str) -> int:
    try:
        return cj.require_uint(value, label, maximum=maximum)
    except cj.CanonicalJsonError as exc:
        raise ReportSchemaError(str(exc)) from exc


def _u32(value: Any, label: str) -> int:
    return _uint(value, (1 << 32) - 1, label)


def _u64(value: Any, label: str) -> int:
    return _uint(value, cj.MAX_U64, label)


def _bool(value: Any, label: str) -> bool:
    if type(value) is not bool:
        _error(label, "must be a boolean")
    return value


def _array(value: Any, item: Callable[[Any, str], Any], label: str, length: int | None = None) -> list[Any]:
    if not isinstance(value, list):
        _error(label, "must be an array")
    if length is not None and len(value) != length:
        _error(label, f"must contain exactly {length} members")
    for index, member in enumerate(value):
        item(member, f"{label}[{index}]")
    return value


def _hex40_value(value: Any, label: str) -> str:
    return _hex(value, 40, label)


def _hex64_value(value: Any, label: str) -> str:
    return _hex(value, 64, label)


def _empty_or(value: Any, validator: Callable[[Any, str], Any], label: str) -> Any:
    if value == "":
        _literal(value, "", label)
        return value
    return validator(value, label)


def _string_array(value: Any, pattern: Callable[[Any, str], Any], label: str, length: int) -> list[Any]:
    return _array(value, pattern, label, length=length)


def _parents(value: Any, label: str) -> list[Any]:
    return _array(value, _hex40_value, label)


def _tool_identity(value: Any, label: str) -> cj.Object:
    obj = _object(value, _TOOL_IDENTITY_KEYS, label)
    _name(obj["version"], f"{label}.version")
    _hex40_value(obj["source_commit"], f"{label}.source_commit")
    _hex40_value(obj["source_manifest_blob"], f"{label}.source_manifest_blob")
    _hex64_value(obj["source_manifest_sha256"], f"{label}.source_manifest_sha256")
    _hex64_value(obj["executable_sha256"], f"{label}.executable_sha256")
    return obj


def _executable_identity(value: Any, label: str) -> cj.Object:
    obj = _object(value, _EXECUTABLE_IDENTITY_KEYS, label)
    _name(obj["version"], f"{label}.version")
    _hex64_value(obj["executable_sha256"], f"{label}.executable_sha256")
    return obj


def _commit_identity(value: Any, label: str) -> cj.Object:
    obj = _object(value, _COMMIT_IDENTITY_KEYS, label)
    _hex40_value(obj["oid"], f"{label}.oid")
    _hex64_value(obj["raw_sha256"], f"{label}.raw_sha256")
    _hex40_value(obj["tree_oid"], f"{label}.tree_oid")
    _parents(obj["parents"], f"{label}.parents")
    return obj


def _path_side(value: Any, label: str) -> cj.Object:
    obj = _object(value, _PATH_SIDE_KEYS, label)
    presence = _enum(obj["presence"], _PRESENCES, f"{label}.presence")
    _empty_or(obj["mode"], lambda item, item_label: _enum(item, _GIT_MODES, item_label), f"{label}.mode")
    _empty_or(obj["kind"], lambda item, item_label: _enum(item, _PATH_KINDS, item_label), f"{label}.kind")
    _empty_or(obj["oid"], _hex40_value, f"{label}.oid")
    _u64(obj["size"], f"{label}.size")
    _empty_or(obj["sha256"], _hex64_value, f"{label}.sha256")
    if presence == "absent":
        for key in ("mode", "kind", "oid", "sha256"):
            _literal(obj[key], "", f"{label}.{key}")
        _literal(obj["size"], 0, f"{label}.size")
    else:
        for key in ("mode", "kind", "oid", "sha256"):
            if obj[key] == "":
                _error(f"{label}.{key}", "cannot be empty for a present path")
    return obj


def _path_identity(value: Any, label: str) -> cj.Object:
    obj = _object(value, _PATH_IDENTITY_KEYS, label)
    _bytes(obj["path_hex"], f"{label}.path_hex")
    _enum(obj["mode"], _GIT_MODES, f"{label}.mode")
    _enum(obj["kind"], _PATH_KINDS, f"{label}.kind")
    _hex40_value(obj["oid"], f"{label}.oid")
    _u64(obj["size"], f"{label}.size")
    _hex64_value(obj["sha256"], f"{label}.sha256")
    return obj


def _path_change(value: Any, label: str) -> cj.Object:
    obj = _object(value, _PATH_CHANGE_KEYS, label)
    _bytes(obj["path_hex"], f"{label}.path_hex")
    status = _enum(obj["status"], _CHANGE_STATUSES, f"{label}.status")
    old = _path_side(obj["old"], f"{label}.old")
    new = _path_side(obj["new"], f"{label}.new")
    if status == "added":
        _literal(old["presence"], "absent", f"{label}.old.presence")
        _literal(new["presence"], "present", f"{label}.new.presence")
    elif status == "deleted":
        _literal(old["presence"], "present", f"{label}.old.presence")
        _literal(new["presence"], "absent", f"{label}.new.presence")
    else:
        _literal(old["presence"], "present", f"{label}.old.presence")
        _literal(new["presence"], "present", f"{label}.new.presence")
        if tuple(old[key] for key in _PATH_SIDE_KEYS[1:]) == tuple(new[key] for key in _PATH_SIDE_KEYS[1:]):
            _error(label, "modified sides must differ")
    return obj


def _revision(value: Any, label: str) -> cj.Object:
    obj = _object(value, _REVISION_KEYS, label)
    _hex40_value(obj["commit_oid"], f"{label}.commit_oid")
    _hex64_value(obj["commit_sha256"], f"{label}.commit_sha256")
    _hex40_value(obj["tree_oid"], f"{label}.tree_oid")
    _hex64_value(obj["tree_manifest_sha256"], f"{label}.tree_manifest_sha256")
    _parents(obj["parents"], f"{label}.parents")
    _array(obj["commits"], _commit_identity, f"{label}.commits")
    _array(obj["changed_paths"], _path_change, f"{label}.changed_paths")
    return obj


def _target(value: Any, label: str) -> cj.Object:
    obj = _object(value, _TARGET_KEYS, label)
    _literal(obj["local_ref"], "refs/heads/main", f"{label}.local_ref")
    _hex40_value(obj["run_oid"], f"{label}.run_oid")
    _hex40_value(obj["expected_merge_base"], f"{label}.expected_merge_base")
    _hex40_value(obj["expected_merge_head"], f"{label}.expected_merge_head")
    _hex40_value(obj["expected_merge_tree"], f"{label}.expected_merge_tree")
    return obj


def _review(value: Any, label: str) -> cj.Object:
    obj = _object(value, _REVIEW_KEYS, label)
    _hex64_value(obj["log_sha256"], f"{label}.log_sha256")
    _hex40_value(obj["review_head"], f"{label}.review_head")
    _hex40_value(obj["review_base"], f"{label}.review_base")
    _enum(obj["state"], _REVIEW_STATES, f"{label}.state")
    _name(obj["reviewer"], f"{label}.reviewer")
    _parents(obj["repair_commits"], f"{label}.repair_commits")
    return obj


def _protected_inputs(value: Any, label: str) -> cj.Object:
    obj = _object(value, _PROTECTED_INPUTS_KEYS, label)
    _hex64_value(obj["baseline_manifest_sha256"], f"{label}.baseline_manifest_sha256")
    _hex64_value(obj["candidate_manifest_sha256"], f"{label}.candidate_manifest_sha256")
    _array(obj["entries"], _path_identity, f"{label}.entries")
    return obj


def _execution(value: Any, label: str) -> cj.Object:
    obj = _object(value, _EXECUTION_KEYS, label)
    for key in ("host_id", "kernel", "cpu", "microcode", "cpu_set", "numa_set", "docker_daemon", "oci_runtime", "image_digest"):
        _name(obj[key], f"{label}.{key}")
    _u64(obj["memory_bytes"], f"{label}.memory_bytes")
    for key in ("docker_client", "cargo", "rustc", "llvm", "cc", "linker"):
        _executable_identity(obj[key], f"{label}.{key}")
    for key in (
        "image_id",
        "image_config",
        "cargo_cache_manifest_sha256",
        "cargo_config_sha256",
        "environment_sha256",
        "mount_manifest_sha256",
        "limit_manifest_sha256",
        "descriptor_manifest_sha256",
    ):
        _hex64_value(obj[key], f"{label}.{key}")
    return obj


def _host_observation(value: Any, label: str) -> cj.Object:
    obj = _object(value, _HOST_OBSERVATION_KEYS, label)
    _u32(obj["ordinal"], f"{label}.ordinal")
    _enum(obj["phase"], _HOST_PHASES, f"{label}.phase")
    for key in (
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
        _u64(obj[key], f"{label}.{key}")
    _empty_or(obj["child_id"], _hex64_value, f"{label}.child_id")
    _hex64_value(obj["container_manifest_sha256"], f"{label}.container_manifest_sha256")
    return obj


def _revision_arm(value: Any, label: str) -> str:
    return _enum(value, _REVISION_ARMS, label)


def _preparation(value: Any, label: str) -> cj.Object:
    obj = _object(value, _PREPARATION_KEYS, label)
    _hex64_value(obj["child_id"], f"{label}.child_id")
    _revision_arm(obj["revision"], f"{label}.revision")
    _u32(obj["sequence"], f"{label}.sequence")
    _hex64_value(obj["stdout_sha256"], f"{label}.stdout_sha256")
    _hex64_value(obj["stderr_sha256"], f"{label}.stderr_sha256")
    _bytes(obj["stderr_tail_hex"], f"{label}.stderr_tail_hex")
    _u32(obj["exit_code"], f"{label}.exit_code")
    _u64(obj["elapsed_ns"], f"{label}.elapsed_ns")
    _u32(obj["monitor_first"], f"{label}.monitor_first")
    _u32(obj["monitor_last"], f"{label}.monitor_last")
    _hex64_value(obj["artifact_manifest_sha256"], f"{label}.artifact_manifest_sha256")
    return obj


def _sample(value: Any, label: str) -> cj.Object:
    obj = _object(value, _SAMPLE_KEYS, label)
    _enum(obj["field"], _FIELDS, f"{label}.field")
    _token(obj["token"], f"{label}.token")
    _u64(obj["microseconds"], f"{label}.microseconds")
    return obj


def _run(value: Any, label: str) -> cj.Object:
    obj = _object(value, _RUN_KEYS, label)
    _hex64_value(obj["child_id"], f"{label}.child_id")
    _revision_arm(obj["revision"], f"{label}.revision")
    _u32(obj["sequence"], f"{label}.sequence")
    _hex64_value(obj["stdout_sha256"], f"{label}.stdout_sha256")
    _hex64_value(obj["stderr_sha256"], f"{label}.stderr_sha256")
    _bytes(obj["stderr_tail_hex"], f"{label}.stderr_tail_hex")
    _u32(obj["exit_code"], f"{label}.exit_code")
    _u64(obj["elapsed_ns"], f"{label}.elapsed_ns")
    _u32(obj["monitor_first"], f"{label}.monitor_first")
    _u32(obj["monitor_last"], f"{label}.monitor_last")
    _array(obj["samples"], _sample, f"{label}.samples")
    return obj


def _pair(value: Any, label: str) -> cj.Object:
    obj = _object(value, _PAIR_KEYS, label)
    _u32(obj["ordinal"], f"{label}.ordinal")
    _run(obj["first"], f"{label}.first")
    _run(obj["second"], f"{label}.second")
    return obj


def _benchmark(value: Any, label: str) -> cj.Object:
    obj = _object(value, _BENCHMARK_KEYS, label)
    _enum(obj["name"], _BENCHMARKS, f"{label}.name")
    _enum(obj["prepare_argv"], _PREPARE_ARGV, f"{label}.prepare_argv")
    _enum(obj["argv"], _ARGV, f"{label}.argv")
    _array(obj["preparations"], _preparation, f"{label}.preparations", length=2)
    _array(obj["warmups"], _run, f"{label}.warmups", length=2)
    _array(obj["pairs"], _pair, f"{label}.pairs", length=10)
    return obj


def _field_result(value: Any, label: str) -> cj.Object:
    obj = _object(value, _FIELD_RESULT_KEYS, label)
    _enum(obj["field"], _FIELDS, f"{label}.field")
    _string_array(obj["baseline_tokens"], _token, f"{label}.baseline_tokens", 10)
    _string_array(obj["candidate_tokens"], _token, f"{label}.candidate_tokens", 10)
    for key in ("baseline_samples_us", "candidate_samples_us", "baseline_sorted_us", "candidate_sorted_us"):
        _array(obj[key], _u64, f"{label}.{key}", length=10)
    for key in ("baseline_middle_sum", "candidate_middle_sum", "ratio_numerator", "ratio_denominator"):
        _u64(obj[key], f"{label}.{key}")
    _literal(obj["median_denominator"], 2, f"{label}.median_denominator")
    _literal(obj["threshold_numerator"], 105, f"{label}.threshold_numerator")
    _literal(obj["threshold_denominator"], 100, f"{label}.threshold_denominator")
    _bool(obj["passed"], f"{label}.passed")
    return obj


def _cleanup(value: Any, label: str) -> cj.Object:
    obj = _object(value, _CLEANUP_KEYS, label)
    for key in ("children_remaining", "containers_remaining", "mounts_remaining", "fds_remaining", "private_dirs_remaining"):
        _u32(obj[key], f"{label}.{key}")
    for key in ("host_lock_held_for_signing", "source_manifests_unchanged", "cache_manifests_unchanged"):
        _bool(obj[key], f"{label}.{key}")
    return obj


def validate_body(value: Any) -> cj.Object:
    """Validate a decoded or programmatically constructed report body."""

    obj = _object(value, _BODY_KEYS, "body")
    _literal(obj["schema"], BODY_SCHEMA, "body.schema")
    _name(obj["profile_id"], "body.profile_id")
    _hex64_value(obj["profile_sha256"], "body.profile_sha256")
    _tool_identity(obj["producer"], "body.producer")
    _tool_identity(obj["verifier"], "body.verifier")
    _tool_identity(obj["monitor"], "body.monitor")
    _hex64_value(obj["run_id"], "body.run_id")
    _time(obj["started_at"], "body.started_at")
    _time(obj["ended_at"], "body.ended_at")
    _revision(obj["baseline"], "body.baseline")
    _revision(obj["candidate"], "body.candidate")
    _target(obj["target"], "body.target")
    _review(obj["review"], "body.review")
    _protected_inputs(obj["protected_inputs"], "body.protected_inputs")
    _execution(obj["execution"], "body.execution")
    _array(obj["host_observations"], _host_observation, "body.host_observations")
    _array(obj["benchmarks"], _benchmark, "body.benchmarks", length=2)
    _array(obj["fields"], _field_result, "body.fields", length=5)
    cleanup = _cleanup(obj["cleanup"], "body.cleanup")
    verdict = _enum(obj["verdict"], _VERDICTS, "body.verdict")
    _empty_or(obj["first_failed_field"], lambda item, item_label: _enum(item, _FIELDS, item_label), "body.first_failed_field")

    if [benchmark["name"] for benchmark in obj["benchmarks"]] != list(_BENCHMARKS):
        _error("body.benchmarks", "must use the declared benchmark order")
    if [field["field"] for field in obj["fields"]] != list(_FIELDS):
        _error("body.fields", "must use the declared field order")
    failed = [field["field"] for field in obj["fields"] if field["passed"] is False]
    expected_first_failed = failed[0] if failed else ""
    if obj["first_failed_field"] != expected_first_failed:
        _error("body.first_failed_field", "does not match the first failed field")
    if verdict == "pass" and failed:
        _error("body.verdict", "pass cannot contain a failed field")
    if verdict == "regression" and not failed:
        _error("body.verdict", "regression requires a failed field")
    for key in ("children_remaining", "containers_remaining", "mounts_remaining", "fds_remaining", "private_dirs_remaining"):
        _literal(cleanup[key], 0, f"body.cleanup.{key}")
    for key in ("host_lock_held_for_signing", "source_manifests_unchanged", "cache_manifests_unchanged"):
        _literal(cleanup[key], True, f"body.cleanup.{key}")
    return obj


def body_digest(body: Any) -> str:
    """Return the domain-separated digest of a canonical body without its LF."""

    body = validate_body(body)
    body_bytes = cj.encode(body)
    return cj.sha256(BODY_DIGEST_DOMAIN + body_bytes[:-1])


def validate_report(value: Any) -> cj.Object:
    """Validate an outer report object, including its derived body digest."""

    obj = _object(value, ("body", "body_sha256"), "report")
    body = validate_body(obj["body"])
    actual = _hex64_value(obj["body_sha256"], "report.body_sha256")
    expected = body_digest(body)
    if actual != expected:
        _error("report.body_sha256", "does not match the canonical body digest")
    return obj


def encode_report(body: Any) -> bytes:
    """Encode one validated body as the canonical outer report record."""

    body = validate_body(body)
    report = cj.Object((("body", body), ("body_sha256", body_digest(body))))
    return cj.encode(report)


def decode_report(raw: bytes) -> cj.Object:
    """Decode and validate one complete canonical report."""

    try:
        value = cj.decode(raw)
    except cj.CanonicalJsonError as exc:
        raise ReportSchemaError(str(exc)) from exc
    return validate_report(value)
