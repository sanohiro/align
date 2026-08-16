"""Trusted binding checks for one evidence report artifact.

This module is the pure verifier boundary consumed by the controller.  It
reads only bytes supplied by a trusted caller: a canonical report, its
canonical SSHSIG armor, the trusted-base PR body, and the trusted review
attestation.  It performs no repository, network, build, or output I/O.

Cryptographic Ed25519 verification is an injected operation.  The later
installed adapter owns the ``ssh-keygen``/key-process boundary; this module
still fixes the exact SSHSIG namespace, key, and signing preimage before that
operation is called.
"""

from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass
from typing import Any, Callable

from . import canonical_json as cj
from . import report_schema
from . import schedule
from . import sshsig


class VerificationError(ValueError):
    """A report artifact is malformed, stale, or not trusted for the run."""


_NAME = re.compile(r"[A-Za-z0-9._/:+=@-]{1,255}\Z")
_HEX40 = re.compile(r"[0-9a-f]{40}\Z")
_HEX64 = re.compile(r"[0-9a-f]{64}\Z")
_TIME = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{9}Z\Z")
_REVIEW_STATES = ("clean", "fixed")

_ATTESTATION_KEYS = (
    "repository",
    "pull_request",
    "review_id",
    "reviewer",
    "review_commit",
    "review_state",
    "review_log_sha256",
    "submitted_at",
)


def _error(message: str) -> None:
    raise VerificationError(message)


def _string(value: object, pattern: re.Pattern[str], label: str) -> str:
    if not isinstance(value, str) or pattern.fullmatch(value) is None:
        _error(f"{label} has the wrong grammar")
    return value


def _uint(value: object, label: str) -> int:
    try:
        return cj.require_uint(value, label, maximum=cj.MAX_U64)
    except cj.CanonicalJsonError as exc:
        raise VerificationError(str(exc)) from exc


def _key_blob(value: object) -> bytes:
    if not isinstance(value, bytes):
        _error("expected public key must be bytes")
    try:
        # ``encode_binary`` applies the complete pinned Ed25519 key-blob
        # grammar without needing a signature from the host key.
        sshsig.encode_binary(
            sshsig.Signature(
                public_key_blob=value,
                namespace=sshsig.REPORT_NAMESPACE,
                signature=b"\0" * 64,
            )
        )
    except sshsig.SSHSigError as exc:
        raise VerificationError(f"expected public key is invalid: {exc}") from exc
    return value


def _canonical_identity(raw: bytes, validator: Callable[[Any, str], cj.Object], label: str) -> bytes:
    if not isinstance(raw, bytes):
        _error(f"{label} must be canonical JSON bytes")
    try:
        value = cj.decode(raw)
        validator(value, label)
        if cj.encode(value) != raw:
            _error(f"{label} is not canonical")
    except cj.CanonicalJsonError as exc:
        raise VerificationError(f"{label} is invalid: {exc}") from exc
    return raw


def _identity_member(raw: bytes, key: str, label: str) -> Any:
    try:
        value = cj.decode(raw)
    except cj.CanonicalJsonError as exc:
        raise VerificationError(f"{label} is invalid: {exc}") from exc
    return value[key]


@dataclass(frozen=True)
class TrustedIdentities:
    """Immutable profile-owned report identity records."""

    profile_id: str
    producer: bytes
    verifier: bytes
    monitor: bytes
    execution: bytes

    def __post_init__(self) -> None:
        _string(self.profile_id, _NAME, "identities.profile_id")
        _canonical_identity(self.producer, report_schema.validate_tool_identity, "identities.producer")
        _canonical_identity(self.verifier, report_schema.validate_tool_identity, "identities.verifier")
        _canonical_identity(self.monitor, report_schema.validate_tool_identity, "identities.monitor")
        _canonical_identity(self.execution, report_schema.validate_execution_identity, "identities.execution")

    @property
    def producer_executable_sha256(self) -> str:
        return _identity_member(self.producer, "executable_sha256", "identities.producer")

    @property
    def verifier_executable_sha256(self) -> str:
        return _identity_member(self.verifier, "executable_sha256", "identities.verifier")

    @property
    def monitor_executable_sha256(self) -> str:
        return _identity_member(self.monitor, "executable_sha256", "identities.monitor")

    @property
    def execution_host_id(self) -> str:
        return _identity_member(self.execution, "host_id", "identities.execution")

    @property
    def execution_image_digest(self) -> str:
        return _identity_member(self.execution, "image_digest", "identities.execution")


