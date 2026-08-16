#!/usr/bin/env bash
# Deterministic owner for the trusted controller/verifier orchestration core.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
from __future__ import annotations

import hashlib
import os
import struct
import tempfile

from scripts.benchmark_evidence import canonical_json as cj
from scripts.benchmark_evidence import cli
from scripts.benchmark_evidence import controller
from scripts.benchmark_evidence import report_schema as rs
from scripts.benchmark_evidence import schedule
from scripts.benchmark_evidence import sshsig
from scripts.benchmark_evidence import verifier


B = "a" * 40
C = "b" * 40
PARENT = "e" * 40
BASE_TREE = "c" * 40
CANDIDATE_TREE = "d" * 40
COMMIT = C
H = "0" * 64
H1 = "1" * 64
PROFILE = "2" * 64


def O(*pairs):
    return cj.Object(pairs)


def replace(value, key, member):
    return O(*((name, member if name == key else current) for name, current in value.pairs))


def path_side(presence, *, size=0, sha=H, mode="100644", kind="blob", oid=B):
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


def tool(version, executable_sha=H):
    return O(
        ("version", version),
        ("source_commit", B),
        ("source_manifest_blob", B),
        ("source_manifest_sha256", H),
        ("executable_sha256", executable_sha),
    )


def executable(version):
    return O(("version", version), ("executable_sha256", H))


def revision(*, candidate):
    if not candidate:
        return O(
            ("commit_oid", B),
            ("commit_sha256", H),
            ("tree_oid", BASE_TREE),
            ("tree_manifest_sha256", H),
            ("parents", [PARENT]),
            ("commits", []),
            ("changed_paths", []),
        )
    return O(
        ("commit_oid", C),
        ("commit_sha256", H),
        ("tree_oid", CANDIDATE_TREE),
        ("tree_manifest_sha256", H1),
        ("parents", [B]),
        ("commits", [O(("oid", COMMIT), ("raw_sha256", H), ("tree_oid", CANDIDATE_TREE), ("parents", [B]))]),
        ("changed_paths", [O(
            ("path_hex", "616263"),
            ("status", "modified"),
            ("old", path_side("present", size=1)),
            ("new", path_side("present", size=2, sha=H1, oid=C)),
        )]),
    )


def observation(ordinal, phase, child_id):
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


def child(child_id, arm, sequence, monitor_first, monitor_last, benchmark_name):
    fields = schedule.FIELDS[:2] if benchmark_name == "json_decode" else schedule.FIELDS[2:]
    samples = [
        O(
            ("field", field),
            ("token", f"{microseconds // 1_000}.{microseconds % 1_000:03d}"),
            ("microseconds", microseconds),
        )
        for index, field in enumerate(fields)
        for microseconds in (1_000_000 + index * 100 + (1 if arm == "candidate" else 0),)
    ]
    return O(
        ("child_id", child_id),
        ("revision", arm),
        ("sequence", sequence),
        ("stdout_sha256", H),
        ("stderr_sha256", H),
        ("stderr_tail_hex", "00"),
        ("exit_code", 0),
        ("elapsed_ns", 1_000_000),
        ("monitor_first", monitor_first),
        ("monitor_last", monitor_last),
        ("samples", samples),
    )


def preparation(child_id, arm, sequence, monitor_first, monitor_last):
    return O(
        ("child_id", child_id),
        ("revision", arm),
        ("sequence", sequence),
        ("stdout_sha256", H),
        ("stderr_sha256", H),
        ("stderr_tail_hex", "00"),
        ("exit_code", 0),
        ("elapsed_ns", 1_000_000),
        ("monitor_first", monitor_first),
        ("monitor_last", monitor_last),
        ("artifact_manifest_sha256", H),
    )


def make_benchmarks():
    grouped = {
        name: {"preparations": [], "warmups": [], "pairs": []}
        for name in schedule.BENCHMARKS
    }
    observations = [observation(0, "pre-build", "")]
    plans = schedule.full_schedule()
    for index, plan in enumerate(plans):
        prefix = "a" if plan.benchmark == "json_decode" else "b"
        child_id = f"{prefix}{plan.sequence:063x}"
        monitor_first = len(observations)
        observations.append(observation(monitor_first, "child-start", child_id))
        monitor_last = len(observations)
        observations.append(observation(monitor_last, "child-end", child_id))
        if plan.phase == "prepare":
            record = preparation(child_id, plan.revision, plan.sequence, monitor_first, monitor_last)
            grouped[plan.benchmark]["preparations"].append(record)
        else:
            record = child(
                child_id,
                plan.revision,
                plan.sequence,
                monitor_first,
                monitor_last,
                plan.benchmark,
            )
            if plan.phase == "warmup":
                grouped[plan.benchmark]["warmups"].append(record)
            else:
                grouped[plan.benchmark]["pairs"].append(record)
        if index + 1 < len(plans):
            observations.append(observation(len(observations), "between-children", ""))
    observations.append(observation(len(observations), "post-run", ""))
    benchmarks = []
    for name in schedule.BENCHMARKS:
        records = grouped[name]
        pairs = []
        for ordinal in range(1, 11):
            first = records["pairs"][(ordinal - 1) * 2]
            second = records["pairs"][(ordinal - 1) * 2 + 1]
            pairs.append(O(("ordinal", ordinal), ("first", first), ("second", second)))
        benchmarks.append(O(
            ("name", name),
            ("prepare_argv", f"bench/{name}/run.sh prepare native"),
            ("argv", f"bench/{name}/run.sh native"),
            ("preparations", records["preparations"]),
            ("warmups", records["warmups"]),
            ("pairs", pairs),
        ))
    return benchmarks, observations


