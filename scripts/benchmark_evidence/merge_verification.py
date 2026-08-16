"""Produce and verify the signed post-merge evidence record.

This module is the pure boundary between an already verified report and the
provider's raw merge object.  A trusted caller supplies the pinned raw-Git
reader, the report signature checker, and a namespace-bound signer.  The
module owns canonical record formation and all identity/reachability joins; it
does not open keys, call a provider, mutate refs, or publish files.
"""

from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass
from typing import Callable, Protocol

from . import canonical_json as cj
from . import git_objects
from . import git_revision
from . import report_schema
from . import sshsig
from . import verifier


class MergeVerificationError(ValueError):
    """A report, merge object, or signed merge record is not acceptable."""


class RawGitMergeReader(Protocol):
    """The trusted raw-object port needed by post-merge verification."""

    def read(self, oid: str) -> git_objects.VerifiedObject:
        ...

    def target_oid(self) -> str:
        ...


SCHEMA = "align.json_escape_benchmark_merge_verification/v1"
TARGET_REF = git_revision.TARGET_REF
_RECORD_KEYS = (
    "schema",
    "profile_id",
    "profile_sha256",
    "verifier",
    "report_sha256",
    "report_signature_sha256",
    "target_ref",
    "target_oid",
    "merge_oid",
    "merge_sha256",
    "parents",
    "tree_oid",
    "verified_at",
)
_NAME = re.compile(r"[A-Za-z0-9._/:+=@-]{1,255}\Z")
_HEX40 = re.compile(r"[0-9a-f]{40}\Z")
_HEX64 = re.compile(r"[0-9a-f]{64}\Z")
_TIME = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{9}Z\Z")

ReportSignatureChecker = Callable[[bytes, sshsig.Signature], bool]
MergeSigner = Callable[[bytes], bytes]


def _error(message: str) -> None:
    raise MergeVerificationError(message)


def _string(value: object, pattern: re.Pattern[str], label: str) -> str:
    if not isinstance(value, str) or pattern.fullmatch(value) is None:
        _error(f"{label} has the wrong grammar")
    return value


def _oid(value: object, label: str) -> str:
    return _string(value, _HEX40, label)


def _digest(value: object, label: str) -> str:
    return _string(value, _HEX64, label)


def _identity(value: object, label: str) -> cj.Object:
    if not isinstance(value, cj.Object):
        _error(f"{label} must preserve canonical object order")
    try:
        return report_schema.validate_tool_identity(value, label)
    except report_schema.ReportSchemaError as exc:
        raise MergeVerificationError(str(exc)) from exc