@dataclass(frozen=True)
class VerifierExpectations:
    """Trusted-base values that are never selected by report bytes."""

    repository: str
    pull_request: int
    profile_sha256: str
    identities: TrustedIdentities
    baseline: str
    candidate: str
    pr_body_sha256: str
    review_attestation_sha256: str
    public_key_blob: bytes
    require_pass: bool = True

    def __post_init__(self) -> None:
        _string(self.repository, _NAME, "repository")
        if type(self.pull_request) is not int or not 0 < self.pull_request <= cj.MAX_U64:
            _error("pull_request must be a positive u64")
        _string(self.profile_sha256, _HEX64, "profile_sha256")
        if not isinstance(self.identities, TrustedIdentities):
            _error("identities has the wrong type")
        _string(self.baseline, _HEX40, "baseline")
        _string(self.candidate, _HEX40, "candidate")
        if self.baseline == self.candidate:
            _error("baseline and candidate must differ")
        _string(self.pr_body_sha256, _HEX64, "pr_body_sha256")
        _string(self.review_attestation_sha256, _HEX64, "review_attestation_sha256")
        _key_blob(self.public_key_blob)
        if type(self.require_pass) is not bool:
            _error("require_pass must be a boolean")


@dataclass(frozen=True)
class EvidenceArtifact:
    """The exact bytes handed from a trusted producer to the verifier."""

    report: bytes
    signature: bytes
    pr_body: bytes
    review_attestation: bytes
    expectations: VerifierExpectations


@dataclass(frozen=True)
class ReviewAttestation:
    """Decoded canonical evidence of one trusted-base review."""

    repository: str
    pull_request: int
    review_id: int
    reviewer: str
    review_commit: str
    review_state: str
    review_log_sha256: str
    submitted_at: str


@dataclass(frozen=True)
class VerifiedEvidence:
    """The only verifier result the controller may pass to publication."""

    artifact: EvidenceArtifact
    report_sha256: str
    body_sha256: str
    baseline: str
    candidate: str
    profile_sha256: str
    review_state: str
    verdict: str
    review_attestation: ReviewAttestation


def decode_review_attestation(raw: bytes) -> ReviewAttestation:
    """Decode one exact trusted-base review attestation."""

    try:
        value = cj.decode(raw)
        obj = cj.require_object(value, _ATTESTATION_KEYS, "review attestation")
    except cj.CanonicalJsonError as exc:
        raise VerificationError(str(exc)) from exc
    state = _string(obj["review_state"], re.compile(r"[a-z]+\Z"), "review_state")
    if state not in _REVIEW_STATES:
        _error("review_state is not a declared state")
    pull_request = _uint(obj["pull_request"], "pull_request")
    review_id = _uint(obj["review_id"], "review_id")
    if pull_request == 0 or review_id == 0:
        _error("pull_request and review_id must be positive")
    return ReviewAttestation(
        repository=_string(obj["repository"], _NAME, "repository"),
        pull_request=pull_request,
        review_id=review_id,
        reviewer=_string(obj["reviewer"], _NAME, "reviewer"),
        review_commit=_string(obj["review_commit"], _HEX40, "review_commit"),
        review_state=state,
        review_log_sha256=_string(obj["review_log_sha256"], _HEX64, "review_log_sha256"),
        submitted_at=_string(obj["submitted_at"], _TIME, "submitted_at"),
    )


def _marker_lines(raw: bytes) -> list[str]:
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise VerificationError(f"PR body is not UTF-8: {exc}") from exc
    if "\x00" in text or "\r" in text:
        _error("PR body contains NUL or CR bytes")
    return text.split("\n")


