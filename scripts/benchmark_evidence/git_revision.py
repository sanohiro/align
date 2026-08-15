"""Read and bind the raw Git revision graph used by benchmark evidence.

The reader is deliberately narrower than a checkout.  It consumes one fixed
``git cat-file --batch`` process, verifies every returned raw object, parses
commit and tree records, and constructs an immutable path inventory.  It never
uses an ambient worktree, index, archive filter, hook, or candidate code.
"""

from __future__ import annotations

import hashlib
import unicodedata
from dataclasses import dataclass
from typing import BinaryIO, Protocol, Sequence

from . import canonical_json
from . import git_batch
from . import git_objects
from . import git_process


class GitRevisionError(ValueError):
    """A raw Git object or revision binding violates the evidence contract."""


MAX_BATCH_HEADER_BYTES = 512
MAX_OBJECT_BYTES = 64 * 1024 * 1024
MAX_TREE_DEPTH = 1024
MAX_TREE_ENTRIES = 1_000_000
TARGET_REF = "refs/heads/main"


class ObjectReader(Protocol):
    """The minimal verified-object source needed by the pure revision parser."""

    def read(self, oid: str) -> git_objects.VerifiedObject:
        ...


@dataclass(frozen=True)
class CommitRecord:
    """The identity-bearing fields extracted from one verified commit."""

    oid: str
    raw_sha256: str
    tree_oid: str
    parents: tuple[str, ...]

    def as_dict(self) -> dict[str, object]:
        return {
            "oid": self.oid,
            "raw_sha256": self.raw_sha256,
            "tree_oid": self.tree_oid,
            "parents": list(self.parents),
        }


@dataclass(frozen=True)
class PathIdentity:
    """One leaf in a raw Git tree closure."""

    path: bytes
    mode: str
    kind: str
    oid: str
    size: int
    sha256: str

    @property
    def path_hex(self) -> str:
        return self.path.hex()

    def as_dict(self) -> dict[str, object]:
        return {
            "path_hex": self.path_hex,
            "mode": self.mode,
            "kind": self.kind,
            "oid": self.oid,
            "size": self.size,
            "sha256": self.sha256,
        }


@dataclass(frozen=True)
class PathSide:
    """A present or absent side of a two-tree path change."""

    presence: str
    mode: str = ""
    kind: str = ""
    oid: str = ""
    size: int = 0
    sha256: str = ""

    @classmethod
    def absent(cls) -> "PathSide":
        return cls("absent")

    @classmethod
    def present(cls, identity: PathIdentity) -> "PathSide":
        return cls(
            "present",
            mode=identity.mode,
            kind=identity.kind,
            oid=identity.oid,
            size=identity.size,
            sha256=identity.sha256,
        )

    def as_dict(self) -> dict[str, object]:
        return {
            "presence": self.presence,
            "mode": self.mode,
            "kind": self.kind,
            "oid": self.oid,
            "size": self.size,
            "sha256": self.sha256,
        }


@dataclass(frozen=True)
class PathChange:
    """One exact added/deleted/modified member of a tree union diff."""

    path: bytes
    status: str
    old: PathSide
    new: PathSide

    @property
    def path_hex(self) -> str:
        return self.path.hex()

    def as_dict(self) -> dict[str, object]:
        return {
            "path_hex": self.path_hex,
            "status": self.status,
            "old": self.old.as_dict(),
            "new": self.new.as_dict(),
        }


@dataclass(frozen=True)
class RevisionSnapshot:
    """A commit, its complete leaf tree inventory, and its manifest identity."""

    commit_oid: str
    commit_sha256: str
    tree_oid: str
    tree_manifest_sha256: str
    parents: tuple[str, ...]
    paths: tuple[PathIdentity, ...]
    commits: tuple[CommitRecord, ...] = ()
    changed_paths: tuple[PathChange, ...] = ()

    def as_dict(self) -> dict[str, object]:
        return {
            "commit_oid": self.commit_oid,
            "commit_sha256": self.commit_sha256,
            "tree_oid": self.tree_oid,
            "tree_manifest_sha256": self.tree_manifest_sha256,
            "parents": list(self.parents),
            "commits": [commit.as_dict() for commit in self.commits],
            "changed_paths": [change.as_dict() for change in self.changed_paths],
        }


@dataclass(frozen=True)
class RevisionBinding:
    """The baseline/candidate pair bound to one observed local target."""

    baseline: RevisionSnapshot
    candidate: RevisionSnapshot
    target_oid: str


def _error(message: str) -> None:
    raise GitRevisionError(message)