@dataclass(frozen=True)
class MergeVerificationRecord:
    """The exact canonical post-merge verification record."""

    profile_id: str
    profile_sha256: str
    verifier: cj.Object
    report_sha256: str
    report_signature_sha256: str
    target_oid: str
    merge_oid: str
    merge_sha256: str
    parents: tuple[str, ...]
    tree_oid: str
    verified_at: str

    def __post_init__(self) -> None:
        _string(self.profile_id, _NAME, "profile_id")
        _digest(self.profile_sha256, "profile_sha256")
        _identity(self.verifier, "verifier")
        _digest(self.report_sha256, "report_sha256")
        _digest(self.report_signature_sha256, "report_signature_sha256")
        _oid(self.target_oid, "target_oid")
        _oid(self.merge_oid, "merge_oid")
        _digest(self.merge_sha256, "merge_sha256")
        if not isinstance(self.parents, tuple) or len(self.parents) != 2:
            _error("parents must contain exactly two ordered commit IDs")
        for parent in self.parents:
            _oid(parent, "merge parent")
        if self.parents[0] == self.parents[1]:
            _error("merge parents must be distinct")
        _oid(self.tree_oid, "tree_oid")
        _string(self.verified_at, _TIME, "verified_at")

    def as_object(self) -> cj.Object:
        """Return the ordered canonical JSON value."""

        return cj.Object(
            (
                ("schema", SCHEMA),
                ("profile_id", self.profile_id),
                ("profile_sha256", self.profile_sha256),
                ("verifier", self.verifier),
                ("report_sha256", self.report_sha256),
                ("report_signature_sha256", self.report_signature_sha256),
                ("target_ref", TARGET_REF),
                ("target_oid", self.target_oid),
                ("merge_oid", self.merge_oid),
                ("merge_sha256", self.merge_sha256),
                ("parents", list(self.parents)),
                ("tree_oid", self.tree_oid),
                ("verified_at", self.verified_at),
            )
        )

    def encode(self) -> bytes:
        """Encode one canonical record, including its final LF."""

        return cj.encode(self.as_object())

    @classmethod
    def from_object(cls, value: object) -> "MergeVerificationRecord":
        try:
            obj = cj.require_object(value, _RECORD_KEYS, "merge verification")
        except cj.CanonicalJsonError as exc:
            raise MergeVerificationError(str(exc)) from exc
        if obj["schema"] != SCHEMA or type(obj["schema"]) is not str:
            _error("merge verification schema is not v1")
        if obj["target_ref"] != TARGET_REF or type(obj["target_ref"]) is not str:
            _error("merge verification target ref is not refs/heads/main")
        parents = obj["parents"]
        if not isinstance(parents, list):
            _error("merge verification parents must be an array")
        return cls(
            profile_id=_string(obj["profile_id"], _NAME, "profile_id"),
            profile_sha256=_digest(obj["profile_sha256"], "profile_sha256"),
            verifier=_identity(obj["verifier"], "verifier"),
            report_sha256=_digest(obj["report_sha256"], "report_sha256"),
            report_signature_sha256=_digest(
                obj["report_signature_sha256"], "report_signature_sha256"
            ),
            target_oid=_oid(obj["target_oid"], "target_oid"),
            merge_oid=_oid(obj["merge_oid"], "merge_oid"),
            merge_sha256=_digest(obj["merge_sha256"], "merge_sha256"),
            parents=tuple(_oid(parent, "merge parent") for parent in parents),
            tree_oid=_oid(obj["tree_oid"], "tree_oid"),
            verified_at=_string(obj["verified_at"], _TIME, "verified_at"),
        )

    @classmethod
    def decode(cls, raw: bytes) -> "MergeVerificationRecord":
        """Decode one canonical record and reject every byte mutation."""

        try:
            value = cj.decode(raw)
        except cj.CanonicalJsonError as exc:
            raise MergeVerificationError(str(exc)) from exc
        record = cls.from_object(value)
        if record.encode() != raw:
            _error("merge verification record is not canonical")
        return record


@dataclass(frozen=True)
class MergeVerificationArtifact:
    """The immutable record/signature pair handed to durable staging."""

    record: bytes
    signature: bytes

    def __post_init__(self) -> None:
        if not isinstance(self.record, bytes):
            _error("merge verification record must be bytes")
        if not isinstance(self.signature, bytes):
            _error("merge verification signature must be bytes")


@dataclass(frozen=True)
class _ObservedMerge:
    target_oid: str
    merge_oid: str
    merge_sha256: str
    parents: tuple[str, ...]
    tree_oid: str


def _verify_signature(
    message: bytes,
    signature_bytes: bytes,
    public_key_blob: bytes,
    namespace: bytes,
    checker: ReportSignatureChecker,
) -> None:
    if not callable(checker):
        _error("signature checker is not callable")
    try:
        signature = sshsig.decode_armor(
            signature_bytes,
            expected_public_key_blob=public_key_blob,
            expected_namespace=namespace,
        )
    except sshsig.SSHSigError as exc:
        raise MergeVerificationError(f"signature framing rejected: {exc}") from exc
    try:
        valid = checker(message, signature)
    except Exception as exc:
        raise MergeVerificationError(f"signature check failed: {exc}") from exc
    if valid is not True:
        _error("signature check rejected the complete message")


