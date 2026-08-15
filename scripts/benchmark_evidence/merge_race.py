"""Fail-closed merge and target-lifecycle state for evidence publication.

The production controller will supply the provider and raw Git object reader.
This module keeps the ordering contract executable without network access: a
fixture-owned remote exposes only validated commit identities, and no state is
accepted until the final target refetch still contains the verified merge.
"""

from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass, replace
from typing import ClassVar


class MergeRaceError(ValueError):
    """A merge or lifecycle transition cannot be accepted."""


_HEX40 = re.compile(r"[0-9a-f]{40}\Z")
_HEX64 = re.compile(r"[0-9a-f]{64}\Z")
_DEFAULT_RESPONSE = object()
_SIGNATURE_DOMAIN = b"merge-verification-test-signature\0"


def _oid(value: object, label: str) -> str:
    if not isinstance(value, str) or _HEX40.fullmatch(value) is None:
        raise MergeRaceError(f"{label} must be a lowercase 40-hex object id")
    return value


def _digest(value: object, label: str) -> str:
    if not isinstance(value, str) or _HEX64.fullmatch(value) is None:
        raise MergeRaceError(f"{label} must be a lowercase 64-hex digest")
    return value


@dataclass(frozen=True)
class RemoteCommit:
    """The raw identity facts needed by the merge verifier fixture."""

    oid: str
    parents: tuple[str, ...]
    tree_oid: str
    raw_sha256: str

    def __post_init__(self) -> None:
        _oid(self.oid, "commit oid")
        if not isinstance(self.parents, tuple):
            raise MergeRaceError("commit parents must preserve tuple order")
        for parent in self.parents:
            _oid(parent, "commit parent")
        if len(set(self.parents)) != len(self.parents):
            raise MergeRaceError("commit parents must be unique")
        _oid(self.tree_oid, "commit tree oid")
        _digest(self.raw_sha256, "raw commit digest")


@dataclass(frozen=True)
class MergeResponse:
    """The provider's returned merge identity; None models an unavailable response."""

    merge_oid: str | None

    def __post_init__(self) -> None:
        if self.merge_oid is not None:
            _oid(self.merge_oid, "merge response oid")


def _signed_payload(commit: RemoteCommit) -> bytes:
    parents = b"".join(parent.encode("ascii") + b"\n" for parent in commit.parents)
    return (
        commit.oid.encode("ascii")
        + b"\n"
        + commit.raw_sha256.encode("ascii")
        + b"\n"
        + parents
        + commit.tree_oid.encode("ascii")
        + b"\n"
    )


@dataclass(frozen=True)
class SignedMergeArtifact:
    """A deterministic stand-in for the signed merge-verification record."""

    merge_oid: str
    merge_sha256: str
    parents: tuple[str, ...]
    tree_oid: str
    signature_sha256: str

    def __post_init__(self) -> None:
        _oid(self.merge_oid, "artifact merge oid")
        _digest(self.merge_sha256, "artifact merge digest")
        if not isinstance(self.parents, tuple) or len(self.parents) != 2:
            raise MergeRaceError("artifact must contain exactly two ordered parents")
        for parent in self.parents:
            _oid(parent, "artifact parent")
        if len(set(self.parents)) != 2:
            raise MergeRaceError("artifact parents must be unique")
        _oid(self.tree_oid, "artifact tree oid")
        _digest(self.signature_sha256, "artifact signature digest")

    @classmethod
    def from_commit(cls, commit: RemoteCommit) -> "SignedMergeArtifact":
        signature = hashlib.sha256(_SIGNATURE_DOMAIN + _signed_payload(commit)).hexdigest()
        return cls(
            merge_oid=commit.oid,
            merge_sha256=commit.raw_sha256,
            parents=commit.parents,
            tree_oid=commit.tree_oid,
            signature_sha256=signature,
        )

    def has_valid_signature(self) -> bool:
        payload = (
            self.merge_oid.encode("ascii")
            + b"\n"
            + self.merge_sha256.encode("ascii")
            + b"\n"
            + b"".join(parent.encode("ascii") + b"\n" for parent in self.parents)
            + self.tree_oid.encode("ascii")
            + b"\n"
        )
        expected = hashlib.sha256(_SIGNATURE_DOMAIN + payload).hexdigest()
        return self.signature_sha256 == expected

    def matches(self, commit: RemoteCommit) -> bool:
        return (
            self.merge_oid == commit.oid
            and self.merge_sha256 == commit.raw_sha256
            and self.parents == commit.parents
            and self.tree_oid == commit.tree_oid
            and self.has_valid_signature()
        )