def field_results(benchmarks):
    by_field = {}
    for benchmark in benchmarks:
        for pair in benchmark["pairs"]:
            for run in (pair["first"], pair["second"]):
                for sample in run["samples"]:
                    values = by_field.setdefault(sample["field"], {"baseline": [], "candidate": [], "baseline_tokens": [], "candidate_tokens": []})
                    arm = run["revision"]
                    values[arm].append(sample["microseconds"])
                    values[f"{arm}_tokens"].append(sample["token"])
    results = []
    for field in schedule.FIELDS:
        values = by_field[field]
        baseline = values["baseline"]
        candidate = values["candidate"]
        baseline_sorted = sorted(baseline)
        candidate_sorted = sorted(candidate)
        baseline_middle = baseline_sorted[4] + baseline_sorted[5]
        candidate_middle = candidate_sorted[4] + candidate_sorted[5]
        results.append(O(
            ("field", field),
            ("baseline_tokens", values["baseline_tokens"]),
            ("candidate_tokens", values["candidate_tokens"]),
            ("baseline_samples_us", baseline),
            ("candidate_samples_us", candidate),
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
        ))
    return results


def controller_run_id():
    return hashlib.sha256(
        b"align-json-escape-evidence-controller-run-v1\0"
        + PROFILE.encode("ascii")
        + b"\0" + B.encode("ascii")
        + b"\0" + C.encode("ascii")
    ).hexdigest()