def _require_marker(lines: list[str], marker: str) -> None:
    if lines.count(marker) != 1:
        _error(f"PR body must contain exactly one {marker!r} marker")


def _verify_preflight_markers(raw: bytes, *, baseline: str, candidate: str, review: Any) -> None:
    lines = _marker_lines(raw)
    markers = (
        "<!-- align-preflight-version:1 -->",
        f"<!-- align-preflight-head:{candidate} -->",
        "<!-- align-preflight-base-ref:main -->",
        f"<!-- align-preflight-base-sha:{baseline} -->",
        f"<!-- align-preflight-review:{review['state']} -->",
        f"<!-- align-preflight-review-head:{review['review_head']} -->",
        f"<!-- align-preflight-reviewer:{review['reviewer']} -->",
    )
    recognized = [line for line in lines if line.startswith("<!-- align-preflight-")]
    if len(recognized) != len(markers) or set(recognized) != set(markers):
        _error("PR body contains an unknown or conflicting preflight marker")
    for marker in markers:
        _require_marker(lines, marker)


def _verify_review_chain(body: cj.Object, expected: VerifierExpectations) -> None:
    """Bind clean/fixed review state to the reported first-parent inventory."""

    candidate = body["candidate"]
    commits = candidate["commits"]
    commit_ids = [commit["oid"] for commit in commits]
    if not commit_ids or len(set(commit_ids)) != len(commit_ids):
        _error("candidate commit inventory must be nonempty and unique")
    if commit_ids[-1] != expected.candidate:
        _error("candidate commit inventory does not end at the expected candidate")
    by_id = {commit["oid"]: commit for commit in commits}
    previous = expected.baseline
    for oid in commit_ids:
        if by_id[oid]["parents"] != [previous]:
            _error("candidate commit inventory is not an exact first-parent chain")
        previous = oid
    if candidate["parents"] != by_id[commit_ids[-1]]["parents"]:
        _error("candidate revision parents do not match its final commit")
    if by_id[commit_ids[-1]]["tree_oid"] != candidate["tree_oid"]:
        _error("candidate revision tree does not match its final commit")
    if candidate["commit_sha256"] != by_id[commit_ids[-1]]["raw_sha256"]:
        _error("candidate revision digest does not match its final commit")

    review = body["review"]
    repair = list(review["repair_commits"])
    if review["state"] == "clean":
        if review["review_head"] != expected.candidate or repair:
            _error("clean review must name the candidate and have no repair commits")
        return

    if review["review_head"] == expected.candidate or not repair:
        _error("fixed review must name an ancestor and a nonempty repair chain")
    if review["review_head"] == expected.baseline:
        _error("fixed review must name a reviewed commit after the baseline")
    try:
        start = commit_ids.index(review["review_head"])
    except ValueError as exc:
        raise VerificationError("fixed review head is absent from candidate commits") from exc
    if commit_ids[start + 1 :] != repair:
        _error("repair commits are not the exact suffix after the reviewed ancestor")
    previous = review["review_head"]
    for oid in repair:
        if by_id[oid]["parents"] != [previous]:
            _error("repair commits are not an exact first-parent sequence")
        previous = oid
    if previous != expected.candidate:
        _error("repair chain does not end at the candidate")


def _checked_u64_sum(left: int, right: int, label: str) -> int:
    value = left + right
    if value > cj.MAX_U64:
        _error(f"{label} overflows u64")
    return value


def _checked_u64_product(left: int, right: int, label: str) -> int:
    if left != 0 and right > cj.MAX_U64 // left:
        _error(f"{label} overflows u64")
    return left * right


def _sample_token(microseconds: int) -> str:
    return f"{microseconds // 1_000}.{microseconds % 1_000:03d}"


