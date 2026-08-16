#!/usr/bin/env bash
# Deterministic owner for canonical post-merge verification.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
from __future__ import annotations

import hashlib
import struct

from scripts.benchmark_evidence import canonical_json as cj
from scripts.benchmark_evidence import git_objects
from scripts.benchmark_evidence import merge_verification as mv
from scripts.benchmark_evidence import report_schema as rs
from scripts.benchmark_evidence import schedule
from scripts.benchmark_evidence import sshsig
from scripts.benchmark_evidence import verifier


H = "0" * 64
H1 = "1" * 64
PROFILE = "2" * 64


def O(*pairs):
    return cj.Object(pairs)


def replace(value, key, member):
    return O(*((name, member if name == key else current) for name, current in value.pairs))


def raw(kind: str, payload: bytes):
    encoded = git_objects.encode(kind, payload)
    try:
        oid = hashlib.sha1(encoded, usedforsecurity=False).hexdigest()
    except TypeError:
        oid = hashlib.sha1(encoded).hexdigest()
    return oid, git_objects.verify(oid, encoded)


def commit(tree_oid: str, parents: tuple[str, ...], message: bytes):
    payload = (
        b"tree " + tree_oid.encode("ascii") + b"\n"
        + b"".join(b"parent " + parent.encode("ascii") + b"\n" for parent in parents)
        + b"author Align <align@example.com> 0 +0000\n"
        + b"committer Align <align@example.com> 0 +0000\n\n"
        + message
    )
    return raw("commit", payload)


def executable(version: str, digest: str = H):
    return O(("version", version), ("executable_sha256", digest))


def tool(version: str, digest: str = H):
    return O(
        ("version", version),
        ("source_commit", BASE),
        ("source_manifest_blob", BASE),
        ("source_manifest_sha256", H),
        ("executable_sha256", digest),
    )


def path_side(*, oid: str, size: int, digest: str):
    return O(
        ("presence", "present"),
        ("mode", "100644"),
        ("kind", "blob"),
        ("oid", oid),
        ("size", size),
        ("sha256", digest),
    )


def observation(ordinal: int, phase: str, child_id: str):
    return O(
        ("ordinal", ordinal),
        ("phase", phase),
        ("monotonic_ns", ordinal + 1),
        ("child_id", child_id),
        ("load_milli", 0),
        ("cpu_pressure_total_us", 0),
        ("memory_pressure_total_us", 0),
        ("free_memory_bytes", 1 << 30),
        ("swap_read_bytes", 0),
        ("swap_write_bytes", 0),
        ("throttle_events", 0),
        ("thermal_events", 0),
        ("foreign_schedule_events", 0),
        ("foreign_container_events", 0),
        ("monitor_lost_events", 0),
        ("frequency_khz", 1),
        ("temperature_millic", 1),
        ("container_manifest_sha256", H),
    )


def preparation(child_id: str, arm: str, sequence: int, first: int, last: int):
    return O(
        ("child_id", child_id),
        ("revision", arm),
        ("sequence", sequence),
        ("stdout_sha256", H),
        ("stderr_sha256", H),
        ("stderr_tail_hex", "00"),
        ("exit_code", 0),
        ("elapsed_ns", 1_000_000),
        ("monitor_first", first),
        ("monitor_last", last),
        ("artifact_manifest_sha256", H),
    )


def measured(child_id: str, arm: str, sequence: int, first: int, last: int, benchmark: str):
    fields = schedule.FIELDS[:2] if benchmark == "json_decode" else schedule.FIELDS[2:]
    samples = []
    for index, field in enumerate(fields):
        microseconds = 1_000_000 + index * 100 + (1 if arm == "candidate" else 0)
        samples.append(
            O(
                ("field", field),
                ("token", f"{microseconds // 1_000}.{microseconds % 1_000:03d}"),
                ("microseconds", microseconds),
            )
        )
    return O(
        ("child_id", child_id),
        ("revision", arm),
        ("sequence", sequence),
        ("stdout_sha256", H),
        ("stderr_sha256", H),
        ("stderr_tail_hex", "00"),
        ("exit_code", 0),
        ("elapsed_ns", 1_000_000),
        ("monitor_first", first),
        ("monitor_last", last),
        ("samples", samples),
    )