def report_body():
    benchmarks, observations = make_benchmarks()
    return O(
        ("schema", rs.BODY_SCHEMA),
        ("profile_id", "native-v1"),
        ("profile_sha256", PROFILE),
        ("producer", tool("producer")),
        ("verifier", tool("verifier", H1)),
        ("monitor", tool("monitor")),
        ("run_id", controller_run_id()),
        ("started_at", "2026-08-15T00:00:00.000000000Z"),
        ("ended_at", "2026-08-15T00:00:01.000000000Z"),
        ("baseline", revision(candidate=False)),
        ("candidate", revision(candidate=True)),
        ("target", O(
            ("local_ref", "refs/heads/main"),
            ("run_oid", B),
            ("expected_merge_base", B),
            ("expected_merge_head", C),
            ("expected_merge_tree", CANDIDATE_TREE),
        )),
        ("review", O(
            ("log_sha256", H),
            ("review_head", C),
            ("review_base", B),
            ("state", "clean"),
            ("reviewer", "codex"),
            ("repair_commits", []),
        )),
        ("protected_inputs", O(
            ("baseline_manifest_sha256", H),
            ("candidate_manifest_sha256", H),
            ("entries", [O(
                ("path_hex", "646566"),
                ("mode", "100644"),
                ("kind", "blob"),
                ("oid", B),
                ("size", 3),
                ("sha256", H),
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
            ("image_digest", "sha256:" + H),
            ("image_id", H),
            ("image_config", H),
            ("cargo", executable("cargo")),
            ("rustc", executable("rustc")),
            ("llvm", executable("llvm")),
            ("cc", executable("cc")),
            ("linker", executable("lld")),
            ("cargo_cache_manifest_sha256", H),
            ("cargo_config_sha256", H),
            ("environment_sha256", H),
            ("mount_manifest_sha256", H),
            ("limit_manifest_sha256", H),
            ("descriptor_manifest_sha256", H),
        )),
        ("host_observations", observations),
        ("benchmarks", benchmarks),
        ("fields", field_results(benchmarks)),
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


def key_blob():
    algorithm = b"ssh-ed25519"
    key = b"k" * 32
    return struct.pack(">I", len(algorithm)) + algorithm + struct.pack(">I", len(key)) + key


def attestation(review_commit=C, review_state="clean"):
    return cj.encode(O(
        ("repository", "sanohiro/align"),
        ("pull_request", 7),
        ("review_id", 42),
        ("reviewer", "codex"),
        ("review_commit", review_commit),
        ("review_state", review_state),
        ("review_log_sha256", H),
        ("submitted_at", "2026-08-15T00:00:01.000000000Z"),
    ))


def pr_body(review_head=C, review_state="clean"):
    return (
        b"Evidence candidate\n"
        b"<!-- align-preflight-version:1 -->\n"
        + f"<!-- align-preflight-head:{C} -->\n".encode()
        + b"<!-- align-preflight-base-ref:main -->\n"
        + f"<!-- align-preflight-base-sha:{B} -->\n".encode()
        + f"<!-- align-preflight-review:{review_state} -->\n".encode()
        + f"<!-- align-preflight-review-head:{review_head} -->\n".encode()
        + b"<!-- align-preflight-reviewer:codex -->\n"
    )


BODY = report_body()
REPORT = rs.encode_report(BODY)
ATTESTATION = attestation()
PR_BODY = pr_body()
IDENTITIES = verifier.TrustedIdentities(
    profile_id=BODY["profile_id"],
    producer=cj.encode(BODY["producer"]),
    verifier=cj.encode(BODY["verifier"]),
    monitor=cj.encode(BODY["monitor"]),
    execution=cj.encode(BODY["execution"]),
)
EXPECTATIONS = verifier.VerifierExpectations(
    repository="sanohiro/align",
    pull_request=7,
    profile_sha256=PROFILE,
    identities=IDENTITIES,
    baseline=B,
    candidate=C,
    pr_body_sha256=hashlib.sha256(PR_BODY).hexdigest(),
    review_attestation_sha256=hashlib.sha256(ATTESTATION).hexdigest(),
    public_key_blob=key_blob(),
)
REPORT_EXPECTATIONS = verifier.ReportExpectations(
    profile_sha256=EXPECTATIONS.profile_sha256,
    identities=EXPECTATIONS.identities,
    baseline=EXPECTATIONS.baseline,
    candidate=EXPECTATIONS.candidate,
    public_key_blob=EXPECTATIONS.public_key_blob,
)
SIGNATURE = sshsig.encode_armor(sshsig.Signature(
    public_key_blob=EXPECTATIONS.public_key_blob,
    namespace=sshsig.REPORT_NAMESPACE,
    signature=b"s" * 64,
))
ARTIFACT = verifier.EvidenceArtifact(REPORT, SIGNATURE, PR_BODY, ATTESTATION, EXPECTATIONS)
PRODUCED = verifier.ProducedEvidence(REPORT, SIGNATURE, REPORT_EXPECTATIONS)
PRODUCED_VERIFIED = verifier.verify_produced_evidence(
    PRODUCED,
    lambda _preimage, _signature: True,
)


def bad_manifest_report():
    benchmarks = []
    for benchmark in BODY["benchmarks"]:
        preparations = list(benchmark["preparations"])
        if benchmark["name"] == "json_decode":
            preparations[0] = replace(preparations[0], "artifact_manifest_sha256", H1)
        benchmarks.append(replace(benchmark, "preparations", preparations))
    return rs.encode_report(replace(BODY, "benchmarks", benchmarks))


BAD_MANIFEST_REPORT = bad_manifest_report()
BAD_MANIFEST_ARTIFACT = verifier.EvidenceArtifact(
    BAD_MANIFEST_REPORT,
    SIGNATURE,
    PR_BODY,
    ATTESTATION,
    EXPECTATIONS,
)
BAD_MANIFEST_PRODUCED = verifier.ProducedEvidence(
    BAD_MANIFEST_REPORT,
    SIGNATURE,
    REPORT_EXPECTATIONS,
)
BAD_MANIFEST_VERIFIED = verifier.verify_artifact(
    BAD_MANIFEST_ARTIFACT,
    lambda _preimage, _signature: True,
)


def regression_benchmarks():
    result = []
    for benchmark in BODY["benchmarks"]:
        pairs = []
        for pair in benchmark["pairs"]:
            runs = []
            for run in (pair["first"], pair["second"]):
                if benchmark["name"] == "json_decode" and run["revision"] == "candidate":
                    samples = list(run["samples"])
                    samples[0] = replace(replace(samples[0], "microseconds", 2_000_000), "token", "2000.000")
                    run = replace(run, "samples", samples)
                runs.append(run)
            pairs.append(O(("ordinal", pair["ordinal"]), ("first", runs[0]), ("second", runs[1])))
        result.append(replace(benchmark, "pairs", pairs))
    return result


REGRESSION_BENCHMARKS = regression_benchmarks()
REGRESSION_BODY = replace(
    replace(
        replace(
            replace(BODY, "benchmarks", REGRESSION_BENCHMARKS),
            "fields",
            field_results(REGRESSION_BENCHMARKS),
        ),
        "verdict",
        "regression",
    ),
    "first_failed_field",
    "A-full",
)
REGRESSION_EXPECTATIONS = verifier.VerifierExpectations(
    repository=EXPECTATIONS.repository,
    pull_request=EXPECTATIONS.pull_request,
    profile_sha256=EXPECTATIONS.profile_sha256,
    identities=EXPECTATIONS.identities,
    baseline=EXPECTATIONS.baseline,
    candidate=EXPECTATIONS.candidate,
    pr_body_sha256=EXPECTATIONS.pr_body_sha256,
    review_attestation_sha256=EXPECTATIONS.review_attestation_sha256,
    public_key_blob=EXPECTATIONS.public_key_blob,
    require_pass=False,
)
REGRESSION_ARTIFACT = verifier.EvidenceArtifact(
    rs.encode_report(REGRESSION_BODY),
    SIGNATURE,
    PR_BODY,
    ATTESTATION,
    REGRESSION_EXPECTATIONS,
)
REGRESSION_PRODUCED = verifier.ProducedEvidence(
    REGRESSION_ARTIFACT.report,
    REGRESSION_ARTIFACT.signature,
    REPORT_EXPECTATIONS,
)
REGRESSION_VERIFIED = verifier.verify_artifact(
    REGRESSION_ARTIFACT,
    lambda _preimage, _signature: True,
)
assert REGRESSION_VERIFIED.verdict == "regression"
REGRESSION_REPORT_VERIFIED = verifier.verify_produced_evidence(
    REGRESSION_PRODUCED,
    lambda _preimage, _signature: True,
)


def overflow_benchmarks():
    result = []
    overflow_us = cj.MAX_U64 // 100 + 1
    for benchmark in BODY["benchmarks"]:
        pairs = []
        for pair in benchmark["pairs"]:
            runs = []
            for run in (pair["first"], pair["second"]):
                if benchmark["name"] == "json_decode" and run["revision"] == "baseline":
                    samples = []
                    for sample in run["samples"]:
                        if sample["field"] == "A-full":
                            sample = replace(
                                replace(sample, "microseconds", overflow_us),
                                "token",
                                f"{overflow_us // 1_000}.{overflow_us % 1_000:03d}",
                            )
                        samples.append(sample)
                    run = replace(run, "samples", samples)
                runs.append(run)
            pairs.append(O(("ordinal", pair["ordinal"]), ("first", runs[0]), ("second", runs[1])))
        result.append(replace(benchmark, "pairs", pairs))
    return result


OVERFLOW_BENCHMARKS = overflow_benchmarks()
OVERFLOW_BODY = replace(
    replace(BODY, "benchmarks", OVERFLOW_BENCHMARKS),
    "fields",
    field_results(OVERFLOW_BENCHMARKS),
)
OVERFLOW_ARTIFACT = verifier.EvidenceArtifact(
    rs.encode_report(OVERFLOW_BODY),
    SIGNATURE,
    PR_BODY,
    ATTESTATION,
    EXPECTATIONS,
)


def rejected(label, action):
    try:
        action()
    except (controller.ControllerError, verifier.VerificationError):
        return
    raise AssertionError(f"{label} was accepted")


preimage_lengths = []
verified = verifier.verify_artifact(
    ARTIFACT,
    lambda preimage, signature: (preimage_lengths.append(len(preimage)) or True),
)
assert verified.baseline == B
assert verified.candidate == C
assert verified.profile_sha256 == PROFILE
assert verified.review_attestation.review_id == 42
assert preimage_lengths == [len(sshsig.signing_preimage(REPORT, sshsig.REPORT_NAMESPACE))]
assert PRODUCED_VERIFIED.baseline == B
assert PRODUCED_VERIFIED.candidate == C
assert PRODUCED_VERIFIED.verdict == "pass"

overlapping_protected = replace(
    BODY["protected_inputs"],
    "entries",
    [replace(BODY["protected_inputs"]["entries"][0], "path_hex", "616263")],
)
rejected("protected input changed path overlap", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(replace(BODY, "protected_inputs", overlapping_protected)),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))

child_id_mismatch_observations = list(BODY["host_observations"])
child_id_mismatch_observations[2] = replace(
    child_id_mismatch_observations[2],
    "child_id",
    BODY["benchmarks"][0]["preparations"][1]["child_id"],
)
rejected("monitor child ID mismatch", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(replace(BODY, "host_observations", child_id_mismatch_observations)),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))

swap_observations = [
    replace(observation, "swap_read_bytes", 0 if ordinal == 0 else 1)
    for ordinal, observation in enumerate(BODY["host_observations"])
]
rejected("monitor swap event", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(replace(BODY, "host_observations", swap_observations)),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))

baseline_commit = O(
    ("oid", B),
    ("raw_sha256", H),
    ("tree_oid", BASE_TREE),
    ("parents", [B]),
)
baseline_in_candidate = replace(
    BODY["candidate"],
    "commits",
    [baseline_commit, BODY["candidate"]["commits"][0]],
)
rejected("baseline pseudo-entry in candidate inventory", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(replace(BODY, "candidate", baseline_in_candidate)),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))