def _oid(value: object, label: str) -> str:
    try:
        return git_objects.validate_oid(value)
    except git_objects.GitObjectError as exc:
        raise GitRevisionError(f"{label}: {exc}") from exc


def _read_exact(stream: BinaryIO, size: int, label: str) -> bytes:
    if size < 0:
        _error(f"{label} size is negative")
    result = bytearray()
    while len(result) < size:
        try:
            chunk = stream.read(size - len(result))
        except OSError as exc:
            raise GitRevisionError(f"cannot read {label}") from exc
        if not chunk:
            _error(f"Git ended before the complete {label} was read")
        result.extend(chunk)
    return bytes(result)


def _read_line(stream: BinaryIO, label: str) -> bytes:
    result = bytearray()
    while len(result) < MAX_BATCH_HEADER_BYTES:
        chunk = _read_exact(stream, 1, label)
        result.extend(chunk)
        if chunk == b"\n":
            return bytes(result)
    _error(f"{label} exceeds the fixed header bound")


def _write_all(stream: BinaryIO, raw: bytes) -> None:
    view = memoryview(raw)
    while view:
        try:
            count = stream.write(view)
        except OSError as exc:
            raise GitRevisionError("cannot write the Git batch request") from exc
        if count is None or count <= 0:
            _error("Git batch request made no write progress")
        view = view[count:]
    try:
        stream.flush()
    except OSError as exc:
        raise GitRevisionError("cannot flush the Git batch request") from exc


class GitBatchObjectReader:
    """Bounded streaming adapter for one already-started pinned Git process."""

    def __init__(self, process: git_process.PinnedGitProcess, max_object_bytes: int = MAX_OBJECT_BYTES):
        if not isinstance(max_object_bytes, int) or max_object_bytes <= 0:
            raise GitRevisionError("Git object bound must be a positive integer")
        self._process = process
        self._max_object_bytes = max_object_bytes
        self._failed = False

    def _response(self, requested: str | None) -> git_objects.VerifiedObject | None:
        if self._failed:
            _error("Git batch reader is unusable after a protocol error")
        try:
            return self._read_response(requested)
        except GitRevisionError:
            self._failed = True
            try:
                self._process.close()
            except git_process.GitProcessError:
                pass
            raise

    def _read_response(self, requested: str | None) -> git_objects.VerifiedObject | None:
        header = _read_line(self._process.stdout, "Git batch header")
        if not header.endswith(b"\n"):
            _error("Git batch header has no LF")
        header_without_lf = header[:-1]
        fields = header_without_lf.split(b" ")
        if len(fields) == 2 and fields[1] == b"missing":
            if requested is None or fields[0] != requested.encode("ascii"):
                _error("Git batch missing-object response does not match the request")
            result = git_batch.parse(requested, header)
            if result.object is None:
                return None
            return result.object
        if len(fields) != 3:
            _error("Git batch header has the wrong shape")
        response_oid, kind_bytes, size_text = fields
        try:
            response_oid_text = response_oid.decode("ascii")
            kind = kind_bytes.decode("ascii")
            size_text_decoded = size_text.decode("ascii")
        except UnicodeDecodeError as exc:
            raise GitRevisionError("Git batch header is not ASCII") from exc
        response_oid_text = _oid(response_oid_text, "Git batch object ID")
        if requested is not None and response_oid_text != requested:
            _error("Git batch object ID does not match the request")
        if (
            not size_text_decoded
            or (len(size_text_decoded) > 1 and size_text_decoded[0] == "0")
            or not size_text_decoded.isdecimal()
        ):
            _error("Git batch size is not canonical decimal")
        size = int(size_text_decoded)
        if size > self._max_object_bytes:
            _error("Git object exceeds the fixed size bound")
        payload_with_separator = _read_exact(self._process.stdout, size + 1, "Git batch payload")
        response = header + payload_with_separator
        if requested is not None:
            try:
                result = git_batch.parse(requested, response)
            except git_batch.GitBatchError as exc:
                raise GitRevisionError(str(exc)) from exc
            if result.object is None:
                return None
            return result.object
        try:
            if not payload_with_separator.endswith(b"\n"):
                _error("Git batch response is missing its payload separator LF")
            raw = git_objects.encode(kind, payload_with_separator[:-1])
            return git_objects.verify(response_oid_text, raw)
        except git_objects.GitObjectError as exc:
            raise GitRevisionError(str(exc)) from exc

    def read(self, oid: str) -> git_objects.VerifiedObject:
        requested = _oid(oid, "object ID")
        _write_all(self._process.stdin, requested.encode("ascii") + b"\n")
        result = self._response(requested)
        if result is None:
            _error(f"Git object {requested} is missing")
        return result

    def read_target(self) -> git_objects.VerifiedObject:
        """Resolve the fixed local target ref through the same batch process."""

        _write_all(self._process.stdin, TARGET_REF.encode("ascii") + b"\n")
        result = self._response(None)
        if result is None:
            _error(f"Git target ref {TARGET_REF} is missing")
        if result.kind != "commit":
            _error(f"Git target ref {TARGET_REF} does not name a commit")
        return result