def benchmarks_and_observations():
    grouped = {
        name: {"preparations": [], "warmups": [], "pairs": []}
        for name in schedule.BENCHMARKS
    }
    observations = [observation(0, "pre-build", "")]
    for index, plan in enumerate(schedule.full_schedule()):
        prefix = "a" if plan.benchmark == "json_decode" else "b"
        child_id = f"{prefix}{plan.sequence:063x}"
        first = len(observations)
        observations.append(observation(first, "child-start", child_id))
        last = len(observations)
        observations.append(observation(last, "child-end", child_id))
        if plan.phase == "prepare":
            value = preparation(child_id, plan.revision, plan.sequence, first, last)
            grouped[plan.benchmark]["preparations"].append(value)
        else:
            value = measured(child_id, plan.revision, plan.sequence, first, last, plan.benchmark)
            grouped[plan.benchmark]["warmups" if plan.phase == "warmup" else "pairs"].append(value)
        if index + 1 < len(schedule.full_schedule()):
            observations.append(observation(len(observations), "between-children", ""))
    observations.append(observation(len(observations), "post-run", ""))
    result = []
    for name in schedule.BENCHMARKS:
        values = grouped[name]
        pairs = []
        for ordinal in range(1, 11):
            pairs.append(
                O(
                    ("ordinal", ordinal),
                    ("first", values["pairs"][(ordinal - 1) * 2]),
                    ("second", values["pairs"][(ordinal - 1) * 2 + 1]),
                )
            )
        result.append(
            O(
                ("name", name),
                ("prepare_argv", f"bench/{name}/run.sh prepare native"),
                ("argv", f"bench/{name}/run.sh native"),
                ("preparations", values["preparations"]),
                ("warmups", values["warmups"]),
                ("pairs", pairs),
            )
        )
    return result, observations


def field_results(benchmarks):
    values = {}
    for benchmark in benchmarks:
        for pair in benchmark["pairs"]:
            for run in (pair["first"], pair["second"]):
                for sample in run["samples"]:
                    field = values.setdefault(
                        sample["field"],
                        {"baseline": [], "candidate": [], "baseline_tokens": [], "candidate_tokens": []},
                    )
                    arm = run["revision"]
                    field[arm].append(sample["microseconds"])
                    field[f"{arm}_tokens"].append(sample["token"])
    results = []
    for name in schedule.FIELDS:
        field = values[name]
        baseline_sorted = sorted(field["baseline"])
        candidate_sorted = sorted(field["candidate"])
        baseline_middle = baseline_sorted[4] + baseline_sorted[5]
        candidate_middle = candidate_sorted[4] + candidate_sorted[5]
        results.append(
            O(
                ("field", name),
                ("baseline_tokens", field["baseline_tokens"]),
                ("candidate_tokens", field["candidate_tokens"]),
                ("baseline_samples_us", field["baseline"]),
                ("candidate_samples_us", field["candidate"]),
                ("baseline_sorted_us", baseline_sorted),
                ("candidate_sorted_us", candidate_sorted),
                ("baseline_middle_sum", baseline_middle),
                ("candidate_middle_sum", candidate_middle),
                ("median_denominator", 2),
                ("ratio_numerator", candidate_middle),
                ("ratio_denominator", baseline_middle),
                ("threshold_numerator", 105),
                ("threshold_denominator", 100),
                ("passed", candidate_middle * 100 <= baseline_middle * 105),
            )
        )
    return results


class Reader:
    def __init__(self, objects, target: str):
        self.objects = dict(objects)
        self.target = target

    def read(self, oid: str):
        return self.objects[oid]

    def target_oid(self) -> str:
        return self.target


