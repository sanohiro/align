#!/usr/bin/env bash
# Deterministic owner for merge-race and target-lifecycle ordering.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
from __future__ import annotations

import hashlib

from scripts.benchmark_evidence.merge_race import (
    DisposableRemote,
    MergeRaceError,
    MergeRaceTransaction,
    RemoteCommit,
    SignedMergeArtifact,
)


BASE = "a" * 40
CANDIDATE = "b" * 40
MERGE = "c" * 40
WRONG_PARENT = "d" * 40
WRONG_TREE = "e" * 40
UNRELATED = "f" * 40
SIDE_PARENT = "5" * 40
OTHER_MERGE = "6" * 40
LATER = "1" * 40
BASE_TREE = "2" * 40
CANDIDATE_TREE = "3" * 40
WRONG_TREE_OID = "4" * 40


def digest(label: str) -> str:
    return hashlib.sha256(label.encode("ascii")).hexdigest()


def commit(
    oid: str,
    parents: tuple[str, ...],
    tree_oid: str,
    label: str,
) -> RemoteCommit:
    return RemoteCommit(oid, parents, tree_oid, digest(label))


def fixture() -> tuple[
    RemoteCommit,
    RemoteCommit,
    RemoteCommit,
    DisposableRemote,
]:
    base = commit(BASE, (), BASE_TREE, "base")
    candidate = commit(CANDIDATE, (BASE,), CANDIDATE_TREE, "candidate")
    merge = commit(MERGE, (BASE, CANDIDATE), CANDIDATE_TREE, "merge")
    return base, candidate, merge, DisposableRemote(base, candidate)


def transaction(
    remote: DisposableRemote,
    base: RemoteCommit,
    candidate: RemoteCommit,
) -> MergeRaceTransaction:
    return MergeRaceTransaction(
        remote,
        base_oid=base.oid,
        candidate_oid=candidate.oid,
        candidate_tree_oid=candidate.tree_oid,
    )


def staged(
    remote: DisposableRemote,
    base: RemoteCommit,
    candidate: RemoteCommit,
    merge: RemoteCommit,
) -> MergeRaceTransaction:
    remote.queue_merge(merge)
    current = transaction(remote, base, candidate)
    current.precheck(base.oid)
    current.merge()
    current.verify_merge()
    current.store_artifact(SignedMergeArtifact.from_commit(merge))
    return current


def expect_error(label: str, action) -> None:
    try:
        action()
    except MergeRaceError:
        return
    raise AssertionError(f"{label} was accepted")


def assert_unadvanced(current: MergeRaceTransaction) -> None:
    assert not current.lifecycle_advanced
    assert not current.artifact_present
    assert current.state in {"rejected", "reverted", "blocked"}


base, candidate, merge, remote = fixture()
expect_error(
    "candidate tree mismatch at binding",
    lambda: MergeRaceTransaction(
        remote,
        base_oid=BASE,
        candidate_oid=CANDIDATE,
        candidate_tree_oid=WRONG_TREE_OID,
    ),
)

base, candidate, merge, remote = fixture()
remote.target_ref = "refs/heads/release"
expect_error(
    "unexpected target ref",
    lambda: transaction(remote, base, candidate),
)

base, candidate, merge, remote = fixture()
remote.queue_merge(merge)
current = transaction(remote, base, candidate)
current.precheck(base.oid)
current.merge()
current.verify_merge()
current.store_artifact(SignedMergeArtifact.from_commit(merge))
result = current.finalize()
assert result.state == "accepted"
assert result.lifecycle_advanced
assert result.merge_oid == MERGE
assert result.target_oid == MERGE
assert result.artifact_present

base, candidate, merge, remote = fixture()
current = staged(remote, base, candidate, merge)
later = commit(LATER, (MERGE,), CANDIDATE_TREE, "later")
remote.add_commit(later)
remote.set_target(later.oid)
result = current.finalize()
assert result.state == "accepted"
assert result.target_oid == LATER