class GitRevisionReader:
    """Own a pinned batch process and expose revision snapshots."""

    def __init__(self, repository: object, home: object, max_object_bytes: int = MAX_OBJECT_BYTES):
        if not isinstance(max_object_bytes, int) or max_object_bytes <= 0:
            raise GitRevisionError("Git object bound must be a positive integer")
        self._process = git_process.PinnedGitProcess(repository, home)
        self._max_object_bytes = max_object_bytes
        self._objects: GitBatchObjectReader | None = None

    def start(self) -> "GitRevisionReader":
        if self._objects is not None:
            raise GitRevisionError("Git revision reader has already started")
        objects = GitBatchObjectReader(self._process, self._max_object_bytes)
        try:
            self._process.start()
        except BaseException:
            self._objects = None
            raise
        self._objects = objects
        return self

    def close(self) -> None:
        self._process.close()
        self._objects = None

    def read(self, oid: str) -> git_objects.VerifiedObject:
        if self._objects is None:
            raise GitRevisionError("Git revision reader has not started")
        return self._objects.read(oid)

    def target_oid(self) -> str:
        if self._objects is None:
            raise GitRevisionError("Git revision reader has not started")
        return self._objects.read_target().oid

    def snapshot(self, commit_oid: str) -> RevisionSnapshot:
        return snapshot_from_reader(self, commit_oid)

    def bind(
        self,
        baseline_oid: str,
        candidate_oid: str,
        target_oid: str,
        *,
        review_head: str | None = None,
        review_base: str | None = None,
        review_state: str | None = None,
        repair_commits: Sequence[str] = (),
    ) -> RevisionBinding:
        return bind_revisions(
            self,
            baseline_oid,
            candidate_oid,
            target_oid,
            review_head=review_head,
            review_base=review_base,
            review_state=review_state,
            repair_commits=repair_commits,
        )

    def __enter__(self) -> "GitRevisionReader":
        return self.start()

    def __exit__(self, exc_type, exc_value, traceback) -> None:
        self.close()


def parse_commit(value: git_objects.VerifiedObject) -> CommitRecord:
    """Parse the tree and parent headers from one verified commit object."""

    if value.kind != "commit":
        _error("revision object is not a commit")
    separator = value.payload.find(b"\n\n")
    if separator < 0:
        _error("commit object has no header/message separator")
    headers = value.payload[:separator].split(b"\n")
    tree_oid: str | None = None
    parents: list[str] = []
    previous_header = False
    for line in headers:
        if not line:
            _error("commit header contains an empty line")
        if line.startswith(b" "):
            if not previous_header:
                _error("commit continuation has no preceding header")
            continue
        if b" " not in line:
            _error("commit header has no field separator")
        key, raw_value = line.split(b" ", 1)
        if not key or any(byte < 0x21 or byte > 0x7E for byte in key):
            _error("commit header name is not printable ASCII")
        if key == b"tree":
            if tree_oid is not None:
                _error("commit has duplicate tree headers")
            try:
                tree_text = raw_value.decode("ascii")
            except UnicodeDecodeError as exc:
                raise GitRevisionError("commit tree header is not ASCII") from exc
            tree_oid = _oid(tree_text, "commit tree")
        elif key == b"parent":
            try:
                parent_text = raw_value.decode("ascii")
            except UnicodeDecodeError as exc:
                raise GitRevisionError("commit parent header is not ASCII") from exc
            parents.append(_oid(parent_text, "commit parent"))
        previous_header = True
    if tree_oid is None:
        _error("commit has no tree header")
    return CommitRecord(value.oid, value.raw_sha256, tree_oid, tuple(parents))


def _tree_name(name: bytes) -> str:
    if not name or b"/" in name or b"\x00" in name:
        _error("tree entry name is empty or contains a separator")
    try:
        text = name.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise GitRevisionError("tree entry path is not valid UTF-8") from exc
    if text in (".", "..") or text.casefold() == ".git":
        _error("tree entry path contains a forbidden component")
    return text


