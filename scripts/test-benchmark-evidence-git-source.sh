#!/usr/bin/env bash
# Deterministic owner for raw Git source construction and retained-FD verification.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/align-git-source.XXXXXX")"
trap 'rm -rf -- "$TEST_ROOT"' EXIT
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$TEST_ROOT" <<'PY'
from __future__ import annotations

import hashlib
import os
import sys
from pathlib import Path

from scripts.benchmark_evidence import git_objects as go
from scripts.benchmark_evidence import git_revision as gr
from scripts.benchmark_evidence import git_source as gs


ROOT = Path(sys.argv[1])


def object_record(payload: bytes) -> tuple[str, go.VerifiedObject]:
    raw = go.encode("blob", payload)
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


def expect_error(call, fragment: str) -> None:
    try:
        call()
    except gs.SourceError as exc:
        assert fragment in str(exc), str(exc)
    else:
        raise AssertionError(f"accepted invalid source: {fragment}")


def make_identity(
    objects: dict[str, go.VerifiedObject],
    path: bytes,
    mode: str,
    kind: str,
    payload: bytes,
) -> gr.PathIdentity:
    oid, value = object_record(payload)
    objects[oid] = value
    return gr.PathIdentity(
        path=path,
        mode=mode,
        kind=kind,
        oid=oid,
        size=len(payload),
        sha256=hashlib.sha256(payload).hexdigest(),
    )


def snapshot(paths: tuple[gr.PathIdentity, ...]) -> gr.RevisionSnapshot:
    paths = tuple(sorted(paths, key=lambda item: item.path_hex))
    return gr.RevisionSnapshot(
        commit_oid="a" * 40,
        commit_sha256="b" * 64,
        tree_oid="c" * 40,
        tree_manifest_sha256=gr.tree_manifest_sha256(paths),
        parents=(),
        paths=paths,
    )


def build_fixture() -> tuple[ObjectMap, gr.RevisionSnapshot]:
    objects: dict[str, go.VerifiedObject] = {}
    paths = (
        make_identity(objects, b"src/main.align", "100644", "blob", b"main\n"),
        make_identity(objects, b"bin/tool", "100755", "blob", b"#!/bin/tool\n"),
        make_identity(objects, b"bin/tool-copy", "100755", "blob", b"#!/bin/tool-copy\n"),
        make_identity(objects, b"link", "120000", "symlink", b"src/main.align"),
    )
    return ObjectMap(objects), snapshot(paths)


def test_materialize_and_retain_root() -> None:
    reader, expected = build_fixture()
    destination = ROOT / "source"
    with gs.materialize_source(
        reader,
        expected,
        str(destination),
        reviewed_symlinks=expected.paths,
    ) as source:
        assert destination.joinpath("src/main.align").read_bytes() == b"main\n"
        assert destination.joinpath("bin/tool").read_bytes() == b"#!/bin/tool\n"
        assert os.stat(destination / "bin/tool").st_mode & 0o777 == 0o755
        assert os.readlink(destination / "link") == "src/main.align"
        assert gs.verify_source(
            reader,
            expected,
            source,
            reviewed_symlinks=expected.paths,
        ) == expected.tree_manifest_sha256

        moved = ROOT / "moved-source"
        os.rename(destination, moved)
        destination.mkdir()
        (destination / "replacement-marker").write_text("must not be observed", encoding="utf-8")
        assert gs.verify_source(
            reader,
            expected,
            source,
            reviewed_symlinks=expected.paths,
        ) == expected.tree_manifest_sha256
        (destination / "replacement-marker").unlink()
        os.rmdir(destination)
        os.rename(moved, destination)
    assert not destination.exists()


def test_mutations_reject() -> None:
    reader, expected = build_fixture()

    def check(name: str, mutate, fragment: str) -> None:
        destination = ROOT / name
        with gs.materialize_source(
            reader,
            expected,
            str(destination),
            reviewed_symlinks=expected.paths,
        ) as source:
            mutate(destination)
            expect_error(
                lambda: gs.verify_source(
                    reader,
                    expected,
                    source,
                    reviewed_symlinks=expected.paths,
                ),
                fragment,
            )

    check(
        "extra",
        lambda path: (path / "extra").write_bytes(b"unexpected"),
        "missing or extra",
    )
    check(
        "missing",
        lambda path: (path / "src/main.align").unlink(),
        "missing or extra",
    )
    check(
        "mode",
        lambda path: os.chmod(path / "bin/tool", 0o644),
        "type or mode",
    )

    def replace_with_symlink(path: Path) -> None:
        (path / "src/main.align").unlink()
        os.symlink("/outside", path / "src/main.align")

    check("type", replace_with_symlink, "type or mode")

    def hard_link(path: Path) -> None:
        (path / "bin/tool-copy").unlink()
        os.link(path / "bin/tool", path / "bin/tool-copy")

    check("hard-link", hard_link, "hard-linked")


