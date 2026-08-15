#!/usr/bin/env bash
# Deterministic owner for the typed benchmark evidence report schema.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
import hashlib
import sys

from scripts.benchmark_evidence import canonical_json as cj
from scripts.benchmark_evidence import report_schema as rs


def O(*pairs):
    return cj.Object(pairs)


H40 = "0" * 40
H64 = "0" * 64
H64_ONE = "1" * 64


def tool(version):
    return O(
        ("version", version),
        ("source_commit", H40),
        ("source_manifest_blob", H40),
        ("source_manifest_sha256", H64),
        ("executable_sha256", H64),
    )


def executable(version):
    return O(("version", version), ("executable_sha256", H64))


def path_side(presence, *, size=0, sha=H64, mode="100644", kind="blob", oid=H40):
    if presence == "absent":
        return O(
            ("presence", "absent"),
            ("mode", ""),
            ("kind", ""),
            ("oid", ""),
            ("size", 0),
            ("sha256", ""),
        )
    return O(
        ("presence", "present"),
        ("mode", mode),
        ("kind", kind),
        ("oid", oid),
        ("size", size),
        ("sha256", sha),
    )


def revision(candidate):
    commit = O(("oid", H40), ("raw_sha256", H64), ("tree_oid", H40), ("parents", [H40]))
    if not candidate:
        return O(
            ("commit_oid", H40),
            ("commit_sha256", H64),
            ("tree_oid", H40),
            ("tree_manifest_sha256", H64),
            ("parents", []),
            ("commits", []),
            ("changed_paths", []),
        )
    change = O(
        ("path_hex", "616263"),
        ("status", "modified"),
        ("old", path_side("present", size=1, sha=H64)),
        ("new", path_side("present", size=2, sha=H64_ONE, oid="1" * 40)),
    )
    return O(
        ("commit_oid", H40),
        ("commit_sha256", H64_ONE),
        ("tree_oid", "1" * 40),
        ("tree_manifest_sha256", H64_ONE),
        ("parents", [H40]),
        ("commits", [commit]),
        ("changed_paths", [change]),
    )


def run(child_id, revision_arm, sequence):
    return O(
        ("child_id", child_id),
        ("revision", revision_arm),
        ("sequence", sequence),
        ("stdout_sha256", H64),
        ("stderr_sha256", H64),
        ("stderr_tail_hex", "00"),
        ("exit_code", 0),
        ("elapsed_ns", 1_000_000),
        ("monitor_first", 0),
        ("monitor_last", 0),
        ("samples", []),
    )


def preparation(child_id, revision_arm, sequence):
    return O(
        ("child_id", child_id),
        ("revision", revision_arm),
        ("sequence", sequence),
        ("stdout_sha256", H64),
        ("stderr_sha256", H64),
        ("stderr_tail_hex", "00"),
        ("exit_code", 0),
        ("elapsed_ns", 1_000_000),
        ("monitor_first", 0),
        ("monitor_last", 0),
        ("artifact_manifest_sha256", H64),
    )


def benchmark(name, prepare_argv, argv, prefix):
    preparations = [preparation(f"{prefix}{i:063x}", arm, i) for i, arm in enumerate(("baseline", "candidate"), 1)]
    warmups = [run(f"{prefix}{i:063x}", arm, i) for i, arm in enumerate(("baseline", "candidate"), 3)]
    pairs = []
    for ordinal in range(1, 11):
        first_arm, second_arm = (("baseline", "candidate") if ordinal % 2 else ("candidate", "baseline"))
        pairs.append(
            O(
                ("ordinal", ordinal),
                ("first", run(f"{prefix}{ordinal * 2 + 5:063x}", first_arm, ordinal * 2)),
                ("second", run(f"{prefix}{ordinal * 2 + 6:063x}", second_arm, ordinal * 2 + 1)),
            )
        )
    return O(
        ("name", name),
        ("prepare_argv", prepare_argv),
        ("argv", argv),
        ("preparations", preparations),
        ("warmups", warmups),
        ("pairs", pairs),
    )