def _tree_entries(payload: bytes) -> list[tuple[str, bytes, str, str]]:
    """Return ``(mode, name, kind, oid)`` entries from one raw tree payload."""

    entries: list[tuple[str, bytes, str, str]] = []
    offset = 0
    previous_sort_key: bytes | None = None
    while offset < len(payload):
        mode_end = payload.find(b" ", offset)
        if mode_end < 0:
            _error("tree entry has no mode separator")
        name_end = payload.find(b"\0", mode_end + 1)
        if name_end < 0:
            _error("tree entry has no name terminator")
        mode_bytes = payload[offset:mode_end]
        name = payload[mode_end + 1 : name_end]
        try:
            mode = mode_bytes.decode("ascii")
        except UnicodeDecodeError as exc:
            raise GitRevisionError("tree entry mode is not ASCII") from exc
        _tree_name(name)
        if mode not in ("40000", "100644", "100755", "120000"):
            _error("tree contains an unsupported mode or submodule")
        oid_start = name_end + 1
        oid_end = oid_start + 20
        if oid_end > len(payload):
            _error("tree entry is missing its object ID")
        oid = payload[oid_start:oid_end].hex()
        sort_key = name + (b"/" if mode == "40000" else b"")
        if previous_sort_key is not None and sort_key <= previous_sort_key:
            _error("tree entries are not in canonical Git order")
        previous_sort_key = sort_key
        kind = "tree" if mode == "40000" else ("symlink" if mode == "120000" else "blob")
        entries.append((mode, name, kind, oid))
        offset = oid_end
    return entries


def _path_key(path: bytes) -> str:
    try:
        text = path.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise GitRevisionError("tree path is not valid UTF-8") from exc
    return unicodedata.normalize("NFC", text).casefold()


def _walk_tree(
    reader: ObjectReader,
    tree_oid: str,
    prefix: tuple[bytes, ...],
    active_trees: set[str],
    seen_paths: set[str],
    paths: list[PathIdentity],
    depth: int,
) -> None:
    if depth > MAX_TREE_DEPTH:
        _error("tree depth exceeds the fixed bound")
    tree_oid = _oid(tree_oid, "tree object")
    if tree_oid in active_trees:
        _error("tree closure contains a cycle")
    active_trees.add(tree_oid)
    try:
        tree = reader.read(tree_oid)
        if tree.kind != "tree":
            _error("commit tree ID does not name a tree")
        if tree.oid != tree_oid:
            _error("Git returned a tree object with the wrong ID")
        for mode, name, kind, oid in _tree_entries(tree.payload):
            path_parts = prefix + (name,)
            path = b"/".join(path_parts)
            key = _path_key(path)
            if key in seen_paths:
                _error("tree closure contains a normalized or case-fold path collision")
            seen_paths.add(key)
            if kind == "tree":
                _walk_tree(reader, oid, path_parts, active_trees, seen_paths, paths, depth + 1)
                continue
            blob = reader.read(oid)
            if blob.kind != "blob":
                _error("tree leaf does not name a blob")
            if blob.oid != oid:
                _error("Git returned a blob object with the wrong ID")
            paths.append(
                PathIdentity(
                    path=path,
                    mode=mode,
                    kind=kind,
                    oid=blob.oid,
                    size=len(blob.payload),
                    sha256=hashlib.sha256(blob.payload).hexdigest(),
                )
            )
            if len(paths) > MAX_TREE_ENTRIES:
                _error("tree closure exceeds the fixed entry bound")
    finally:
        active_trees.remove(tree_oid)


def tree_manifest_bytes(paths: Sequence[PathIdentity]) -> bytes:
    """Return the canonical bytes whose digest binds a complete tree inventory."""

    ordered = sorted(paths, key=lambda item: item.path_hex)
    if tuple(ordered) != tuple(paths):
        _error("tree paths are not in path-hex order")
    values = [item.as_dict() for item in ordered]
    return canonical_json.encode(values)


def tree_manifest_sha256(paths: Sequence[PathIdentity]) -> str:
    return hashlib.sha256(tree_manifest_bytes(paths)).hexdigest()


def snapshot_from_reader(reader: ObjectReader, commit_oid: str) -> RevisionSnapshot:
    """Read one commit and its complete raw tree closure."""

    commit_oid = _oid(commit_oid, "commit object")
    commit_object = reader.read(commit_oid)
    if commit_object.oid != commit_oid:
        _error("Git returned a commit object with the wrong ID")
    commit = parse_commit(commit_object)
    paths: list[PathIdentity] = []
    _walk_tree(reader, commit.tree_oid, (), set(), set(), paths, 0)
    paths.sort(key=lambda item: item.path_hex)
    return RevisionSnapshot(
        commit_oid=commit.oid,
        commit_sha256=commit.raw_sha256,
        tree_oid=commit.tree_oid,
        tree_manifest_sha256=tree_manifest_sha256(paths),
        parents=commit.parents,
        paths=tuple(paths),
    )