incomplete_observations = [
    replace(observation, "ordinal", ordinal)
    for ordinal, observation in enumerate(
        observation
        for observation in BODY["host_observations"]
        if observation["phase"] in ("child-start", "child-sample", "child-end")
    )
]
rejected("incomplete monitor lifecycle", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(replace(BODY, "host_observations", incomplete_observations)),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))

non_dense_observations = list(BODY["host_observations"])
non_dense_observations[1] = replace(non_dense_observations[1], "ordinal", 99)
rejected("non-dense monitor ordinal", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(replace(BODY, "host_observations", non_dense_observations)),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))

phase_child_mismatch = list(BODY["host_observations"])
phase_child_mismatch[0] = replace(
    phase_child_mismatch[0],
    "child_id",
    BODY["benchmarks"][0]["preparations"][0]["child_id"],
)
rejected("monitor phase child mismatch", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(replace(BODY, "host_observations", phase_child_mismatch)),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))

nonmonotonic_observations = list(BODY["host_observations"])
nonmonotonic_observations[2] = replace(
    nonmonotonic_observations[2],
    "monotonic_ns",
    nonmonotonic_observations[1]["monotonic_ns"],
)
rejected("nonmonotonic monitor observation", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(replace(BODY, "host_observations", nonmonotonic_observations)),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))