blob_base_oid, blob_base = raw("blob", b"base")
blob_candidate_oid, blob_candidate = raw("blob", b"candidate")
base_tree_oid, base_tree = raw("tree", b"100644 base\0" + bytes.fromhex(blob_base_oid))
candidate_tree_oid, candidate_tree = raw("tree", b"100644 base\0" + bytes.fromhex(blob_candidate_oid))
BASE, base_commit = commit(base_tree_oid, (), b"base\n")
CANDIDATE, candidate_commit = commit(candidate_tree_oid, (BASE,), b"candidate\n")
MERGE, merge_commit = commit(candidate_tree_oid, (BASE, CANDIDATE), b"merge\n")
TARGET, target_commit = commit(candidate_tree_oid, (MERGE,), b"target\n")

OBJECTS = {
    item.oid: item
    for item in (blob_base, blob_candidate, base_tree, candidate_tree, base_commit, candidate_commit, merge_commit, target_commit)
}
READER = Reader(OBJECTS, TARGET)


def key_blob():
    algorithm = b"ssh-ed25519"
    key = b"k" * 32
    return struct.pack(">I", len(algorithm)) + algorithm + struct.pack(">I", len(key)) + key


def revision(candidate: bool):
    if not candidate:
        return O(
            ("commit_oid", BASE),
            ("commit_sha256", base_commit.raw_sha256),
            ("tree_oid", base_tree_oid),
            ("tree_manifest_sha256", H),
            ("parents", []),
            ("commits", []),
            ("changed_paths", []),
        )
    return O(
        ("commit_oid", CANDIDATE),
        ("commit_sha256", candidate_commit.raw_sha256),
        ("tree_oid", candidate_tree_oid),
        ("tree_manifest_sha256", H1),
        ("parents", [BASE]),
        ("commits", [O(("oid", CANDIDATE), ("raw_sha256", candidate_commit.raw_sha256), ("tree_oid", candidate_tree_oid), ("parents", [BASE]))]),
        ("changed_paths", [O(
            ("path_hex", "62617365"),
            ("status", "modified"),
            ("old", path_side(oid=blob_base_oid, size=4, digest=hashlib.sha256(b"base").hexdigest())),
            ("new", path_side(oid=blob_candidate_oid, size=9, digest=hashlib.sha256(b"candidate").hexdigest())),
        )]),
    )


BENCHMARKS, OBSERVATIONS = benchmarks_and_observations()
RUN_ID = hashlib.sha256(
    b"align-json-escape-evidence-controller-run-v1\0"
    + PROFILE.encode("ascii") + b"\0" + BASE.encode("ascii") + b"\0" + CANDIDATE.encode("ascii")
).hexdigest()
BODY = O(
    ("schema", rs.BODY_SCHEMA),
    ("profile_id", "native-v1"),
    ("profile_sha256", PROFILE),
    ("producer", tool("producer")),
    ("verifier", tool("verifier", H1)),
    ("monitor", tool("monitor")),
    ("run_id", RUN_ID),
    ("started_at", "2026-08-15T00:00:00.000000000Z"),
    ("ended_at", "2026-08-15T00:00:01.000000000Z"),
    ("baseline", revision(False)),
    ("candidate", revision(True)),
    ("target", O(("local_ref", "refs/heads/main"), ("run_oid", BASE), ("expected_merge_base", BASE), ("expected_merge_head", CANDIDATE), ("expected_merge_tree", candidate_tree_oid))),
    ("review", O(("log_sha256", H), ("review_head", CANDIDATE), ("review_base", BASE), ("state", "clean"), ("reviewer", "codex"), ("repair_commits", []))),
    ("protected_inputs", O(("baseline_manifest_sha256", H), ("candidate_manifest_sha256", H), ("entries", [O(("path_hex", "646566"), ("mode", "100644"), ("kind", "blob"), ("oid", blob_base_oid), ("size", 4), ("sha256", hashlib.sha256(b"base").hexdigest()))]))),
    ("execution", O(
        ("host_id", "host-1"), ("kernel", "linux"), ("cpu", "x86_64"), ("microcode", "0x1"),
        ("cpu_set", "0-1"), ("numa_set", "0"), ("memory_bytes", 1 << 30),
        ("docker_client", executable("docker")), ("docker_daemon", "daemon"), ("oci_runtime", "runc"),
        ("image_digest", "sha256:" + H), ("image_id", H), ("image_config", H),
        ("cargo", executable("cargo")), ("rustc", executable("rustc")), ("llvm", executable("llvm")),
        ("cc", executable("cc")), ("linker", executable("lld")), ("cargo_cache_manifest_sha256", H),
        ("cargo_config_sha256", H), ("environment_sha256", H), ("mount_manifest_sha256", H),
        ("limit_manifest_sha256", H), ("descriptor_manifest_sha256", H),
    )),
    ("host_observations", OBSERVATIONS),
    ("benchmarks", BENCHMARKS),
    ("fields", field_results(BENCHMARKS)),
    ("cleanup", O(("children_remaining", 0), ("containers_remaining", 0), ("mounts_remaining", 0), ("fds_remaining", 0), ("private_dirs_remaining", 0), ("host_lock_held_for_signing", True), ("source_manifests_unchanged", True), ("cache_manifests_unchanged", True))),
    ("verdict", "pass"),
    ("first_failed_field", ""),
)
REPORT = rs.encode_report(BODY)
PUBLIC_KEY = key_blob()
REPORT_SIGNATURE = sshsig.encode_armor(sshsig.Signature(PUBLIC_KEY, sshsig.REPORT_NAMESPACE, b"s" * 64))


