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


def tool(version):
    return O(
        ("version", version),
        ("source_commit", B),
        ("source_manifest_blob", B),
        ("source_manifest_sha256", H),
        ("executable_sha256", H),
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
            ("parents", []),
            ("commits", []),
            ("changed_paths", []),
        )
    return O(
        ("commit_oid", C),
        ("commit_sha256", H1),
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


def child(child_id, arm, sequence):
    return O(
        ("child_id", child_id),
        ("revision", arm),
        ("sequence", sequence),
        ("stdout_sha256", H),
        ("stderr_sha256", H),
        ("stderr_tail_hex", "00"),
        ("exit_code", 0),
        ("elapsed_ns", 1_000_000),
        ("monitor_first", 0),
        ("monitor_last", 0),
        ("samples", []),
    )


def preparation(child_id, arm, sequence):
    return O(
        ("child_id", child_id),
        ("revision", arm),
        ("sequence", sequence),
        ("stdout_sha256", H),
        ("stderr_sha256", H),
        ("stderr_tail_hex", "00"),
        ("exit_code", 0),
        ("elapsed_ns", 1_000_000),
        ("monitor_first", 0),
        ("monitor_last", 0),
        ("artifact_manifest_sha256", H),
    )


def benchmark(name, prepare_argv, argv, prefix):
    preparations = [preparation(f"{prefix}{i:063x}", arm, i) for i, arm in enumerate(("baseline", "candidate"), 1)]
    warmups = [child(f"{prefix}{i:063x}", arm, i) for i, arm in enumerate(("baseline", "candidate"), 3)]
    pairs = []
    for ordinal in range(1, 11):
        first_arm, second_arm = (("baseline", "candidate") if ordinal % 2 else ("candidate", "baseline"))
        pairs.append(O(
            ("ordinal", ordinal),
            ("first", child(f"{prefix}{ordinal * 2 + 5:063x}", first_arm, ordinal * 2)),
            ("second", child(f"{prefix}{ordinal * 2 + 6:063x}", second_arm, ordinal * 2 + 1)),
        ))
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


def report_body():
    return O(
        ("schema", rs.BODY_SCHEMA),
        ("profile_id", "native-v1"),
        ("profile_sha256", PROFILE),
        ("producer", tool("producer")),
        ("verifier", tool("verifier")),
        ("monitor", tool("monitor")),
        ("run_id", H),
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
                ("path_hex", "616263"),
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
EXPECTATIONS = verifier.VerifierExpectations(
    repository="sanohiro/align",
    pull_request=7,
    profile_sha256=PROFILE,
    baseline=B,
    candidate=C,
    pr_body_sha256=hashlib.sha256(PR_BODY).hexdigest(),
    review_attestation_sha256=hashlib.sha256(ATTESTATION).hexdigest(),
    public_key_blob=key_blob(),
)
SIGNATURE = sshsig.encode_armor(sshsig.Signature(
    public_key_blob=EXPECTATIONS.public_key_blob,
    namespace=sshsig.REPORT_NAMESPACE,
    signature=b"s" * 64,
))
ARTIFACT = verifier.EvidenceArtifact(REPORT, SIGNATURE, PR_BODY, ATTESTATION, EXPECTATIONS)


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
    replace(BODY["candidate"], "parents", [FIXED_HEAD]),
    "commits",
    fixed_commits,
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


class FixtureLease:
    def __init__(self):
        self.events = []
        self.locked = False
        self.reserved = False
        self.closed = False

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
)
FAKE_VERIFIED = verifier.VerifiedEvidence(
    artifact=ARTIFACT,
    report_sha256=H,
    body_sha256=H,
    baseline=B,
    candidate=C,
    profile_sha256=PROFILE,
    review_state="clean",
    verdict="pass",
    review_attestation=verifier.decode_review_attestation(ATTESTATION),
)


def make_hooks(
    *,
    fail_gate=None,
    fail_child=None,
    fail_verify=False,
    fail_publish=False,
    bad_manifests=False,
    bad_artifact_expectations=False,
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
        assert phases[-1] == "report"
        events.append("produce")
        if bad_artifact_expectations:
            wrong = verifier.VerifierExpectations(
                repository=EXPECTATIONS.repository,
                pull_request=EXPECTATIONS.pull_request,
                profile_sha256=EXPECTATIONS.profile_sha256,
                baseline="f" * 40,
                candidate=C,
                pr_body_sha256=EXPECTATIONS.pr_body_sha256,
                review_attestation_sha256=EXPECTATIONS.review_attestation_sha256,
                public_key_blob=EXPECTATIONS.public_key_blob,
            )
            return verifier.EvidenceArtifact(REPORT, SIGNATURE, PR_BODY, ATTESTATION, wrong)
        return ARTIFACT

    def verify(artifact):
        events.append("verify")
        if fail_verify:
            raise verifier.VerificationError("fixture verifier rejected")
        assert artifact is ARTIFACT
        return FAKE_VERIFIED

    def publish(verified_value, artifact, output_dir):
        assert verified_value.artifact is artifact
        events.append("publish")
        if fail_publish:
            raise controller.ControllerError("fixture publication failed")

    return controller.ControllerHooks(
        gates=gates,
        child_id=child_id,
        execute=execute,
        manifests=manifests,
        produce_artifact=produce,
        verify_artifact=verify,
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
assert result.phases == (
    "exclusive", "reserve", "bootstrap", "host", "image", "source", "review", "schedule",
    "manifests", "report", "verify", "stage", "unlock", "publish",
    "finalize", "accepted",
)
assert lease.events == ["acquire", "reserve", "release", "mark", "finalize"]
assert events[:4] == ["bootstrap", "host", "image", "source"]
assert events[-3:] == ["produce", "verify", "publish"]
assert len([event for event in events if event.startswith("exec-")]) == len(schedule.full_schedule())

hooks, events = make_hooks(fail_gate="host")
lease = FixtureLease()
result = controller.Controller(CONFIG, hooks, lease).run(invocation())
assert result.state == "rejected"
assert not result.cleanup.fail_closed
assert result.phases == ("exclusive", "reserve", "bootstrap", "host")
assert "image" not in events and not lease.reserved

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

hooks, events = make_hooks(fail_publish=True)
lease = FixtureLease()
result = controller.Controller(CONFIG, hooks, lease).run(invocation())
assert result.state == "fail-closed"
assert result.cleanup.fail_closed
assert lease.reserved
assert lease.events == ["acquire", "reserve", "release", "abort-keep"]

print("trusted controller/verifier checks passed")
PY
