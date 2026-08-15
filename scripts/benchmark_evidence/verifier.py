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


@dataclass(frozen=True)
class VerifierExpectations:
    """Trusted-base values that are never selected by report bytes."""

    repository: str
    pull_request: int
    profile_sha256: str
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
    for marker in markers:
        _require_marker(lines, marker)


def _verify_report_bindings(
    report: cj.Object,
    expected: VerifierExpectations,
    attestation: ReviewAttestation,
) -> None:
    body = report["body"]
    if body["profile_sha256"] != expected.profile_sha256:
        _error("report profile digest does not match trusted expectations")
    if body["baseline"]["commit_oid"] != expected.baseline:
        _error("report baseline does not match trusted expectations")
    if body["candidate"]["commit_oid"] != expected.candidate:
        _error("report candidate does not match trusted expectations")
    target = body["target"]
    if target["expected_merge_base"] != expected.baseline:
        _error("report target merge base does not match baseline")
    if target["expected_merge_head"] != expected.candidate:
        _error("report target merge head does not match candidate")
    if target["expected_merge_tree"] != body["candidate"]["tree_oid"]:
        _error("report target merge tree does not match candidate tree")

    review = body["review"]
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
        report_sha256=hashlib.sha256(artifact.report).hexdigest(),
        body_sha256=report["body_sha256"],
        baseline=expected.baseline,
        candidate=expected.candidate,
        profile_sha256=expected.profile_sha256,
        review_state=report["body"]["review"]["state"],
        verdict=report["body"]["verdict"],
        review_attestation=attestation,
    )