def regression_benchmarks():
    result = []
    for benchmark in BENCHMARKS:
        pairs = []
        for pair in benchmark["pairs"]:
            runs = []
            for run in (pair["first"], pair["second"]):
                if benchmark["name"] == "json_decode" and run["revision"] == "candidate":
                    samples = list(run["samples"])
                    samples[0] = replace(
                        replace(samples[0], "microseconds", 2_000_000),
                        "token",
                        "2000.000",
                    )
                    run = replace(run, "samples", samples)
                runs.append(run)
            pairs.append(O(("ordinal", pair["ordinal"]), ("first", runs[0]), ("second", runs[1])))
        result.append(replace(benchmark, "pairs", pairs))
    return result


REGRESSION_BENCHMARKS = regression_benchmarks()
REGRESSION_BODY = replace(
    replace(
        replace(BODY, "benchmarks", REGRESSION_BENCHMARKS),
        "fields",
        field_results(REGRESSION_BENCHMARKS),
    ),
    "verdict",
    "regression",
)
REGRESSION_BODY = replace(REGRESSION_BODY, "first_failed_field", "A-full")
REGRESSION_REPORT = rs.encode_report(REGRESSION_BODY)
REGRESSION_SIGNATURE = sshsig.encode_armor(sshsig.Signature(PUBLIC_KEY, sshsig.REPORT_NAMESPACE, b"r" * 64))
IDENTITIES = verifier.TrustedIdentities(
    profile_id=BODY["profile_id"],
    producer=cj.encode(BODY["producer"]),
    verifier=cj.encode(BODY["verifier"]),
    monitor=cj.encode(BODY["monitor"]),
    execution=cj.encode(BODY["execution"]),
)
EXPECTATIONS = verifier.ReportExpectations(PROFILE, IDENTITIES, BASE, CANDIDATE, PUBLIC_KEY)
MERGE_SIGNATURE = sshsig.encode_armor(sshsig.Signature(PUBLIC_KEY, sshsig.MERGE_NAMESPACE, b"m" * 64))
signed_messages: list[bytes] = []


def report_checker(message: bytes, signature: sshsig.Signature) -> bool:
    expected_signature = b"s" * 64 if message == REPORT else b"r" * 64 if message == REGRESSION_REPORT else None
    return message in (REPORT, REGRESSION_REPORT) and signature.namespace == sshsig.REPORT_NAMESPACE and signature.signature == expected_signature


def signer(message: bytes) -> bytes:
    signed_messages.append(message)
    return MERGE_SIGNATURE


def merge_checker(message: bytes, signature: sshsig.Signature) -> bool:
    return bool(signed_messages) and message == signed_messages[-1] and signature.namespace == sshsig.MERGE_NAMESPACE