counter_reset_observations = list(BODY["host_observations"])
counter_reset_observations[1] = replace(counter_reset_observations[1], "throttle_events", 1)
counter_reset_observations[2] = replace(counter_reset_observations[2], "throttle_events", 0)
rejected("monitor counter reset", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(replace(BODY, "host_observations", counter_reset_observations)),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))

bad_sample = replace(BODY["benchmarks"][0]["pairs"][0]["first"]["samples"][0], "microseconds", 2_000_000)
bad_sample = replace(bad_sample, "token", "2000.000")
bad_run = replace(
    BODY["benchmarks"][0]["pairs"][0]["first"],
    "samples",
    [bad_sample] + BODY["benchmarks"][0]["pairs"][0]["first"]["samples"][1:],
)
bad_pair = replace(BODY["benchmarks"][0]["pairs"][0], "first", bad_run)
bad_pairs = [bad_pair] + BODY["benchmarks"][0]["pairs"][1:]
bad_benchmark = replace(BODY["benchmarks"][0], "pairs", bad_pairs)
bad_benchmarks = [bad_benchmark, BODY["benchmarks"][1]]
rejected("derived sample mismatch", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(replace(BODY, "benchmarks", bad_benchmarks)),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))
bad_commit = replace(BODY["candidate"]["commits"][0], "parents", [C])
bad_candidate = replace(BODY["candidate"], "commits", [bad_commit])
rejected("pre-review parent chain mismatch", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(replace(BODY, "candidate", bad_candidate)),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))
bad_commit_sha = replace(BODY["candidate"]["commits"][0], "raw_sha256", H1)
bad_candidate_sha = replace(BODY["candidate"], "commits", [bad_commit_sha])
rejected("candidate raw SHA mismatch", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(replace(BODY, "candidate", bad_candidate_sha)),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))
rejected("threshold product overflow", lambda: verifier.verify_artifact(
    OVERFLOW_ARTIFACT,
    lambda _preimage, _signature: True,
))
rejected("wrong trusted execution identity", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(replace(BODY, "execution", replace(BODY["execution"], "image_digest", "sha256:" + H1))),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))

rejected("wrong report candidate", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(replace(BODY, "candidate", replace(BODY["candidate"], "commit_oid", "f" * 40))),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))
wrong_target = replace(BODY["target"], "run_oid", C)
rejected("target run OID drift", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(replace(BODY, "target", wrong_target)),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))
wrong_clean_review = replace(BODY["review"], "repair_commits", [C])
rejected("clean review with repair commits", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(replace(BODY, "review", wrong_clean_review)),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))
rejected("wrong PR marker", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(REPORT, SIGNATURE, PR_BODY + b"<!-- align-preflight-review:fixed -->\n", ATTESTATION, EXPECTATIONS),
    lambda _preimage, _signature: True,
))
conflicting_pr_body = PR_BODY + b"<!-- align-preflight-review:fixed -->\n"
conflicting_expectations = verifier.VerifierExpectations(
    repository=EXPECTATIONS.repository,
    pull_request=EXPECTATIONS.pull_request,
    profile_sha256=EXPECTATIONS.profile_sha256,
    identities=EXPECTATIONS.identities,
    baseline=EXPECTATIONS.baseline,
    candidate=EXPECTATIONS.candidate,
    pr_body_sha256=hashlib.sha256(conflicting_pr_body).hexdigest(),
    review_attestation_sha256=EXPECTATIONS.review_attestation_sha256,
    public_key_blob=EXPECTATIONS.public_key_blob,
)
rejected("conflicting PR marker with trusted digest", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(REPORT, SIGNATURE, conflicting_pr_body, ATTESTATION, conflicting_expectations),
    lambda _preimage, _signature: True,
))
rejected("wrong review signature namespace", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        REPORT,
        sshsig.encode_armor(sshsig.Signature(EXPECTATIONS.public_key_blob, sshsig.MERGE_NAMESPACE, b"s" * 64)),
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))
rejected("cryptographic signature rejection", lambda: verifier.verify_artifact(
    ARTIFACT,
    lambda _preimage, _signature: False,
))
failed_fields = list(BODY["fields"])
failed_fields[0] = replace(failed_fields[0], "passed", False)
regression_body = replace(
    replace(BODY, "fields", failed_fields),
    "verdict",
    "regression",
)
regression_body = replace(regression_body, "first_failed_field", "A-full")
rejected("regression is not accepted evidence", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(regression_body),
        SIGNATURE,
        PR_BODY,
        ATTESTATION,
        EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))