def field_result(field, passed=True):
    tokens = ["1.000"] * 10
    samples = [1_000_000] * 10
    return O(
        ("field", field),
        ("baseline_tokens", tokens),
        ("candidate_tokens", list(tokens)),
        ("baseline_samples_us", list(samples)),
        ("candidate_samples_us", list(samples)),
        ("baseline_sorted_us", list(samples)),
        ("candidate_sorted_us", list(samples)),
        ("baseline_middle_sum", 2_000_000),
        ("candidate_middle_sum", 2_000_000),
        ("median_denominator", 2),
        ("ratio_numerator", 2_000_000),
        ("ratio_denominator", 2_000_000),
        ("threshold_numerator", 105),
        ("threshold_denominator", 100),
        ("passed", passed),
    )


def body():
    return O(
        ("schema", rs.BODY_SCHEMA),
        ("profile_id", "native-v1"),
        ("profile_sha256", H64),
        ("producer", tool("producer")),
        ("verifier", tool("verifier")),
        ("monitor", tool("monitor")),
        ("run_id", H64),
        ("started_at", "2026-08-15T00:00:00.000000000Z"),
        ("ended_at", "2026-08-15T00:00:01.000000000Z"),
        ("baseline", revision(False)),
        ("candidate", revision(True)),
        ("target", O(
            ("local_ref", "refs/heads/main"),
            ("run_oid", H40),
            ("expected_merge_base", H40),
            ("expected_merge_head", H40),
            ("expected_merge_tree", H40),
        )),
        ("review", O(
            ("log_sha256", H64),
            ("review_head", H40),
            ("review_base", H40),
            ("state", "clean"),
            ("reviewer", "codex"),
            ("repair_commits", []),
        )),
        ("protected_inputs", O(
            ("baseline_manifest_sha256", H64),
            ("candidate_manifest_sha256", H64),
            ("entries", [O(
                ("path_hex", "616263"),
                ("mode", "100644"),
                ("kind", "blob"),
                ("oid", H40),
                ("size", 3),
                ("sha256", H64),
            )]),
        )),
        ("execution", O(
            ("host_id", "host-1"),
            ("kernel", "linux"),
            ("cpu", "x86_64"),
            ("microcode", "0x1"),
            ("cpu_set", "0-1"),
            ("numa_set", "0"),
            ("memory_bytes", 1 << 30),
            ("docker_client", executable("docker")),
            ("docker_daemon", "daemon"),
            ("oci_runtime", "runc"),
            ("image_digest", "sha256:" + H64),
            ("image_id", H64),
            ("image_config", H64),
            ("cargo", executable("cargo")),
            ("rustc", executable("rustc")),
            ("llvm", executable("llvm")),
            ("cc", executable("cc")),
            ("linker", executable("lld")),
            ("cargo_cache_manifest_sha256", H64),
            ("cargo_config_sha256", H64),
            ("environment_sha256", H64),
            ("mount_manifest_sha256", H64),
            ("limit_manifest_sha256", H64),
            ("descriptor_manifest_sha256", H64),
        )),
        ("host_observations", []),
        ("benchmarks", [
            benchmark("json_decode", "bench/json_decode/run.sh prepare native", "bench/json_decode/run.sh native", "a"),
            benchmark("json_soa", "bench/json_soa/run.sh prepare native", "bench/json_soa/run.sh native", "b"),
        ]),
        ("fields", [field_result(field) for field in ("A-full", "A-proj", "soa ms", "aos ms", "proj ms")]),
        ("cleanup", O(
            ("children_remaining", 0),
            ("containers_remaining", 0),
            ("mounts_remaining", 0),
            ("fds_remaining", 0),
            ("private_dirs_remaining", 0),
            ("host_lock_held_for_signing", True),
            ("source_manifests_unchanged", True),
            ("cache_manifests_unchanged", True),
        )),
        ("verdict", "pass"),
        ("first_failed_field", ""),
    )