def _verify_host_observations(body: cj.Object, records: list[cj.Object]) -> None:
    observations = body["host_observations"]
    if [observation["ordinal"] for observation in observations] != list(range(len(observations))):
        _error("host observations must have dense ordinals")
    covered: set[int] = set()
    previous_last = -1
    for record in records:
        first = record["monitor_first"]
        last = record["monitor_last"]
        if first >= last or last >= len(observations) or first <= previous_last:
            _error("child monitor ranges must be nonempty, ordered, and disjoint")
        child_id = record["child_id"]
        first_observation = observations[first]
        last_observation = observations[last]
        if (
            first_observation["phase"] != "child-start"
            or first_observation["child_id"] != child_id
            or last_observation["phase"] != "child-end"
            or last_observation["child_id"] != child_id
        ):
            _error("child monitor range boundaries do not match the child")
        for index in range(first, last + 1):
            observation = observations[index]
            if observation["child_id"] != child_id:
                _error("child monitor range contains another child")
            if index not in (first, last) and observation["phase"] != "child-sample":
                _error("child monitor range interior is not a sample")
            covered.add(index)
        previous_last = last
    for index, observation in enumerate(observations):
        if observation["child_id"] == "":
            if observation["phase"] in ("child-start", "child-sample", "child-end"):
                _error("non-child observation has a child phase")
        elif index not in covered:
            _error("orphaned child observation is not named by a report record")