def test_symlink_and_inventory_rejections() -> None:
    reader, baseline = build_fixture()
    objects = dict(reader.objects)
    changed_link = make_identity(objects, b"link", "120000", "symlink", b"src/other.align")
    other = make_identity(objects, b"src/other.align", "100644", "blob", b"other\n")
    changed = snapshot((baseline.paths[0], baseline.paths[1], changed_link, other))
    expect_error(
        lambda: gs.materialize_source(
            ObjectMap(objects),
            changed,
            str(ROOT / "changed-link"),
            reviewed_symlinks=baseline.paths,
        ),
        "new or changed",
    )

    unsafe_objects: dict[str, go.VerifiedObject] = {}
    unsafe_link = make_identity(unsafe_objects, b"link", "120000", "symlink", b"../outside")
    unsafe = snapshot((unsafe_link,))
    expect_error(
        lambda: gs.materialize_source(
            ObjectMap(unsafe_objects),
            unsafe,
            str(ROOT / "unsafe-link"),
            reviewed_symlinks=unsafe.paths,
        ),
        "forbidden",
    )

    collision_objects: dict[str, go.VerifiedObject] = {}
    upper = make_identity(collision_objects, b"A", "100644", "blob", b"a")
    lower = make_identity(collision_objects, b"a", "100644", "blob", b"a")
    collision = snapshot((upper, lower))
    expect_error(
        lambda: gs.materialize_source(
            ObjectMap(collision_objects),
            collision,
            str(ROOT / "collision"),
        ),
        "collision",
    )

    existing = ROOT / "existing"
    existing.mkdir()
    expect_error(
        lambda: gs.materialize_source(
            reader,
            baseline,
            str(existing),
            reviewed_symlinks=baseline.paths,
        ),
        "new source root",
    )


def test_snapshot_and_cleanup_fail_closed() -> None:
    reader, expected = build_fixture()
    bad_snapshot = gr.RevisionSnapshot(
        commit_oid=expected.commit_oid,
        commit_sha256=expected.commit_sha256,
        tree_oid=expected.tree_oid,
        tree_manifest_sha256="d" * 64,
        parents=expected.parents,
        paths=expected.paths,
    )
    expect_error(
        lambda: gs.materialize_source(
            reader,
            bad_snapshot,
            str(ROOT / "bad-snapshot"),
            reviewed_symlinks=expected.paths,
        ),
        "manifest identity",
    )
    assert not (ROOT / "bad-snapshot").exists()

    destination = ROOT / "root-replacement"
    source = gs.materialize_source(
        reader,
        expected,
        str(destination),
        reviewed_symlinks=expected.paths,
    )
    moved = ROOT / "root-replacement-moved"
    os.rename(destination, moved)
    destination.mkdir()
    (destination / "replacement-marker").write_text("must survive", encoding="utf-8")
    expect_error(source.remove, "replaced")
    assert (destination / "replacement-marker").read_text(encoding="utf-8") == "must survive"
    (destination / "replacement-marker").unlink()
    os.rmdir(destination)
    os.rename(moved, destination)
    # The failed removal closed its descriptors and deliberately left the
    # original tree for administrator recovery instead of deleting a swap.
    assert destination.joinpath("src/main.align").read_bytes() == b"main\n"

    explicit = gs.materialize_source(
        reader,
        expected,
        str(ROOT / "explicit-close"),
        reviewed_symlinks=expected.paths,
    )
    explicit.close()
    explicit.remove()
    assert not (ROOT / "explicit-close").exists()

    failing_identity = next(identity for identity in expected.paths if identity.path == b"bin/tool-copy")

    class FailingReader:
        def read(self, oid: str) -> go.VerifiedObject:
            if oid == failing_identity.oid:
                raise gr.GitRevisionError("fixture object failure")
            return reader.read(oid)

    expect_error(
        lambda: gs.materialize_source(
            FailingReader(),
            expected,
            str(ROOT / "mid-construction-failure"),
            reviewed_symlinks=expected.paths,
        ),
        "cannot read source blob",
    )
    assert not (ROOT / "mid-construction-failure").exists()

    original_open = gs.os.open

    def fail_new_root_open(path, flags, *args, **kwargs):
        if path == "partial-root" and flags == gs._DIRECTORY_FLAGS:
            raise OSError("fixture root open failure")
        return original_open(path, flags, *args, **kwargs)

    gs.os.open = fail_new_root_open
    try:
        expect_error(
            lambda: gs.materialize_source(
                reader,
                expected,
                str(ROOT / "partial-root"),
                reviewed_symlinks=expected.paths,
            ),
            "cannot create the new source root",
        )
    finally:
        gs.os.open = original_open
    assert not (ROOT / "partial-root").exists()


test_materialize_and_retain_root()
test_mutations_reject()
test_symlink_and_inventory_rejections()
test_snapshot_and_cleanup_fail_closed()
print("benchmark evidence Git source checks passed")
PY