def expect_error(label: str, action) -> None:
    try:
        action()
    except mv.MergeVerificationError:
        return
    raise AssertionError(f"{label} was accepted")


artifact = mv.produce_signed(
    REPORT,
    REPORT_SIGNATURE,
    EXPECTATIONS,
    READER,
    MERGE,
    "2026-08-16T08:00:00.000000000Z",
    report_checker,
    signer,
    merge_checker,
)
record = mv.verify_signed(
    artifact,
    REPORT,
    REPORT_SIGNATURE,
    EXPECTATIONS,
    READER,
    MERGE,
    report_checker,
    merge_checker,
)
assert record.target_oid == TARGET
assert record.merge_oid == MERGE
assert record.merge_sha256 == merge_commit.raw_sha256
assert record.parents == (BASE, CANDIDATE)
assert record.tree_oid == candidate_tree_oid
assert record.report_sha256 == hashlib.sha256(REPORT).hexdigest()
assert record.report_signature_sha256 == hashlib.sha256(REPORT_SIGNATURE).hexdigest()
assert mv.MergeVerificationRecord.decode(artifact.record) == record
assert len(signed_messages) == 1

reordered = cj.Object(tuple(reversed(record.as_object().pairs)))
expect_error("reordered merge record", lambda: mv.MergeVerificationRecord.decode(cj.encode(reordered)))
expect_error("unknown merge record schema", lambda: mv.MergeVerificationRecord.decode(cj.encode(replace(record.as_object(), "schema", "wrong/v1"))))
tampered_record = mv.MergeVerificationArtifact(
    cj.encode(replace(record.as_object(), "merge_sha256", H)),
    artifact.signature,
)
expect_error("tampered merge record signature", lambda: mv.verify_signed(tampered_record, REPORT, REPORT_SIGNATURE, EXPECTATIONS, READER, MERGE, report_checker, merge_checker))

forged_merge = git_objects.VerifiedObject("commit", MERGE, H, merge_commit.payload)
forged_reader = Reader({**OBJECTS, MERGE: forged_merge}, TARGET)
expect_error("forged raw merge digest", lambda: mv.produce_signed(REPORT, REPORT_SIGNATURE, EXPECTATIONS, forged_reader, MERGE, record.verified_at, report_checker, signer, merge_checker))

wrong_parent_oid, wrong_parent = commit(candidate_tree_oid, (CANDIDATE, BASE), b"wrong-parent\n")
wrong_target_oid, wrong_target = commit(candidate_tree_oid, (wrong_parent_oid,), b"wrong-target\n")
wrong_reader = Reader({**OBJECTS, wrong_parent.oid: wrong_parent, wrong_target.oid: wrong_target}, wrong_target_oid)
expect_error("wrong merge parent order", lambda: mv.produce_signed(REPORT, REPORT_SIGNATURE, EXPECTATIONS, wrong_reader, wrong_parent_oid, record.verified_at, report_checker, signer, merge_checker))

side_oid, side_commit = commit(candidate_tree_oid, (), b"side\n")
side_reader = Reader({**OBJECTS, side_commit.oid: side_commit}, side_oid)
expect_error("merge reachable only through a side target", lambda: mv.verify_signed(artifact, REPORT, REPORT_SIGNATURE, EXPECTATIONS, side_reader, MERGE, report_checker, merge_checker))

before_regression_sign = len(signed_messages)
expect_error("regression report after merge", lambda: mv.produce_signed(REGRESSION_REPORT, REGRESSION_SIGNATURE, EXPECTATIONS, READER, MERGE, record.verified_at, report_checker, signer, merge_checker))
assert len(signed_messages) == before_regression_sign

bad_report_signature = sshsig.encode_armor(sshsig.Signature(PUBLIC_KEY, sshsig.REPORT_NAMESPACE, b"x" * 64))
expect_error("stale report signature", lambda: mv.produce_signed(REPORT, bad_report_signature, EXPECTATIONS, READER, MERGE, record.verified_at, report_checker, signer, merge_checker))

print("merge-verification evidence checks passed")
PY