base, candidate, merge, remote = fixture()
current = staged(remote, base, candidate, merge)
remote.target_ref = "refs/heads/release"
expect_error("target ref movement before final refetch", current.finalize)
assert current.state == "rejected"
assert remote.target_oid == MERGE
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
current = staged(remote, base, candidate, merge)
later = commit(LATER, (MERGE,), CANDIDATE_TREE, "unavailable-later")
remote.add_commit(later)
remote.set_target(later.oid)
remote.set_fetch_failure(LATER)
expect_error("unavailable descendant fetch", current.finalize)
assert current.state == "rejected"
assert remote.target_oid == LATER
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
unrelated = commit(UNRELATED, (), BASE_TREE, "side-parent-root")
side_parent = commit(
    SIDE_PARENT,
    (UNRELATED, MERGE),
    CANDIDATE_TREE,
    "side-parent-target",
)
remote.add_commit(unrelated)
remote.add_commit(side_parent)
current = staged(remote, base, candidate, merge)
remote.set_target(side_parent.oid)
expect_error("merge reachable only through side parent", current.finalize)
assert current.state == "rejected"
assert remote.target_oid == SIDE_PARENT
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
remote.queue_merge(merge)
current = transaction(remote, base, candidate)
current.precheck(base.oid)
current.merge()
current.verify_merge()
other_merge = commit(
    OTHER_MERGE,
    (BASE, CANDIDATE),
    CANDIDATE_TREE,
    "different-raw-merge",
)
expect_error(
    "initial artifact mismatch",
    lambda: current.store_artifact(SignedMergeArtifact.from_commit(other_merge)),
)
assert current.state == "reverted"
assert remote.target_oid == BASE
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
remote.set_target(candidate.oid)
current = transaction(remote, base, candidate)
expect_error("target movement before precheck", lambda: current.precheck(base.oid))
assert current.state == "rejected"
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
current = transaction(remote, base, candidate)
expect_error("local target mismatch", lambda: current.precheck(candidate.oid))
assert current.state == "rejected"
assert remote.target_oid == BASE
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
current = transaction(remote, base, candidate)
current.precheck(base.oid)
remote.set_target(candidate.oid)
remote.queue_merge(merge)
expect_error("precheck-to-merge target race", current.merge)
assert current.state == "rejected"
assert remote.target_oid == CANDIDATE
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
wrong = commit(WRONG_PARENT, (CANDIDATE, BASE), CANDIDATE_TREE, "wrong-parent")
remote.queue_merge(wrong)
current = transaction(remote, base, candidate)
current.precheck(base.oid)
current.merge()
expect_error("wrong merge parents", current.verify_merge)
assert current.state == "reverted"
assert remote.target_oid == BASE
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
wrong = commit(WRONG_TREE, (BASE, CANDIDATE), WRONG_TREE_OID, "wrong-tree")
remote.queue_merge(wrong)
current = transaction(remote, base, candidate)
current.precheck(base.oid)
current.merge()
expect_error("wrong merge tree", current.verify_merge)
assert current.state == "reverted"
assert remote.target_oid == BASE
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
wrong = commit(WRONG_PARENT, (CANDIDATE, BASE), CANDIDATE_TREE, "revert-failure")
remote.queue_merge(wrong)
current = transaction(remote, base, candidate)
current.precheck(base.oid)
current.merge()
remote.set_revert_failure(True)
expect_error("failed merge revert", current.verify_merge)
assert current.state == "blocked"
assert remote.blocked
assert remote.target_oid == WRONG_PARENT
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
remote.queue_merge(merge, response_oid=None)
current = transaction(remote, base, candidate)
current.precheck(base.oid)
expect_error("unavailable merge response", current.merge)
assert current.state == "blocked"
assert remote.target_oid == MERGE
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
remote.queue_merge(merge, response_oid=UNRELATED)
current = transaction(remote, base, candidate)
current.precheck(base.oid)
expect_error("unknown merge response", current.merge)
assert current.state == "blocked"
assert remote.target_oid == MERGE
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
unrelated = commit(
    UNRELATED,
    (BASE, CANDIDATE),
    CANDIDATE_TREE,
    "unrelated-response",
)
remote.add_commit(unrelated)
remote.queue_merge(merge, response_oid=unrelated.oid)
current = transaction(remote, base, candidate)
current.precheck(base.oid)
expect_error("wrong merge response identity", current.merge)
assert current.state == "blocked"
assert remote.target_oid == MERGE
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
current = staged(remote, base, candidate, merge)
current.tamper_artifact(signature_sha256="0" * 64)
expect_error("signed artifact mutation", current.finalize)
assert current.state == "reverted"
assert remote.target_oid == BASE
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
current = staged(remote, base, candidate, merge)
current.tamper_artifact(tree_oid=WRONG_TREE_OID)
expect_error("signed artifact payload mutation", current.finalize)
assert current.state == "reverted"
assert remote.target_oid == BASE
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
current = staged(remote, base, candidate, merge)
mutated = commit(MERGE, (BASE, CANDIDATE), CANDIDATE_TREE, "raw-object-mutation")
remote.replace_commit(mutated)
expect_error("final raw merge mutation", current.finalize)
assert current.state == "reverted"
assert remote.target_oid == BASE
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
current = staged(remote, base, candidate, merge)
unrelated = commit(UNRELATED, (), BASE_TREE, "force-push")
remote.add_commit(unrelated)
remote.set_target(unrelated.oid)
expect_error("force-push before final refetch", current.finalize)
assert current.state == "rejected"
assert remote.target_oid == UNRELATED
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
current = staged(remote, base, candidate, merge)
remote.set_fetch_failure(MERGE)
expect_error("unavailable final refetch", current.finalize)
assert current.state == "rejected"
assert remote.target_oid == MERGE
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
current = staged(remote, base, candidate, merge)
remote.set_target(None)
expect_error("target removal before final refetch", current.finalize)
assert current.state == "rejected"
assert remote.target_oid is None
assert_unadvanced(current)

base, candidate, merge, remote = fixture()
current = transaction(remote, base, candidate)
current.precheck(base.oid)
remote.queue_merge(merge)
current.merge()
current.verify_merge()
expect_error(
    "duplicate artifact parent",
    lambda: SignedMergeArtifact(
        merge_oid=MERGE,
        merge_sha256=merge.raw_sha256,
        parents=(BASE, BASE),
        tree_oid=CANDIDATE_TREE,
        signature_sha256="0" * 64,
    ),
)
assert current.state == "verified"

print("merge-race evidence checks passed")
PY