bad_attestation = replace(cj.decode(ATTESTATION), "review_id", 43)
rejected("review attestation digest binding", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(REPORT, SIGNATURE, PR_BODY, cj.encode(bad_attestation), EXPECTATIONS),
    lambda _preimage, _signature: True,
))

FIXED_HEAD = "f" * 40
fixed_commits = [
    O(("oid", FIXED_HEAD), ("raw_sha256", H), ("tree_oid", CANDIDATE_TREE), ("parents", [B])),
    O(("oid", C), ("raw_sha256", H1), ("tree_oid", CANDIDATE_TREE), ("parents", [FIXED_HEAD])),
]
fixed_candidate = replace(
    replace(
        replace(BODY["candidate"], "parents", [FIXED_HEAD]),
        "commits",
        fixed_commits,
    ),
    "commit_sha256",
    H1,
)
fixed_review = replace(
    replace(
        replace(BODY["review"], "review_head", FIXED_HEAD),
        "state",
        "fixed",
    ),
    "repair_commits",
    [C],
)
fixed_body = replace(replace(BODY, "candidate", fixed_candidate), "review", fixed_review)
FIXED_ATTESTATION = attestation(FIXED_HEAD, "fixed")
FIXED_PR_BODY = pr_body(FIXED_HEAD, "fixed")
FIXED_EXPECTATIONS = verifier.VerifierExpectations(
    repository="sanohiro/align",
    pull_request=7,
    profile_sha256=PROFILE,
    identities=IDENTITIES,
    baseline=B,
    candidate=C,
    pr_body_sha256=hashlib.sha256(FIXED_PR_BODY).hexdigest(),
    review_attestation_sha256=hashlib.sha256(FIXED_ATTESTATION).hexdigest(),
    public_key_blob=key_blob(),
)
fixed_verified = verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(fixed_body),
        SIGNATURE,
        FIXED_PR_BODY,
        FIXED_ATTESTATION,
        FIXED_EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
)
assert fixed_verified.review_state == "fixed"
assert fixed_verified.artifact.review_attestation == FIXED_ATTESTATION

BASELINE_FIXED_BODY = replace(
    fixed_body,
    "review",
    replace(fixed_body["review"], "review_head", B),
)
BASELINE_FIXED_ATTESTATION = attestation(B, "fixed")
BASELINE_FIXED_PR_BODY = pr_body(B, "fixed")
BASELINE_FIXED_EXPECTATIONS = verifier.VerifierExpectations(
    repository="sanohiro/align",
    pull_request=7,
    profile_sha256=PROFILE,
    identities=IDENTITIES,
    baseline=B,
    candidate=C,
    pr_body_sha256=hashlib.sha256(BASELINE_FIXED_PR_BODY).hexdigest(),
    review_attestation_sha256=hashlib.sha256(BASELINE_FIXED_ATTESTATION).hexdigest(),
    public_key_blob=key_blob(),
)
rejected("fixed review rooted at baseline", lambda: verifier.verify_artifact(
    verifier.EvidenceArtifact(
        rs.encode_report(BASELINE_FIXED_BODY),
        SIGNATURE,
        BASELINE_FIXED_PR_BODY,
        BASELINE_FIXED_ATTESTATION,
        BASELINE_FIXED_EXPECTATIONS,
    ),
    lambda _preimage, _signature: True,
))


class FixtureLease:
    def __init__(self):
        self.events = []
        self.locked = False
        self.reserved = False
        self.closed = False
        self.fail_abort = False

    def acquire(self):
        assert not self.locked and not self.reserved
        self.locked = True
        self.events.append("acquire")

    def create_reservation(self, run_id, output_dir):
        assert self.locked and not self.reserved
        assert len(run_id) == 64 and output_dir.startswith("/")
        self.reserved = True
        self.events.append("reserve")

    def release_lock_for_publication(self):
        assert self.locked and self.reserved
        self.locked = False
        self.events.append("release")

    def mark_published(self):
        assert not self.locked and self.reserved
        self.events.append("mark")

    def finalize_publication(self):
        assert not self.locked and self.reserved
        self.reserved = False
        self.closed = True
        self.events.append("finalize")

    def abort(self, *, remove_reservation):
        if self.fail_abort:
            raise OSError("fixture abort failed")
        self.events.append("abort-remove" if remove_reservation else "abort-keep")
        self.locked = False
        if remove_reservation:
            self.reserved = False
        self.closed = True