def _verify_report_semantics(body: cj.Object, expected: VerifierExpectations) -> None:
    """Reconstruct every derived report relationship before publication."""

    if body["profile_id"] != expected.identities.profile_id:
        _error("report profile ID does not match the trusted identity bundle")
    for label, actual, trusted in (
        ("producer", body["producer"], expected.identities.producer),
        ("verifier", body["verifier"], expected.identities.verifier),
        ("monitor", body["monitor"], expected.identities.monitor),
        ("execution", body["execution"], expected.identities.execution),
    ):
        if cj.encode(actual) != trusted:
            _error(f"report {label} identity does not match the trusted profile")

    expected_run_id = hashlib.sha256(
        b"align-json-escape-evidence-controller-run-v1\0"
        + expected.profile_sha256.encode("ascii")
        + b"\0"
        + expected.baseline.encode("ascii")
        + b"\0"
        + expected.candidate.encode("ascii")
    ).hexdigest()
    if body["run_id"] != expected_run_id:
        _error("report run ID does not match the trusted invocation")
    if body["started_at"] > body["ended_at"]:
        _error("report end time precedes its start time")

    baseline = body["baseline"]
    if baseline["commits"] or baseline["changed_paths"]:
        _error("baseline revision must not contain candidate inventory")
    candidate = body["candidate"]
    paths = [change["path_hex"] for change in candidate["changed_paths"]]
    if paths != sorted(set(paths)):
        _error("candidate changed paths must be sorted and unique")
    protected = body["protected_inputs"]
    protected_paths = [entry["path_hex"] for entry in protected["entries"]]
    if protected_paths != sorted(set(protected_paths)):
        _error("protected inputs must be sorted and unique")
    if protected["baseline_manifest_sha256"] != protected["candidate_manifest_sha256"]:
        _error("protected input manifests differ")

    records: list[cj.Object] = []
    for benchmark in body["benchmarks"]:
        name = benchmark["name"]
        if (
            benchmark["prepare_argv"] != f"bench/{name}/run.sh prepare native"
            or benchmark["argv"] != f"bench/{name}/run.sh native"
        ):
            _error("benchmark argv does not match its declared name")
        records.extend(benchmark["preparations"])
    for benchmark in body["benchmarks"]:
        records.extend(benchmark["warmups"])
        for ordinal, pair in enumerate(benchmark["pairs"], 1):
            if pair["ordinal"] != ordinal:
                _error("benchmark pair ordinals are not consecutive")
            records.extend((pair["first"], pair["second"]))
    plans = schedule.full_schedule()
    if len(records) != len(plans):
        _error("report child inventory does not cover the fixed schedule")
    child_ids: set[str] = set()
    samples_by_benchmark: dict[str, dict[str, dict[str, list[Any]]]] = {}
    for plan, record in zip(plans, records):
        if record["sequence"] != plan.sequence or record["revision"] != plan.revision:
            _error("report child sequence or revision differs from the fixed schedule")
        if record["child_id"] in child_ids:
            _error("report child ID was reused")
        child_ids.add(record["child_id"])
        if record["exit_code"] != 0:
            _error("report contains a nonzero child result")
        if plan.phase == "prepare":
            continue
        fields = schedule.FIELDS[:2] if plan.benchmark == "json_decode" else schedule.FIELDS[2:]
        if [sample["field"] for sample in record["samples"]] != list(fields):
            _error("run samples do not use the fixed benchmark field order")
        for sample in record["samples"]:
            if sample["microseconds"] == 0 or sample["token"] != _sample_token(sample["microseconds"]):
                _error("run sample token is not the exact integer rendering")
        if plan.phase == "sample":
            benchmark_values = samples_by_benchmark.setdefault(
                plan.benchmark,
                {field: {"baseline": [], "candidate": [], "baseline_tokens": [], "candidate_tokens": []} for field in fields},
            )
            for sample in record["samples"]:
                arm = record["revision"]
                benchmark_values[sample["field"]][arm].append(sample["microseconds"])
                benchmark_values[sample["field"]][f"{arm}_tokens"].append(sample["token"])
    _verify_host_observations(body, records)

    for field_result in body["fields"]:
        benchmark_name = "json_decode" if field_result["field"] in schedule.FIELDS[:2] else "json_soa"
        values = samples_by_benchmark[benchmark_name][field_result["field"]]
        baseline_values = values["baseline"]
        candidate_values = values["candidate"]
        if len(baseline_values) != 10 or len(candidate_values) != 10:
            _error("field result does not have ten samples per revision")
        baseline_sorted = sorted(baseline_values)
        candidate_sorted = sorted(candidate_values)
        baseline_middle_sum = _checked_u64_sum(baseline_sorted[4], baseline_sorted[5], "baseline middle sum")
        candidate_middle_sum = _checked_u64_sum(candidate_sorted[4], candidate_sorted[5], "candidate middle sum")
        if field_result["baseline_tokens"] != values["baseline_tokens"]:
            _error("baseline field tokens do not match the measured samples")
        if field_result["candidate_tokens"] != values["candidate_tokens"]:
            _error("candidate field tokens do not match the measured samples")
        if field_result["baseline_samples_us"] != baseline_values:
            _error("baseline field samples do not match the measured samples")
        if field_result["candidate_samples_us"] != candidate_values:
            _error("candidate field samples do not match the measured samples")
        if field_result["baseline_sorted_us"] != baseline_sorted:
            _error("baseline sorted samples are not the exact permutation")
        if field_result["candidate_sorted_us"] != candidate_sorted:
            _error("candidate sorted samples are not the exact permutation")
        if field_result["baseline_middle_sum"] != baseline_middle_sum:
            _error("baseline middle sum is not reconstructed from samples")
        if field_result["candidate_middle_sum"] != candidate_middle_sum:
            _error("candidate middle sum is not reconstructed from samples")
        if field_result["ratio_numerator"] != candidate_middle_sum:
            _error("ratio numerator is not the candidate middle sum")
        if field_result["ratio_denominator"] != baseline_middle_sum:
            _error("ratio denominator is not the baseline middle sum")
        if baseline_middle_sum == 0:
            _error("baseline middle sum must be positive")
        candidate_threshold = _checked_u64_product(
            candidate_middle_sum,
            100,
            "candidate threshold product",
        )
        baseline_threshold = _checked_u64_product(
            baseline_middle_sum,
            105,
            "baseline threshold product",
        )
        passed = candidate_threshold <= baseline_threshold
        if field_result["passed"] is not passed:
            _error("field pass state does not match the exact threshold comparison")