def _verify_report(
    report: bytes,
    report_signature: bytes,
    expectations: verifier.ReportExpectations,
    checker: ReportSignatureChecker,
) -> cj.Object:
    if not isinstance(expectations, verifier.ReportExpectations):
        _error("report expectations have the wrong type")
    try:
        checked = verifier.verify_produced_evidence(
            verifier.ProducedEvidence(report, report_signature, expectations),
            checker,
        )
    except verifier.VerificationError as exc:
        raise MergeVerificationError(f"report verification failed: {exc}") from exc
    if checked.verdict != "pass":
        _error("merge verification requires a passing report")
    try:
        return report_schema.decode_report(report)
    except report_schema.ReportSchemaError as exc:
        raise MergeVerificationError(f"report schema rejected: {exc}") from exc


def _read(reader: RawGitMergeReader, oid: str) -> git_objects.VerifiedObject:
    if not callable(getattr(reader, "read", None)):
        _error("raw merge reader has no read port")
    try:
        value = reader.read(oid)
    except Exception as exc:
        raise MergeVerificationError(f"raw Git object {oid} is unavailable") from exc
    if not isinstance(value, git_objects.VerifiedObject) or value.oid != oid:
        _error("raw merge reader returned an unbound object")
    try:
        reconstructed = git_objects.verify(oid, git_objects.encode(value.kind, value.payload))
    except git_objects.GitObjectError as exc:
        raise MergeVerificationError(f"raw merge reader returned an invalid object: {exc}") from exc
    if reconstructed.raw_sha256 != value.raw_sha256:
        _error("raw merge reader changed the object digest")
    return reconstructed


def _observe_merge(
    reader: RawGitMergeReader,
    merge_oid: str,
    report: cj.Object,
    expectations: verifier.ReportExpectations,
) -> _ObservedMerge:
    merge_oid = _oid(merge_oid, "merge oid")
    if not callable(getattr(reader, "target_oid", None)):
        _error("raw merge reader has no target_oid port")
    try:
        target_oid = _oid(reader.target_oid(), "target oid")
    except MergeVerificationError:
        raise
    except Exception as exc:
        raise MergeVerificationError("target ref could not be resolved") from exc

    target = report["body"]["target"]
    candidate = report["body"]["candidate"]
    if target["local_ref"] != TARGET_REF:
        _error("report target ref is not refs/heads/main")
    if target["expected_merge_base"] != expectations.baseline:
        _error("report merge base is not the trusted baseline")
    if target["expected_merge_head"] != expectations.candidate:
        _error("report merge head is not the trusted candidate")
    if target["expected_merge_tree"] != candidate["tree_oid"]:
        _error("report merge tree is not the candidate tree")

    current = target_oid
    visited: set[str] = set()
    merge_object: git_objects.VerifiedObject | None = None
    merge_commit: git_revision.CommitRecord | None = None
    while current is not None and current not in visited:
        if len(visited) >= git_revision.MAX_TREE_ENTRIES:
            _error("target first-parent chain exceeds the fixed bound")
        visited.add(current)
        value = _read(reader, current)
        if value.kind != "commit":
            _error("target first-parent chain contains a non-commit object")
        try:
            commit = git_revision.parse_commit(value)
        except git_revision.GitRevisionError as exc:
            raise MergeVerificationError(f"target commit is malformed: {exc}") from exc
        if current == merge_oid:
            merge_object = value
            merge_commit = commit
            break
        current = commit.parents[0] if commit.parents else None

    if merge_object is None or merge_commit is None:
        if current in visited:
            _error("target first-parent chain contains a cycle before the merge")
        _error("target first-parent chain does not contain the merge")
    if merge_commit.parents != (expectations.baseline, expectations.candidate):
        _error("merge parents are not exactly baseline then candidate")
    if merge_commit.tree_oid != candidate["tree_oid"]:
        _error("merge tree does not equal the signed candidate tree")
    return _ObservedMerge(
        target_oid=target_oid,
        merge_oid=merge_oid,
        merge_sha256=merge_object.raw_sha256,
        parents=merge_commit.parents,
        tree_oid=merge_commit.tree_oid,
    )


def _trusted_verifier_identity(expectations: verifier.ReportExpectations) -> cj.Object:
    try:
        identity = cj.decode(expectations.identities.verifier)
    except cj.CanonicalJsonError as exc:
        raise MergeVerificationError("trusted verifier identity is not canonical") from exc
    return _identity(identity, "trusted verifier identity")