INVOCATION_COUNTER = 0


def invocation():
    global INVOCATION_COUNTER
    INVOCATION_COUNTER += 1
    output = f"/tmp/align-controller-verifier-owner-{os.getpid()}-{INVOCATION_COUNTER}"
    return cli.parse([
        "run",
        "--repository", "/trusted/repository",
        "--baseline", B,
        "--candidate", C,
        "--review-log", "/trusted/review.log",
        "--output-dir", output,
    ])


CONFIG = controller.ControllerConfig(
    profile_id="native-v1",
    profile_sha256=PROFILE,
    controller_sha256=H,
    verifier_sha256=H1,
    monitor_sha256=H,
    host_id="host-1",
    image_digest="sha256:" + H,
    public_key_blob=EXPECTATIONS.public_key_blob,
    identities=IDENTITIES,
)
FAKE_VERIFIED = verifier.VerifiedReport(
    produced=PRODUCED,
    report_sha256=H,
    body_sha256=H,
    baseline=B,
    candidate=C,
    profile_sha256=PROFILE,
    review_state="clean",
    verdict="pass",
)


def make_hooks(
    *,
    fail_gate=None,
    fail_child=None,
    fail_verify=False,
    fail_stage=False,
    fail_discard=False,
    fail_publish=False,
    bad_manifests=False,
    bad_artifact_expectations=False,
    bad_report_manifests=False,
    regression=False,
):
    events = []

    def gate(inv, config, name):
        events.append(name)
        if name == fail_gate:
            raise controller.ControllerError(f"{name} gate failed")

    gates = tuple((name, lambda inv, config, name=name: gate(inv, config, name)) for name in controller._GATE_NAMES)

    def child_id(plan):
        value = hashlib.sha256(f"child-{plan.sequence}".encode()).hexdigest()
        events.append(f"id-{plan.sequence}")
        return value

    def execute(plan, child_id_value):
        events.append(f"exec-{plan.sequence}")
        if plan.sequence == fail_child:
            raise controller.ControllerError("child failed")
        return controller.ChildResult(
            child_id=child_id_value,
            artifact_manifest_sha256=H,
            build_attempted=plan.phase == "prepare",
            resources=(controller.OwnedResource("children", child_id_value),),
        )

    def manifests(inv):
        return (False, True) if bad_manifests else (True, True)

    def produce(inv, manifests, phases):
        assert set(manifests) == {(bench, rev) for bench in schedule.BENCHMARKS for rev in schedule.REVISIONS}
        assert not hasattr(inv, "pr_body")
        assert not hasattr(inv, "review_attestation")
        assert phases[-1] == "report"
        events.append("produce")
        if bad_artifact_expectations:
            wrong = verifier.ReportExpectations(
                profile_sha256=EXPECTATIONS.profile_sha256,
                identities=EXPECTATIONS.identities,
                baseline="f" * 40,
                candidate=C,
                public_key_blob=EXPECTATIONS.public_key_blob,
            )
            return verifier.ProducedEvidence(REPORT, SIGNATURE, wrong)
        if bad_report_manifests:
            return BAD_MANIFEST_PRODUCED
        if regression:
            return REGRESSION_PRODUCED
        return PRODUCED

    def verify(produced):
        events.append("verify-report")
        if fail_verify:
            raise verifier.VerificationError("fixture verifier rejected")
        if bad_report_manifests:
            assert produced is BAD_MANIFEST_PRODUCED
            return verifier.VerifiedReport(
                produced=produced,
                report_sha256=H,
                body_sha256=H,
                baseline=B,
                candidate=C,
                profile_sha256=PROFILE,
                review_state="clean",
                verdict="pass",
            )
        if regression:
            assert produced is REGRESSION_PRODUCED
            return REGRESSION_REPORT_VERIFIED
        assert produced is PRODUCED
        return FAKE_VERIFIED

    def stage(verified_value, output_dir):
        assert verified_value.produced is PRODUCED or verified_value.produced is REGRESSION_PRODUCED
        assert output_dir.startswith("/")
        events.append("stage-report")
        if fail_stage:
            raise controller.ControllerError("fixture staging failed")

    def discard(output_dir):
        assert output_dir.startswith("/")
        events.append("discard-stage")
        if fail_discard:
            raise controller.ControllerError("fixture staging discard failed")

    def publish(verified_value, output_dir):
        assert verified_value.produced is PRODUCED or verified_value.produced is REGRESSION_PRODUCED
        events.append("publish")
        if fail_publish:
            raise controller.ControllerError("fixture publication failed")

    return controller.ControllerHooks(
        gates=gates,
        child_id=child_id,
        execute=execute,
        manifests=manifests,
        produce_report=produce,
        verify_report=verify,
        stage_report=stage,
        discard_staging=discard,
        publish=publish,
    ), events


