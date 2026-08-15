#!/usr/bin/env bash
# Deterministic owner for raw Git revision/tree binding.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/align-git-revision.XXXXXX")"
trap 'rm -rf -- "$TEST_ROOT"' EXIT
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" "$TEST_ROOT" <<'PY'
from __future__ import annotations

import hashlib
import io
import os
import subprocess
import sys
from pathlib import Path

from scripts.benchmark_evidence import git_objects as go
from scripts.benchmark_evidence import git_revision as gr


ROOT = Path(sys.argv[2])
REPO = ROOT / "repo"
HOME = ROOT / "home"
REPO.mkdir()
HOME.mkdir()


def git(*args: str) -> str:
    environment = {
        "HOME": str(HOME),
        "LC_ALL": "C",
        "PATH": "/usr/bin:/bin",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_ATTR_NOSYSTEM": "1",
        "GIT_OPTIONAL_LOCKS": "0",
    }
    result = subprocess.run(
        ["/usr/bin/git", "-C", str(REPO), *args],
        check=True,
        env=environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return result.stdout.strip()


def expect_error(call, fragment: str) -> None:
    try:
        call()
    except gr.GitRevisionError as exc:
        assert fragment in str(exc), str(exc)
    else:
        raise AssertionError("accepted invalid Git revision input")


def object_record(kind: str, payload: bytes) -> tuple[str, go.VerifiedObject]:
    raw = go.encode(kind, payload)
    oid = hashlib.sha1(raw).hexdigest()
    return oid, go.verify(oid, raw)


class ObjectMap:
    def __init__(self, objects: dict[str, go.VerifiedObject]):
        self.objects = objects

    def read(self, oid: str) -> go.VerifiedObject:
        try:
            return self.objects[oid]
        except KeyError as exc:
            raise gr.GitRevisionError(f"missing object {oid}") from exc


def fake_commit(tree_oid: str, parents: tuple[str, ...] = ()) -> tuple[str, go.VerifiedObject]:
    payload = (
        b"tree "
        + tree_oid.encode("ascii")
        + b"\n"
        + b"".join(b"parent " + parent.encode("ascii") + b"\n" for parent in parents)
        + b"author Fixture <fixture@example.com> 0 +0000\n"
        + b"committer Fixture <fixture@example.com> 0 +0000\n\nfixture\n"
    )
    return object_record("commit", payload)


def fake_tree(entries: list[tuple[str, bytes, str]]) -> tuple[str, go.VerifiedObject]:
    payload = b"".join(
        mode.encode("ascii") + b" " + name + b"\0" + bytes.fromhex(oid)
        for mode, name, oid in entries
    )
    return object_record("tree", payload)


def test_pure_rejections() -> None:
    blob_oid, blob = object_record("blob", b"x\n")

    class FakeProcess:
        def __init__(self, response: bytes):
            self.stdin = io.BytesIO()
            self.stdout = io.BytesIO(response)

    response = blob_oid.encode("ascii") + b" blob 2\nx\n\n"
    assert gr.GitBatchObjectReader(FakeProcess(response), 2).read(blob_oid).payload == b"x\n"
    expect_error(
        lambda: gr.GitBatchObjectReader(
            FakeProcess(blob_oid.encode("ascii") + b" blob 3\n"),
            2,
        ).read(blob_oid),
        "fixed size bound",
    )
    empty_tree_oid, empty_tree = fake_tree([])
    base_oid, base = fake_commit(empty_tree_oid)
    child_oid, child = fake_commit(empty_tree_oid, (base_oid,))
    merge_oid, merge = fake_commit(empty_tree_oid, (child_oid, base_oid))
    objects = ObjectMap({
        blob_oid: blob,
        empty_tree_oid: empty_tree,
        base_oid: base,
        child_oid: child,
        merge_oid: merge,
    })
    binding = gr.bind_revisions(objects, base_oid, child_oid, base_oid)
    assert binding.baseline.commit_oid == base_oid
    assert binding.candidate.commit_oid == child_oid
    assert binding.candidate.commits[0].oid == child_oid
    assert binding.candidate.changed_paths == ()
    expect_error(lambda: gr.bind_revisions(objects, base_oid, child_oid, child_oid), "local target")
    expect_error(lambda: gr.bind_revisions(objects, base_oid, merge_oid, base_oid), "merge")
    expect_error(
        lambda: gr.bind_revisions(
            objects,
            base_oid,
            child_oid,
            base_oid,
            review_head=child_oid,
            review_base=base_oid,
            review_state="fixed",
        ),
        "repair commits",
    )
    expect_error(
        lambda: gr.bind_revisions(
            objects,
            base_oid,
            child_oid,
            base_oid,
            review_head=child_oid,
            review_base=base_oid,
            review_state="clean",
            repair_commits=(child_oid,),
        ),
        "clean review",
    )

    invalid_cases = [
        ("unsupported mode", [
            ("160000", b"submodule", blob_oid),
        ], "unsupported mode"),
        ("invalid UTF-8", [
            ("100644", b"\xff", blob_oid),
        ], "valid UTF-8"),
        ("unsorted", [
            ("100644", b"b", blob_oid),
            ("100644", b"a", blob_oid),
        ], "canonical Git order"),
        ("case collision", [
            ("100644", b"A", blob_oid),
            ("100644", b"a", blob_oid),
        ], "path collision"),
    ]
    for _name, entries, fragment in invalid_cases:
        tree_oid, tree = fake_tree(entries)
        commit_oid, commit = fake_commit(tree_oid)
        source = ObjectMap({blob_oid: blob, tree_oid: tree, commit_oid: commit})
        expect_error(lambda: gr.snapshot_from_reader(source, commit_oid), fragment)

    missing_tree_commit_oid, missing_tree_commit = fake_commit("0" * 40)
    expect_error(
        lambda: gr.snapshot_from_reader(
            ObjectMap({missing_tree_commit_oid: missing_tree_commit}),
            missing_tree_commit_oid,
        ),
        "missing object",
    )


def test_real_repository() -> None:
    git("init", "--initial-branch=main", "-q")
    git("config", "user.name", "Revision Fixture")
    git("config", "user.email", "revision@example.com")
    (REPO / "old.txt").write_bytes(b"old\n")
    (REPO / "deleted.txt").write_bytes(b"deleted\n")
    git("add", "old.txt", "deleted.txt")
    git("commit", "-m", "base", "-q")
    base = git("rev-parse", "HEAD")

    (REPO / "old.txt").write_bytes(b"new\n")
    os.chmod(REPO / "old.txt", 0o755)
    (REPO / "deleted.txt").unlink()
    (REPO / "added.txt").write_bytes(b"added\n")
    os.symlink("old.txt", REPO / "link.txt")
    git("add", "-A")
    git("commit", "-m", "candidate", "-q")
    candidate = git("rev-parse", "HEAD")
    git("update-ref", "refs/heads/main", base)

    with gr.GitRevisionReader(str(REPO), str(HOME)) as reader:
        assert reader.target_oid() == base
        binding = reader.bind(
            base,
            candidate,
            reader.target_oid(),
            review_head=candidate,
            review_base=base,
            review_state="clean",
        )
        assert binding.baseline.parents == ()
        assert binding.candidate.parents == (base,)
        assert [commit.oid for commit in binding.candidate.commits] == [candidate]
        assert [change.path_hex for change in binding.candidate.changed_paths] == sorted(
            change.path_hex for change in binding.candidate.changed_paths
        )
        changes = {change.path.decode(): change for change in binding.candidate.changed_paths}
        assert changes["old.txt"].status == "modified"
        assert changes["deleted.txt"].status == "deleted"
        assert changes["added.txt"].status == "added"
        assert changes["link.txt"].new.kind == "symlink"
        assert changes["old.txt"].old.mode == "100644"
        assert changes["old.txt"].new.mode == "100755"
        assert binding.baseline.as_dict()["commits"] == []
        assert binding.baseline.as_dict()["changed_paths"] == []

        git("update-ref", "refs/heads/main", candidate)
        assert reader.target_oid() == candidate
        expect_error(
            lambda: reader.bind(base, candidate, reader.target_oid()),
            "local target",
        )
        git("update-ref", "refs/heads/main", candidate)
        (REPO / "second.txt").write_bytes(b"second\n")
        git("add", "second.txt")
        git("commit", "-m", "repair", "-q")
        repaired_candidate = git("rev-parse", "HEAD")
        git("update-ref", "refs/heads/main", base)
        repaired = reader.bind(
            base,
            repaired_candidate,
            reader.target_oid(),
            review_head=candidate,
            review_base=base,
            review_state="fixed",
            repair_commits=(repaired_candidate,),
        )
        assert [commit.oid for commit in repaired.candidate.commits] == [candidate, repaired_candidate]

        expect_error(
            lambda: reader.bind(
                base,
                repaired_candidate,
                base,
                review_head=base,
                review_base=base,
                review_state="fixed",
                repair_commits=(),
            ),
            "repair commits",
        )


test_pure_rejections()
test_real_repository()
print("benchmark evidence Git revision checks passed")
PY