class MergeArtifactStore:
    """Small fixture-owned store whose contents can be mutated before finalization."""

    def __init__(self) -> None:
        self._artifact: SignedMergeArtifact | None = None

    @property
    def artifact(self) -> SignedMergeArtifact | None:
        return self._artifact

    def put(self, artifact: SignedMergeArtifact) -> None:
        if not isinstance(artifact, SignedMergeArtifact):
            raise MergeRaceError("merge artifact has the wrong type")
        self._artifact = artifact

    def tamper(self, **changes: object) -> None:
        if self._artifact is None:
            raise MergeRaceError("no merge artifact is staged")
        try:
            self._artifact = replace(self._artifact, **changes)
        except TypeError as exc:
            raise MergeRaceError("merge artifact mutation is not a known field") from exc

    def clear(self) -> None:
        self._artifact = None


class DisposableRemote:
    """A validated in-memory remote used to exercise provider/lifecycle races."""

    target_ref: ClassVar[str] = "refs/heads/main"

    def __init__(self, base: RemoteCommit, candidate: RemoteCommit) -> None:
        if candidate.parents != (base.oid,):
            raise MergeRaceError("candidate must be a direct child of base")
        self._base_oid = base.oid
        self._commits: dict[str, RemoteCommit] = {}
        self.add_commit(base)
        self.add_commit(candidate)
        self._target_oid: str | None = base.oid
        self._queued_merge: RemoteCommit | None = None
        self._queued_response_oid: str | None = None
        self._queued_response_set = False
        self._last_merge_oid: str | None = None
        self._revert_failure = False
        self._fetch_failures: set[str] = set()
        self._blocked = False

    @property
    def target_oid(self) -> str | None:
        return self._target_oid

    @property
    def base_oid(self) -> str:
        return self._base_oid

    @property
    def blocked(self) -> bool:
        return self._blocked

    @property
    def last_merge_oid(self) -> str | None:
        return self._last_merge_oid

    def add_commit(self, commit: RemoteCommit) -> None:
        if not isinstance(commit, RemoteCommit):
            raise MergeRaceError("remote commit has the wrong type")
        previous = self._commits.get(commit.oid)
        if previous is not None and previous != commit:
            raise MergeRaceError("remote cannot replace an object identity with different bytes")
        self._commits[commit.oid] = commit

    def replace_commit(self, commit: RemoteCommit) -> None:
        """Model an adversarial raw-object replacement in the fixture."""

        if not isinstance(commit, RemoteCommit):
            raise MergeRaceError("remote commit has the wrong type")
        if commit.oid not in self._commits:
            raise MergeRaceError("cannot replace an unknown remote object")
        self._commits[commit.oid] = commit

    def remove_commit(self, oid: str) -> None:
        oid = _oid(oid, "remote object oid")
        if oid in (self._base_oid, self._target_oid):
            raise MergeRaceError("cannot remove the base or target object")
        self._commits.pop(oid, None)

    def set_target(self, oid: str | None) -> None:
        if oid is not None:
            oid = _oid(oid, "target oid")
            if oid not in self._commits:
                raise MergeRaceError("target must reference a known commit")
        self._target_oid = oid

    def queue_merge(
        self,
        commit: RemoteCommit,
        *,
        response_oid: str | None | object = _DEFAULT_RESPONSE,
    ) -> None:
        self.add_commit(commit)
        if response_oid is _DEFAULT_RESPONSE:
            response_oid = commit.oid
        if response_oid is not None:
            _oid(response_oid, "queued response oid")
        self._queued_merge = commit
        self._queued_response_oid = response_oid
        self._queued_response_set = True

    def merge(self) -> MergeResponse:
        if self._blocked:
            raise MergeRaceError("remote is blocked for administrator recovery")
        if not self._queued_response_set or self._queued_merge is None:
            raise MergeRaceError("remote has no queued merge fixture")
        merge = self._queued_merge
        self._target_oid = merge.oid
        self._last_merge_oid = merge.oid
        response = MergeResponse(self._queued_response_oid)
        self._queued_merge = None
        self._queued_response_set = False
        return response

    def fetch_commit(self, oid: str) -> RemoteCommit:
        oid = _oid(oid, "fetched commit oid")
        if oid in self._fetch_failures:
            raise MergeRaceError("fetched commit is unavailable")
        try:
            return self._commits[oid]
        except KeyError as exc:
            raise MergeRaceError("fetched commit is unavailable") from exc

    def set_fetch_failure(self, oid: str, value: bool = True) -> None:
        oid = _oid(oid, "fetch failure object oid")
        if type(value) is not bool:
            raise MergeRaceError("fetch failure flag must be boolean")
        if value:
            self._fetch_failures.add(oid)
        else:
            self._fetch_failures.discard(oid)

    def set_revert_failure(self, value: bool) -> None:
        if type(value) is not bool:
            raise MergeRaceError("revert failure flag must be boolean")
        self._revert_failure = value

    def revert(self, merge_oid: str) -> bool:
        merge_oid = _oid(merge_oid, "revert merge oid")
        if self._revert_failure or self._target_oid != merge_oid or self._blocked:
            return False
        self._target_oid = self._base_oid
        return True

    def block(self) -> None:
        self._blocked = True

    def first_parent_contains(self, oid: str) -> bool:
        oid = _oid(oid, "first-parent merge oid")
        current = self._target_oid
        seen: set[str] = set()
        while current is not None and current not in seen:
            seen.add(current)
            if current == oid:
                return True
            commit = self._commits.get(current)
            if commit is None:
                return False
            current = commit.parents[0] if commit.parents else None
        return False