def replace(value, key, member):
    return O(*((name, member if name == key else current) for name, current in value.pairs))


def rejected(label, action):
    try:
        action()
    except rs.ReportSchemaError:
        return
    raise AssertionError(f"{label} was accepted")


valid_body = body()
rs.validate_body(valid_body)
report = rs.encode_report(valid_body)
assert report.endswith(b"\n")
assert b"\\" not in report
assert rs.body_digest(valid_body) == "6ec6977787caa144340e11a99540b1dd5bd6fef74441a9397559a5a94307d343"
assert hashlib.sha256(report).hexdigest() == "caff6686e7ac7388712e6aad1600e8bea75904e898c5e731e92d42a517ac4237"
assert rs.decode_report(report) == O(("body", valid_body), ("body_sha256", rs.body_digest(valid_body)))
stale_body = O(*valid_body.pairs)
stale_body["unexpected"] = 1
rejected("stale report body mapping", lambda: rs.validate_body(stale_body))

rejected("wrong outer member order", lambda: rs.decode_report(cj.encode(O(("body_sha256", rs.body_digest(valid_body)), ("body", valid_body)))))
rejected("body digest mutation", lambda: rs.validate_report(replace(O(("body", valid_body), ("body_sha256", rs.body_digest(valid_body))), "body_sha256", H64_ONE)))
rejected("wrong body member order", lambda: rs.validate_body(O(("profile_id", "native-v1"), *valid_body.pairs[0:1], *valid_body.pairs[2:])))
rejected("benchmark cardinality", lambda: rs.validate_body(replace(valid_body, "benchmarks", valid_body["benchmarks"][:1])))
rejected("benchmark order", lambda: rs.validate_body(replace(valid_body, "benchmarks", list(reversed(valid_body["benchmarks"])))) )
rejected("field order", lambda: rs.validate_body(replace(valid_body, "fields", list(reversed(valid_body["fields"])))) )
rejected("u32 overflow", lambda: rs.validate_body(replace(valid_body, "benchmarks", [replace(valid_body["benchmarks"][0], "preparations", [replace(valid_body["benchmarks"][0]["preparations"][0], "sequence", 2**32), valid_body["benchmarks"][0]["preparations"][1]]), valid_body["benchmarks"][1]])))
rejected("invalid hex", lambda: rs.validate_body(replace(valid_body, "profile_sha256", H64_ONE[:-1] + "g")))
rejected("u64 overflow", lambda: rs.validate_body(replace(valid_body, "fields", [replace(valid_body["fields"][0], "baseline_middle_sum", 2**64)] + valid_body["fields"][1:])))
rejected("absent path metadata", lambda: rs.validate_body(replace(valid_body, "baseline", replace(valid_body["baseline"], "changed_paths", [O(
    ("path_hex", "616263"),
    ("status", "deleted"),
    ("old", path_side("absent")),
    ("new", path_side("absent")),
)]))))
rejected("unchanged modified path", lambda: rs.validate_body(replace(valid_body, "candidate", replace(valid_body["candidate"], "changed_paths", [O(
    ("path_hex", "616263"),
    ("status", "modified"),
    ("old", path_side("present", size=1)),
    ("new", path_side("present", size=1)),
)]))))
rejected("regression without failed field", lambda: rs.validate_body(replace(valid_body, "verdict", "regression")))
failed_body = replace(valid_body, "verdict", "regression")
failed_fields = list(failed_body["fields"])
failed_fields[2] = replace(failed_fields[2], "passed", False)
failed_body = replace(failed_body, "fields", failed_fields)
failed_body = replace(failed_body, "first_failed_field", "soa ms")
rs.validate_body(failed_body)
rejected("wrong first regression field", lambda: rs.validate_body(replace(failed_body, "first_failed_field", "A-full")))

print("typed evidence report checks passed")
PY