def _record(
    report: bytes,
    report_signature: bytes,
    expectations: verifier.ReportExpectations,
    observed: _ObservedMerge,
    verified_at: str,
) -> MergeVerificationRecord:
    return MergeVerificationRecord(
        profile_id=expectations.identities.profile_id,
        profile_sha256=expectations.profile_sha256,
        verifier=_trusted_verifier_identity(expectations),
        report_sha256=hashlib.sha256(report).hexdigest(),
        report_signature_sha256=hashlib.sha256(report_signature).hexdigest(),
        target_oid=observed.target_oid,
        merge_oid=observed.merge_oid,
        merge_sha256=observed.merge_sha256,
        parents=observed.parents,
        tree_oid=observed.tree_oid,
        verified_at=_string(verified_at, _TIME, "verified_at"),
    )


def _verify_merge_signature(
    artifact: MergeVerificationArtifact,
    expectations: verifier.ReportExpectations,
    checker: ReportSignatureChecker,
) -> MergeVerificationRecord:
    record = MergeVerificationRecord.decode(artifact.record)
    _verify_signature(
        artifact.record,
        artifact.signature,
        expectations.public_key_blob,
        sshsig.MERGE_NAMESPACE,
        checker,
    )
    return record


def produce_signed(
    report: bytes,
    report_signature: bytes,
    expectations: verifier.ReportExpectations,
    reader: RawGitMergeReader,
    merge_oid: str,
    verified_at: str,
    report_checker: ReportSignatureChecker,
    signer: MergeSigner,
    signature_checker: ReportSignatureChecker,
) -> MergeVerificationArtifact:
    """Produce one canonical signed merge-verification artifact."""

    if not isinstance(report, bytes) or not isinstance(report_signature, bytes):
        _error("report and report signature must be bytes")
    if not callable(signer):
        _error("merge signer is not callable")
    report_value = _verify_report(report, report_signature, expectations, report_checker)
    observed = _observe_merge(reader, merge_oid, report_value, expectations)
    record = _record(report, report_signature, expectations, observed, verified_at)
    record_bytes = record.encode()
    try:
        signature = signer(record_bytes)
    except Exception as exc:
        raise MergeVerificationError("merge record signing failed") from exc
    if not isinstance(signature, bytes):
        _error("merge signer returned a non-byte signature")
    artifact = MergeVerificationArtifact(record_bytes, signature)
    _verify_merge_signature(artifact, expectations, signature_checker)
    return artifact


def verify_signed(
    artifact: MergeVerificationArtifact,
    report: bytes,
    report_signature: bytes,
    expectations: verifier.ReportExpectations,
    reader: RawGitMergeReader,
    merge_oid: str,
    report_checker: ReportSignatureChecker,
    signature_checker: ReportSignatureChecker,
) -> MergeVerificationRecord:
    """Verify an artifact against a fresh report and raw target observation."""

    if not isinstance(artifact, MergeVerificationArtifact):
        _error("merge verification artifact has the wrong type")
    report_value = _verify_report(report, report_signature, expectations, report_checker)
    record = _verify_merge_signature(artifact, expectations, signature_checker)
    merge_oid = _oid(merge_oid, "merge oid")
    if record.merge_oid != merge_oid:
        _error("merge artifact is bound to a different merge OID")
    if record.report_sha256 != hashlib.sha256(report).hexdigest():
        _error("merge artifact report digest does not match the report bytes")
    if record.report_signature_sha256 != hashlib.sha256(report_signature).hexdigest():
        _error("merge artifact report-signature digest does not match the signature bytes")
    observed = _observe_merge(reader, merge_oid, report_value, expectations)
    expected_record = _record(
        report,
        report_signature,
        expectations,
        observed,
        record.verified_at,
    )
    if record != expected_record:
        _error("merge artifact does not match the fresh raw-object observation")
    return record


__all__ = [
    "MergeVerificationArtifact",
    "MergeVerificationError",
    "MergeVerificationRecord",
    "RawGitMergeReader",
    "produce_signed",
    "verify_signed",
]