class MergeRaceTransaction:
    """State machine enforcing the merge, verification, and lifecycle ordering."""

    _STATES: ClassVar[tuple[str, ...]] = (
        "new",
        "prechecked",
        "merged",
        "verified",
        "staged",
        "accepted",
        "rejected",
        "reverted",
        "blocked",
    )

    def __init__(
        self,
        remote: DisposableRemote,
        *,
        base_oid: str,
        candidate_oid: str,
        candidate_tree_oid: str,
    ) -> None:
        if not isinstance(remote, DisposableRemote):
            raise MergeRaceError("merge transaction requires a disposable remote")
        self._remote = remote
        self._base_oid = _oid(base_oid, "base oid")
        self._candidate_oid = _oid(candidate_oid, "candidate oid")
        self._candidate_tree_oid = _oid(candidate_tree_oid, "candidate tree oid")
        if self._base_oid != remote.base_oid:
            raise MergeRaceError("transaction base does not match the disposable remote")
        try:
            candidate = remote.fetch_commit(self._candidate_oid)
        except MergeRaceError as exc:
            raise MergeRaceError("transaction candidate object is unavailable") from exc
        if candidate.parents != (self._base_oid,):
            raise MergeRaceError("transaction candidate is not a direct child of base")
        if candidate.tree_oid != self._candidate_tree_oid:
            raise MergeRaceError("transaction candidate tree does not match the bound tree")
        self._state = "new"
        self._merge_oid: str | None = None
        self._merge_commit: RemoteCommit | None = None
        self._store = MergeArtifactStore()
        self._lifecycle_advanced = False
        self._last_error: str | None = None

    @property
    def state(self) -> str:
        return self._state

    @property
    def lifecycle_advanced(self) -> bool:
        return self._lifecycle_advanced

    @property
    def merge_oid(self) -> str | None:
        return self._merge_oid

    @property
    def artifact_present(self) -> bool:
        return self._store.artifact is not None

    @property
    def last_error(self) -> str | None:
        return self._last_error

    def _require(self, state: str) -> None:
        if self._state != state:
            raise MergeRaceError(f"merge transaction is {self._state}, expected {state}")

    def _fail(self, message: str, *, revert: bool = False, block: bool = False) -> None:
        self._last_error = message
        self._store.clear()
        if block:
            self._remote.block()
            self._state = "blocked"
        elif revert and self._merge_oid is not None:
            if self._remote.revert(self._merge_oid):
                self._state = "reverted"
            else:
                self._remote.block()
                self._state = "blocked"
        else:
            self._state = "rejected"
        raise MergeRaceError(message)

    def precheck(self, local_target_oid: str) -> None:
        self._require("new")
        try:
            local_target_oid = _oid(local_target_oid, "local target oid")
        except MergeRaceError as exc:
            self._fail(str(exc))
        if local_target_oid != self._base_oid:
            self._fail("local target does not equal base")
        if self._remote.target_oid != self._base_oid:
            self._fail("remote target moved before precheck")
        self._state = "prechecked"

    def merge(self) -> None:
        self._require("prechecked")
        if self._remote.target_oid != self._base_oid:
            self._fail("remote target moved after precheck and before merge")
        try:
            response = self._remote.merge()
        except MergeRaceError as exc:
            self._fail(str(exc), block=True)
        if response.merge_oid is None:
            self._fail("merge response is unavailable", block=True)
        try:
            commit = self._remote.fetch_commit(response.merge_oid)
        except MergeRaceError as exc:
            self._fail(str(exc), block=True)
        if commit.oid != response.merge_oid:
            self._fail("merge response identity does not match fetched object", block=True)
        self._merge_oid = commit.oid
        self._merge_commit = commit
        self._state = "merged"

    def verify_merge(self) -> None:
        self._require("merged")
        if self._merge_commit is None:
            self._fail("merge object was not retained for verification", block=True)
        if self._merge_commit.parents != (self._base_oid, self._candidate_oid):
            self._fail("merge parents are not exactly (base, candidate)", revert=True)
        if self._merge_commit.tree_oid != self._candidate_tree_oid:
            self._fail("merge tree does not equal candidate tree", revert=True)
        self._state = "verified"

    def store_artifact(self, artifact: SignedMergeArtifact) -> None:
        self._require("verified")
        if self._merge_commit is None:
            self._fail("merge object was not retained for artifact binding", block=True)
        if not isinstance(artifact, SignedMergeArtifact) or not artifact.matches(self._merge_commit):
            self._fail("signed merge artifact does not match the verified merge", revert=True)
        self._store.put(artifact)
        self._state = "staged"

    def tamper_artifact(self, **changes: object) -> None:
        self._require("staged")
        try:
            self._store.tamper(**changes)
        except MergeRaceError as exc:
            self._fail(str(exc), revert=True)

    def finalize(self) -> "MergeRaceResult":
        self._require("staged")
        if self._merge_oid is None:
            self._fail("merge identity was not retained for final refetch", block=True)
        if not self._remote.first_parent_contains(self._merge_oid):
            self._fail("final target does not retain merge on its first-parent chain")
        try:
            final_commit = self._remote.fetch_commit(self._merge_oid)
        except MergeRaceError as exc:
            self._fail(str(exc))
        artifact = self._store.artifact
        if artifact is None or not artifact.matches(final_commit):
            self._fail("final merge refetch does not match signed artifact", revert=True)
        self._state = "accepted"
        self._lifecycle_advanced = True
        return MergeRaceResult(
            state=self._state,
            lifecycle_advanced=True,
            merge_oid=self._merge_oid,
            target_oid=self._remote.target_oid,
            artifact_present=True,
        )


@dataclass(frozen=True)
class MergeRaceResult:
    state: str
    lifecycle_advanced: bool
    merge_oid: str | None
    target_oid: str | None
    artifact_present: bool