def _changed_paths(old: Sequence[PathIdentity], new: Sequence[PathIdentity]) -> tuple[PathChange, ...]:
    old_by_path = {item.path: item for item in old}
    new_by_path = {item.path: item for item in new}
    if len(old_by_path) != len(old) or len(new_by_path) != len(new):
        _error("tree inventory contains a duplicate path")
    changes: list[PathChange] = []
    for path in sorted(set(old_by_path) | set(new_by_path), key=bytes.hex):
        old_item = old_by_path.get(path)
        new_item = new_by_path.get(path)
        if old_item == new_item:
            continue
        if old_item is None:
            changes.append(PathChange(path, "added", PathSide.absent(), PathSide.present(new_item)))
        elif new_item is None:
            changes.append(PathChange(path, "deleted", PathSide.present(old_item), PathSide.absent()))
        else:
            changes.append(PathChange(path, "modified", PathSide.present(old_item), PathSide.present(new_item)))
    return tuple(changes)


def _review_binding(
    baseline_oid: str,
    candidate_oid: str,
    chain: Sequence[CommitRecord],
    review_head: str | None,
    review_base: str | None,
    review_state: str | None,
    repair_commits: Sequence[str],
) -> None:
    if review_head is None and review_base is None and review_state is None and not repair_commits:
        return
    if review_head is None or review_base is None or review_state is None:
        _error("review binding is incomplete")
    review_head = _oid(review_head, "review head")
    review_base = _oid(review_base, "review base")
    repairs = tuple(_oid(item, "review repair commit") for item in repair_commits)
    if review_base != baseline_oid:
        _error("review base does not match the evidence baseline")
    chain_oids = tuple(item.oid for item in chain)
    if review_state == "clean":
        if review_head != candidate_oid or repairs:
            _error("clean review does not name the candidate exactly")
        return
    if review_state != "fixed":
        _error("review state is not clean or fixed")
    if review_head == baseline_oid:
        expected = chain_oids
    elif review_head in chain_oids:
        expected = chain_oids[chain_oids.index(review_head) + 1 :]
    else:
        _error("fixed review head is not on the candidate first-parent chain")
    if not expected or repairs != expected or repairs[-1] != candidate_oid:
        _error("fixed review repair commits are not the exact candidate suffix")


def bind_revisions(
    reader: ObjectReader,
    baseline_oid: str,
    candidate_oid: str,
    target_oid: str,
    *,
    review_head: str | None = None,
    review_base: str | None = None,
    review_state: str | None = None,
    repair_commits: Sequence[str] = (),
) -> RevisionBinding:
    """Bind a candidate's non-merge first-parent chain and two-tree diff."""

    baseline_oid = _oid(baseline_oid, "baseline")
    candidate_oid = _oid(candidate_oid, "candidate")
    target_oid = _oid(target_oid, "local target")
    if baseline_oid == candidate_oid:
        _error("baseline and candidate must differ")
    if target_oid != baseline_oid:
        _error("local target does not equal the evidence baseline")

    chain: list[CommitRecord] = []
    current = candidate_oid
    visited: set[str] = set()
    while current != baseline_oid:
        if current in visited:
            _error("candidate first-parent chain contains a cycle")
        visited.add(current)
        commit = parse_commit(reader.read(current))
        if len(commit.parents) != 1:
            _error("candidate history contains a merge or a root before the baseline")
        chain.append(commit)
        current = commit.parents[0]
        if len(chain) > MAX_TREE_ENTRIES:
            _error("candidate first-parent chain exceeds the fixed bound")
    chain.reverse()
    _review_binding(
        baseline_oid,
        candidate_oid,
        chain,
        review_head,
        review_base,
        review_state,
        repair_commits,
    )

    baseline = snapshot_from_reader(reader, baseline_oid)
    candidate_snapshot = snapshot_from_reader(reader, candidate_oid)
    candidate = RevisionSnapshot(
        commit_oid=candidate_snapshot.commit_oid,
        commit_sha256=candidate_snapshot.commit_sha256,
        tree_oid=candidate_snapshot.tree_oid,
        tree_manifest_sha256=candidate_snapshot.tree_manifest_sha256,
        parents=candidate_snapshot.parents,
        paths=candidate_snapshot.paths,
        commits=tuple(chain),
        changed_paths=_changed_paths(baseline.paths, candidate_snapshot.paths),
    )
    return RevisionBinding(baseline=baseline, candidate=candidate, target_oid=target_oid)