hooks, events = make_hooks()
lease = FixtureLease()
result = controller.Controller(CONFIG, hooks, lease).run(invocation())
assert result.accepted
assert result.state == "accepted"
assert result.error is None
assert result.cleanup.accepted
assert result.cleanup.fail_closed is False
assert result.verified is FAKE_VERIFIED
assert result.phases == (
    "exclusive", "reserve", "bootstrap", "host", "image", "source", "review", "schedule",
    "manifests", "report", "verify", "stage", "unlock", "publish",
    "finalize", "accepted",
)
assert lease.events == ["acquire", "reserve", "release", "mark", "finalize"]
assert events[:4] == ["bootstrap", "host", "image", "source"]
assert events[-4:] == ["produce", "verify-report", "stage-report", "publish"]
assert len([event for event in events if event.startswith("exec-")]) == len(schedule.full_schedule())

hooks, events = make_hooks(regression=True)
lease = FixtureLease()
result = controller.Controller(CONFIG, hooks, lease).run(invocation())
assert result.state == "regression"
assert not result.accepted
assert result.verified is REGRESSION_REPORT_VERIFIED
assert result.cleanup.accepted
assert not lease.reserved
assert result.phases[-1] == "regression"
assert events[-4:] == ["produce", "verify-report", "stage-report", "publish"]

hooks, events = make_hooks(fail_gate="host")
lease = FixtureLease()
result = controller.Controller(CONFIG, hooks, lease).run(invocation())
assert result.state == "rejected"
assert not result.cleanup.fail_closed
assert result.phases == ("exclusive", "reserve", "bootstrap", "host")
assert "image" not in events and not lease.reserved

hooks, events = make_hooks(fail_gate="host")
lease = FixtureLease()
lease.fail_abort = True
result = controller.Controller(CONFIG, hooks, lease).run(invocation())
assert result.state == "fail-closed"
assert result.cleanup.fail_closed
assert result.cleanup.reservation_present
assert lease.reserved

hooks, events = make_hooks(fail_child=0)
lease = FixtureLease()
result = controller.Controller(CONFIG, hooks, lease).run(invocation())
assert result.state == "fail-closed"
assert result.cleanup.fail_closed
assert lease.reserved
assert lease.events == ["acquire", "reserve", "abort-keep"]

hooks, events = make_hooks(fail_verify=True)
lease = FixtureLease()
result = controller.Controller(CONFIG, hooks, lease).run(invocation())
assert result.state == "rejected"
assert not result.cleanup.fail_closed
assert not lease.reserved
assert "publish" not in events

hooks, events = make_hooks(fail_stage=True)
lease = FixtureLease()
result = controller.Controller(CONFIG, hooks, lease).run(invocation())
assert result.state == "rejected"
assert not result.cleanup.fail_closed
assert "publish" not in events
assert events[-2:] == ["stage-report", "discard-stage"]
assert not lease.reserved

hooks, events = make_hooks(fail_stage=True, fail_discard=True)
lease = FixtureLease()
result = controller.Controller(CONFIG, hooks, lease).run(invocation())
assert result.state == "fail-closed"
assert result.cleanup.fail_closed
assert lease.reserved
assert events[-2:] == ["stage-report", "discard-stage"]

hooks, events = make_hooks(bad_artifact_expectations=True)
lease = FixtureLease()
result = controller.Controller(CONFIG, hooks, lease).run(invocation())
assert result.state == "rejected"
assert "verify" not in events
assert not lease.reserved

hooks, events = make_hooks(bad_manifests=True)
lease = FixtureLease()
result = controller.Controller(CONFIG, hooks, lease).run(invocation())
assert result.state == "rejected"
assert not result.cleanup.fail_closed
assert "produce" not in events
assert not lease.reserved

hooks, events = make_hooks(bad_report_manifests=True)
lease = FixtureLease()
result = controller.Controller(CONFIG, hooks, lease).run(invocation())
assert result.state == "rejected"
assert result.phases[-1] == "verify"
assert not result.cleanup.fail_closed
assert "publish" not in events
assert not lease.reserved

hooks, events = make_hooks(fail_publish=True)
lease = FixtureLease()
result = controller.Controller(CONFIG, hooks, lease).run(invocation())
assert result.state == "fail-closed"
assert result.cleanup.fail_closed
assert lease.reserved
assert events[-3:] == ["stage-report", "publish", "discard-stage"]

print("trusted controller/verifier checks passed")
PY