def _verify_report_bindings(
    report: cj.Object,
    expected: VerifierExpectations,
    attestation: ReviewAttestation,
) -> None:
    body = report["body"]
    _verify_report_semantics(body, expected)
    if body["profile_sha256"] != expected.profile_sha256:
        _error("report profile digest does not match trusted expectations")
    if body["baseline"]["commit_oid"] != expected.baseline:
        _error("report baseline does not match trusted expectations")
    if body["candidate"]["commit_oid"] != expected.candidate:
        _error("report candidate does not match trusted expectations")
    target = body["target"]
    if target["run_oid"] != expected.baseline:
        _error("report target run OID does not match the baseline")
    if target["expected_merge_base"] != expected.baseline:
        _error("report target merge base does not match baseline")
    if target["expected_merge_head"] != expected.candidate:
        _error("report target merge head does not match candidate")
    if target["expected_merge_tree"] != body["candidate"]["tree_oid"]:
        _error("report target merge tree does not match candidate tree")

    review = body["review"]
    _verify_review_chain(body, expected)
    if review["review_base"] != expected.baseline:
        _error("report review base does not match baseline")
    if review["review_head"] != expected.candidate and review["state"] == "clean":
        _error("clean review must be bound to the candidate")
    if review["log_sha256"] != attestation.review_log_sha256:
        _error("report review log digest does not match the attestation")
    if review["review_head"] != attestation.review_commit:
        _error("review attestation commit does not match the report")
    if review["reviewer"] != attestation.reviewer:
        _error("review attestation reviewer does not match the report")
    if review["state"] != attestation.review_state:
        _error("review attestation state does not match the report")

    if expected.require_pass and body["verdict"] != "pass":
        _error("accepted verification requires a pass verdict")


def verify_artifact(
    artifact: EvidenceArtifact,
    signature_checker: Callable[[bytes, sshsig.Signature], bool],
) -> VerifiedEvidence:
    """Verify one complete artifact without performing any external I/O."""

    if not isinstance(artifact, EvidenceArtifact):
        _error("artifact has the wrong type")
    if not callable(signature_checker):
        _error("signature checker is not callable")
    for label, value in (
        ("report", artifact.report),
        ("signature", artifact.signature),
        ("PR body", artifact.pr_body),
        ("review attestation", artifact.review_attestation),
    ):
        if not isinstance(value, bytes):
            _error(f"{label} must be bytes")
    expected = artifact.expectations
    if hashlib.sha256(artifact.pr_body).hexdigest() != expected.pr_body_sha256:
        _error("PR body digest does not match trusted expectations")
    if hashlib.sha256(artifact.review_attestation).hexdigest() != expected.review_attestation_sha256:
        _error("review attestation digest does not match trusted expectations")

    try:
        report = report_schema.decode_report(artifact.report)
    except report_schema.ReportSchemaError as exc:
        raise VerificationError(f"report schema rejected: {exc}") from exc
    attestation = decode_review_attestation(artifact.review_attestation)
    if attestation.repository != expected.repository:
        _error("review attestation repository does not match trusted expectations")
    if attestation.pull_request != expected.pull_request:
        _error("review attestation PR number does not match trusted expectations")
    _verify_preflight_markers(
        artifact.pr_body,
        baseline=expected.baseline,
        candidate=expected.candidate,
        review=report["body"]["review"],
    )
    _verify_report_bindings(report, expected, attestation)

    try:
        signature = sshsig.decode_armor(
            artifact.signature,
            expected_public_key_blob=expected.public_key_blob,
            expected_namespace=sshsig.REPORT_NAMESPACE,
        )
    except sshsig.SSHSigError as exc:
        raise VerificationError(f"signature framing rejected: {exc}") from exc
    try:
        verified = signature_checker(sshsig.signing_preimage(artifact.report, sshsig.REPORT_NAMESPACE), signature)
    except Exception as exc:
        raise VerificationError(f"cryptographic signature check failed: {exc}") from exc
    if verified is not True:
        _error("cryptographic signature check rejected the report")

    return VerifiedEvidence(
        artifact=artifact,
        report_sha256=hashlib.sha256(artifact.report).hexdigest(),
        body_sha256=report["body_sha256"],
        baseline=expected.baseline,
        candidate=expected.candidate,
        profile_sha256=expected.profile_sha256,
        review_state=report["body"]["review"]["state"],
        verdict=report["body"]["verdict"],
        review_attestation=attestation,
    )
