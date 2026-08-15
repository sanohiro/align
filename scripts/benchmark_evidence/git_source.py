"""Materialize and re-verify one identity-bound raw Git tree.

This module is deliberately below the controller.  It consumes a verified
``RevisionSnapshot`` and raw blobs from the pinned object reader, creates a
private source tree without a checkout, and verifies that tree through a
retained root descriptor.  It never follows a source symlink or re-resolves
the root pathname during verification.
"""

from __future__ import annotations

import hashlib
import os
import secrets
import stat
import unicodedata
from dataclasses import dataclass
from typing import Sequence

from . import git_objects
from . import git_revision


class SourceError(ValueError):
    """A raw source tree or its materialized representation is invalid."""


def _close_fd_pair(root_fd: int, parent_fd: int) -> None:
    errors: list[OSError] = []
    for fd in (root_fd, parent_fd):
        try:
            os.close(fd)
        except OSError as exc:
            errors.append(exc)
    if errors:
        raise SourceError("cannot close materialized source descriptors") from errors[0]


MAX_SOURCE_FILE_BYTES = git_revision.MAX_OBJECT_BYTES
_IO_CHUNK_BYTES = 1024 * 1024
try:
    _DIRECTORY_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW
    _FILE_FLAGS = os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW
except AttributeError as exc:  # pragma: no cover - unsupported non-POSIX host
    raise RuntimeError("raw source binding requires no-follow POSIX open flags") from exc


@dataclass
class MaterializedSource:
    """Own one private source root and the descriptors used to re-verify it."""

    path: str
    snapshot: git_revision.RevisionSnapshot
    _root_fd: int
    _parent_fd: int
    _name: str
    _root_identity: tuple[int, int]
    _closed: bool = False
    _removed: bool = False

    @property
    def root_fd(self) -> int:
        if self._closed:
            raise SourceError("source root is closed")
        return self._root_fd

    def close(self) -> None:
        """Close retained descriptors without removing the source tree."""

        if self._closed:
            return
        try:
            _close_fd_pair(self._root_fd, self._parent_fd)
        finally:
            self._closed = True

    def remove(self) -> None:
        """Remove only the originally created root after identity checking."""

        if self._removed:
            return
        root_fd = self._root_fd
        parent_fd = self._parent_fd
        reopened = self._closed
        if reopened:
            parts = _absolute_components(self.path, "source root")
            parent_fd = _open_components(parts[:-1])
            root_fd = -1
            try:
                root_fd = os.open(parts[-1], _DIRECTORY_FLAGS, dir_fd=parent_fd)
                observed = os.fstat(root_fd)
                if (observed.st_dev, observed.st_ino) != self._root_identity or not stat.S_ISDIR(observed.st_mode):
                    _error("materialized source root was replaced")
            except BaseException:
                if root_fd >= 0:
                    try:
                        os.close(root_fd)
                    except OSError:
                        pass
                try:
                    os.close(parent_fd)
                except OSError:
                    pass
                raise
        failure: BaseException | None = None
        try:
            current = os.lstat(self._name, dir_fd=parent_fd)
            if (current.st_dev, current.st_ino) != self._root_identity or not stat.S_ISDIR(current.st_mode):
                raise SourceError("materialized source root was replaced")
            tombstone = f".align-source-remove-{os.getpid()}-{secrets.token_hex(16)}"
            os.rename(self._name, tombstone, src_dir_fd=parent_fd, dst_dir_fd=parent_fd)
            detached = os.lstat(tombstone, dir_fd=parent_fd)
            if (detached.st_dev, detached.st_ino) != self._root_identity or not stat.S_ISDIR(detached.st_mode):
                raise SourceError("materialized source root changed while detaching")
            _remove_tree_fd(root_fd)
            current = os.lstat(tombstone, dir_fd=parent_fd)
            if (current.st_dev, current.st_ino) != self._root_identity or not stat.S_ISDIR(current.st_mode):
                raise SourceError("materialized source root changed during cleanup")
            os.rmdir(tombstone, dir_fd=parent_fd)
            os.fsync(parent_fd)
            self._removed = True
        except SourceError as exc:
            failure = exc
        except OSError as exc:
            failure = SourceError("cannot remove materialized source")
            failure.__cause__ = exc
        finally:
            try:
                if reopened:
                    _close_fd_pair(root_fd, parent_fd)
                else:
                    self.close()
            except SourceError as exc:
                if failure is None:
                    failure = exc
        if failure is not None:
            raise failure

    def __enter__(self) -> "MaterializedSource":
        if self._closed:
            raise SourceError("source root is closed")
        return self

    def __exit__(self, exc_type, exc_value, traceback) -> None:
        self.remove()


@dataclass
class _Layout:
    leaves: dict[tuple[bytes, ...], git_revision.PathIdentity]
    directories: set[tuple[bytes, ...]]
    children: dict[tuple[bytes, ...], dict[bytes, str]]
    nodes: dict[tuple[bytes, ...], str]


def _error(message: str) -> None:
    raise SourceError(message)


def _absolute_components(path: object, label: str) -> tuple[str, ...]:
    if not isinstance(path, str) or not path or not path.startswith("/") or "\x00" in path:
        raise SourceError(f"{label} must be an absolute path without NUL")
    parts = tuple(path.split("/")[1:])
    if not parts or any(part in ("", ".", "..") for part in parts):
        raise SourceError(f"{label} must use canonical absolute path components")
    return parts


def _open_components(parts: Sequence[str]) -> int:
    fd = os.open("/", _DIRECTORY_FLAGS)
    try:
        for part in parts:
            next_fd = os.open(part, _DIRECTORY_FLAGS, dir_fd=fd)
            os.close(fd)
            fd = next_fd
        return fd
    except OSError as exc:
        try:
            os.close(fd)
        except OSError:
            pass
        raise SourceError("source parent is not a no-follow directory chain") from exc


def _source_components(path: bytes) -> tuple[bytes, ...]:
    if not isinstance(path, bytes) or not path or b"\x00" in path:
        _error("source path is empty or contains NUL")
    parts = tuple(path.split(b"/"))
    if any(not part for part in parts):
        _error("source path contains an empty component")
    for part in parts:
        try:
            text = part.decode("utf-8")
        except UnicodeDecodeError as exc:
            raise SourceError("source path is not valid UTF-8") from exc
        if text in (".", "..") or text.casefold() == ".git":
            _error("source path contains a forbidden component")
    return parts


def _normalized_key(parts: Sequence[bytes]) -> str:
    try:
        text = b"/".join(parts).decode("utf-8")
    except UnicodeDecodeError as exc:
        raise SourceError("source path is not valid UTF-8") from exc
    return unicodedata.normalize("NFC", text).casefold()


def _validate_identity(identity: git_revision.PathIdentity) -> tuple[bytes, ...]:
    if not isinstance(identity, git_revision.PathIdentity):
        _error("source inventory contains a non-path identity")
    parts = _source_components(identity.path)
    try:
        git_objects.validate_oid(identity.oid)
    except git_objects.GitObjectError as exc:
        raise SourceError(f"source identity has an invalid object ID: {exc}") from exc
    if not isinstance(identity.size, int) or identity.size < 0 or identity.size > MAX_SOURCE_FILE_BYTES:
        _error("source identity exceeds the fixed file-size bound")
    if (
        not isinstance(identity.sha256, str)
        or len(identity.sha256) != 64
        or any(char not in "0123456789abcdef" for char in identity.sha256)
    ):
        _error("source identity has an invalid SHA-256")
    if identity.kind == "blob" and identity.mode not in ("100644", "100755"):
        _error("source blob has an unsupported mode")
    if identity.kind == "symlink" and identity.mode != "120000":
        _error("source symlink has an unsupported mode")
    if identity.kind not in ("blob", "symlink"):
        _error("source identity has an unsupported kind")
    return parts


def _layout(paths: Sequence[git_revision.PathIdentity]) -> _Layout:
    leaves: dict[tuple[bytes, ...], git_revision.PathIdentity] = {}
    directories: set[tuple[bytes, ...]] = {()}
    nodes: dict[tuple[bytes, ...], str] = {}
    normalized: dict[str, tuple[tuple[bytes, ...], str]] = {}
    for identity in paths:
        parts = _validate_identity(identity)
        if parts in leaves:
            _error("source inventory contains a duplicate path")
        leaves[parts] = identity
        for index in range(1, len(parts) + 1):
            node = parts[:index]
            kind = "leaf" if index == len(parts) else "directory"
            key = _normalized_key(node)
            previous = normalized.get(key)
            if previous is not None and previous != (node, kind):
                _error("source inventory contains a normalized or case-fold collision")
            normalized[key] = (node, kind)
            existing = nodes.get(node)
            if existing is not None and existing != kind:
                _error("source inventory contains a file/directory collision")
            nodes[node] = kind
            if kind == "directory":
                directories.add(node)
    children: dict[tuple[bytes, ...], dict[bytes, str]] = {}
    for directory in directories:
        if directory:
            children.setdefault(directory[:-1], {})[directory[-1]] = "directory"
    for leaf in leaves:
        children.setdefault(leaf[:-1], {})[leaf[-1]] = "leaf"
    return _Layout(leaves, directories, children, nodes)


def _reviewed_symlink_map(
    reviewed_symlinks: Sequence[git_revision.PathIdentity],
) -> dict[bytes, git_revision.PathIdentity]:
    result: dict[bytes, git_revision.PathIdentity] = {}
    for identity in reviewed_symlinks:
        _validate_identity(identity)
        if identity.kind != "symlink":
            continue
        previous = result.get(identity.path)
        if previous is not None and previous != identity:
            _error("reviewed symlink identities disagree")
        result[identity.path] = identity
    return result


def _blob_payload(
    reader: git_revision.ObjectReader,
    identity: git_revision.PathIdentity,
    cache: dict[str, bytes],
) -> bytes:
    payload = cache.get(identity.oid)
    if payload is None:
        try:
            value = reader.read(identity.oid)
        except Exception as exc:
            raise SourceError(f"cannot read source blob {identity.oid}") from exc
        if value.kind != "blob" or value.oid != identity.oid:
            _error("source tree leaf does not name the expected blob")
        payload = value.payload
        cache[identity.oid] = payload
    if len(payload) != identity.size or hashlib.sha256(payload).hexdigest() != identity.sha256:
        _error("raw source blob does not match its path identity")
    return payload


def _symlink_target(
    identity: git_revision.PathIdentity,
    parts: tuple[bytes, ...],
    payload: bytes,
    layout: _Layout,
    reviewed: dict[bytes, git_revision.PathIdentity],
) -> tuple[bytes, ...]:
    if reviewed.get(identity.path) != identity:
        _error("new or changed source symlink is not reviewed")
    if not payload or b"\x00" in payload or payload.startswith(b"/"):
        _error("source symlink target is absolute, empty, or contains NUL")
    target_parts = tuple(payload.split(b"/"))
    if any(not part for part in target_parts):
        _error("source symlink target contains an empty component")
    for part in target_parts:
        try:
            text = part.decode("utf-8")
        except UnicodeDecodeError as exc:
            raise SourceError("source symlink target is not valid UTF-8") from exc
        if text in (".", "..") or text.casefold() == ".git":
            _error("source symlink target contains a forbidden component")
    target = parts[:-1] + target_parts
    if target not in layout.nodes:
        _error("source symlink target is not in the reviewed tree")
    return target


def _validate_symlinks(
    reader: git_revision.ObjectReader,
    layout: _Layout,
    reviewed: dict[bytes, git_revision.PathIdentity],
    cache: dict[str, bytes],
) -> dict[tuple[bytes, ...], bytes]:
    targets: dict[tuple[bytes, ...], bytes] = {}
    resolved: dict[tuple[bytes, ...], tuple[bytes, ...]] = {}
    for parts, identity in layout.leaves.items():
        if identity.kind != "symlink":
            continue
        payload = _blob_payload(reader, identity, cache)
        target = _symlink_target(identity, parts, payload, layout, reviewed)
        targets[parts] = payload
        resolved[parts] = target
    for start in resolved:
        current = start
        seen: set[tuple[bytes, ...]] = set()
        while current in resolved:
            if current in seen:
                _error("source symlink graph contains a cycle")
            seen.add(current)
            current = resolved[current]
    return targets


def _write_all(fd: int, payload: bytes) -> None:
    view = memoryview(payload)
    while view:
        count = os.write(fd, view)
        if count <= 0:
            _error("source file write made no progress")
        view = view[count:]


def _write_file(parent_fd: int, name: bytes, identity: git_revision.PathIdentity, payload: bytes) -> None:
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC | os.O_NOFOLLOW
    fd = os.open(name, flags, 0o600, dir_fd=parent_fd)
    try:
        _write_all(fd, payload)
        os.fchmod(fd, 0o755 if identity.mode == "100755" else 0o644)
        os.fsync(fd)
        observed = os.fstat(fd)
        if (
            not stat.S_ISREG(observed.st_mode)
            or stat.S_IMODE(observed.st_mode) != (0o755 if identity.mode == "100755" else 0o644)
            or observed.st_nlink != 1
            or observed.st_size != len(payload)
        ):
            _error("materialized source file changed while being written")
    finally:
        os.close(fd)


def _open_child_directory(parent_fd: int, name: bytes) -> int:
    return os.open(name, _DIRECTORY_FLAGS, dir_fd=parent_fd)


def _remove_tree_fd(fd: int) -> None:
    for raw_name in os.listdir(fd):
        name = os.fsencode(raw_name)
        before = os.lstat(name, dir_fd=fd)
        if stat.S_ISDIR(before.st_mode) and not stat.S_ISLNK(before.st_mode):
            child_fd = _open_child_directory(fd, name)
            try:
                child_stat = os.fstat(child_fd)
                if (child_stat.st_dev, child_stat.st_ino) != (before.st_dev, before.st_ino):
                    _error("source cleanup observed a replaced directory")
                _remove_tree_fd(child_fd)
                current = os.lstat(name, dir_fd=fd)
                if (current.st_dev, current.st_ino) != (before.st_dev, before.st_ino):
                    _error("source cleanup observed a replaced directory")
                os.rmdir(name, dir_fd=fd)
            finally:
                os.close(child_fd)
        else:
            current = os.lstat(name, dir_fd=fd)
            if (current.st_dev, current.st_ino) != (before.st_dev, before.st_ino):
                _error("source cleanup observed a replaced entry")
            os.unlink(name, dir_fd=fd)


def _new_source(path: str, snapshot: git_revision.RevisionSnapshot) -> MaterializedSource:
    parts = _absolute_components(path, "source root")
    parent_fd = _open_components(parts[:-1])
    name = parts[-1]
    root_fd = -1
    created = False
    created_identity: tuple[int, int] | None = None
    try:
        os.mkdir(name, 0o700, dir_fd=parent_fd)
        created = True
        created_stat = os.lstat(name, dir_fd=parent_fd)
        if not stat.S_ISDIR(created_stat.st_mode):
            raise SourceError("new source root is not a directory")
        created_identity = (created_stat.st_dev, created_stat.st_ino)
        root_fd = os.open(name, _DIRECTORY_FLAGS, dir_fd=parent_fd)
        observed = os.fstat(root_fd)
        if not stat.S_ISDIR(observed.st_mode):
            raise SourceError("new source root is not a directory")
        return MaterializedSource(
            path="/" + "/".join(parts),
            snapshot=snapshot,
            _root_fd=root_fd,
            _parent_fd=parent_fd,
            _name=name,
            _root_identity=(observed.st_dev, observed.st_ino),
        )
    except BaseException as exc:
        if root_fd >= 0:
            try:
                os.close(root_fd)
            except OSError:
                pass
        if created and created_identity is not None:
            try:
                current = os.lstat(name, dir_fd=parent_fd)
                if (
                    (current.st_dev, current.st_ino) == created_identity
                    and stat.S_ISDIR(current.st_mode)
                ):
                    os.rmdir(name, dir_fd=parent_fd)
            except OSError:
                pass
        try:
            os.close(parent_fd)
        except OSError:
            pass
        if isinstance(exc, SourceError):
            raise
        if isinstance(exc, OSError):
            raise SourceError("cannot create the new source root") from exc
        raise


def _close_directory_fds(directory_fds: dict[tuple[bytes, ...], int]) -> None:
    errors: list[OSError] = []
    for path, fd in sorted(directory_fds.items(), key=lambda item: len(item[0]), reverse=True):
        if not path:
            continue
        try:
            os.close(fd)
        except OSError as exc:
            errors.append(exc)
    if errors:
        raise SourceError("cannot close source directory descriptors") from errors[0]


def _check_snapshot(snapshot: git_revision.RevisionSnapshot) -> None:
    try:
        observed = git_revision.tree_manifest_sha256(snapshot.paths)
    except git_revision.GitRevisionError as exc:
        raise SourceError("source snapshot inventory is not canonical") from exc
    if observed != snapshot.tree_manifest_sha256:
        _error("source snapshot manifest identity does not match its paths")


def materialize_source(
    reader: git_revision.ObjectReader,
    snapshot: git_revision.RevisionSnapshot,
    destination: str,
    *,
    reviewed_symlinks: Sequence[git_revision.PathIdentity] = (),
) -> MaterializedSource:
    """Create and verify a new source root from raw blobs.

    Symlinks are accepted only when their complete baseline identity appears in
    ``reviewed_symlinks`` and their target remains inside the same tree.
    """

    _check_snapshot(snapshot)
    layout = _layout(snapshot.paths)
    reviewed = _reviewed_symlink_map(reviewed_symlinks)
    cache: dict[str, bytes] = {}
    symlink_payloads = _validate_symlinks(reader, layout, reviewed, cache)
    source = _new_source(destination, snapshot)
    directory_fds: dict[tuple[bytes, ...], int] = {(): source.root_fd}
    try:
        for directory in sorted(layout.directories - {()}, key=lambda item: (len(item), item)):
            parent_fd = directory_fds[directory[:-1]]
            os.mkdir(directory[-1], 0o755, dir_fd=parent_fd)
            child_fd = _open_child_directory(parent_fd, directory[-1])
            os.fchmod(child_fd, 0o755)
            directory_fds[directory] = child_fd
            os.fsync(parent_fd)
        for parts, identity in sorted(layout.leaves.items(), key=lambda item: b"/".join(item[0])):
            parent_fd = directory_fds[parts[:-1]]
            if identity.kind == "symlink":
                os.symlink(symlink_payloads[parts], parts[-1], dir_fd=parent_fd)
                os.fsync(parent_fd)
            else:
                payload = _blob_payload(reader, identity, cache)
                _write_file(parent_fd, parts[-1], identity, payload)
                os.fsync(parent_fd)
        verify_source(reader, snapshot, source, reviewed_symlinks=reviewed_symlinks, _cache=cache)
        return source
    except BaseException as exc:
        cleanup_error: BaseException | None = None
        try:
            source.remove()
        except BaseException as cleanup:
            cleanup_error = cleanup
        try:
            _close_directory_fds(directory_fds)
        except BaseException as cleanup:
            if cleanup_error is None:
                cleanup_error = cleanup
        if cleanup_error is not None:
            raise SourceError("source construction failed and cleanup was incomplete") from cleanup_error
        if isinstance(exc, SourceError):
            raise
        if isinstance(exc, OSError):
            raise SourceError("source construction failed") from exc
        raise
    finally:
        if not source._closed:
            _close_directory_fds(directory_fds)


def _read_file(fd: int, expected_size: int) -> bytes:
    chunks: list[bytes] = []
    total = 0
    while total <= expected_size:
        chunk = os.read(fd, min(_IO_CHUNK_BYTES, expected_size - total + 1))
        if not chunk:
            break
        chunks.append(chunk)
        total += len(chunk)
        if total > expected_size:
            _error("source file contains bytes beyond its identity")
    if total != expected_size:
        _error("source file is shorter than its identity")
    return b"".join(chunks)


def _verify_file(
    parent_fd: int,
    name: bytes,
    identity: git_revision.PathIdentity,
    expected: bytes,
    seen_inodes: set[tuple[int, int]],
) -> None:
    before = os.lstat(name, dir_fd=parent_fd)
    expected_mode = 0o755 if identity.mode == "100755" else 0o644
    if not stat.S_ISREG(before.st_mode) or stat.S_IMODE(before.st_mode) != expected_mode:
        _error("materialized source file type or mode does not match")
    if before.st_nlink != 1:
        _error("hard-linked source files are not permitted")
    inode = (before.st_dev, before.st_ino)
    if inode in seen_inodes:
        _error("source file identity is reused")
    seen_inodes.add(inode)
    fd = os.open(name, _FILE_FLAGS, dir_fd=parent_fd)
    try:
        opened = os.fstat(fd)
        if (opened.st_dev, opened.st_ino) != inode:
            _error("source file was replaced while opening")
        observed = _read_file(fd, identity.size)
        after = os.fstat(fd)
        if (
            (after.st_dev, after.st_ino) != inode
            or stat.S_IMODE(after.st_mode) != expected_mode
            or after.st_nlink != 1
            or after.st_size != identity.size
            or observed != expected
        ):
            _error("source file changed while being verified")
    finally:
        os.close(fd)


def _verify_symlink(
    parent_fd: int,
    name: bytes,
    identity: git_revision.PathIdentity,
    expected: bytes,
) -> None:
    observed = os.lstat(name, dir_fd=parent_fd)
    if not stat.S_ISLNK(observed.st_mode) or observed.st_nlink != 1:
        _error("materialized source symlink type does not match")
    try:
        target = os.readlink(name, dir_fd=parent_fd)
    except OSError as exc:
        raise SourceError("cannot read materialized source symlink") from exc
    if not isinstance(target, bytes) or target != expected:
        _error("materialized source symlink target changed")


def _verify_directory(
    fd: int,
    prefix: tuple[bytes, ...],
    layout: _Layout,
    reader: git_revision.ObjectReader,
    cache: dict[str, bytes],
    seen_inodes: set[tuple[int, int]],
) -> None:
    observed_names = {os.fsencode(name) for name in os.listdir(fd)}
    expected_children = layout.children.get(prefix, {})
    if observed_names != set(expected_children):
        _error("materialized source tree has missing or extra entries")
    for name in sorted(expected_children):
        child_kind = expected_children[name]
        child_path = prefix + (name,)
        if child_kind == "directory":
            before = os.lstat(name, dir_fd=fd)
            if not stat.S_ISDIR(before.st_mode) or stat.S_ISLNK(before.st_mode):
                _error("materialized source directory type does not match")
            child_fd = _open_child_directory(fd, name)
            try:
                opened = os.fstat(child_fd)
                if (opened.st_dev, opened.st_ino) != (before.st_dev, before.st_ino):
                    _error("source directory was replaced while opening")
                _verify_directory(child_fd, child_path, layout, reader, cache, seen_inodes)
                after = os.fstat(child_fd)
                if (after.st_dev, after.st_ino) != (before.st_dev, before.st_ino):
                    _error("source directory changed while being verified")
            finally:
                os.close(child_fd)
            continue
        identity = layout.leaves[child_path]
        expected = _blob_payload(reader, identity, cache)
        if identity.kind == "symlink":
            _verify_symlink(fd, name, identity, expected)
        else:
            _verify_file(fd, name, identity, expected, seen_inodes)


def verify_source(
    reader: git_revision.ObjectReader,
    snapshot: git_revision.RevisionSnapshot,
    source: MaterializedSource,
    *,
    reviewed_symlinks: Sequence[git_revision.PathIdentity] = (),
    _cache: dict[str, bytes] | None = None,
) -> str:
    """Verify every expected source byte, mode, type, and path via root FD."""

    if source._closed:
        raise SourceError("source root is closed")
    if source.snapshot != snapshot:
        _error("source snapshot does not match the materialized source")
    _check_snapshot(snapshot)
    layout = _layout(snapshot.paths)
    reviewed = _reviewed_symlink_map(reviewed_symlinks)
    cache = {} if _cache is None else _cache
    _validate_symlinks(reader, layout, reviewed, cache)
    root_stat = os.fstat(source.root_fd)
    if (root_stat.st_dev, root_stat.st_ino) != source._root_identity or not stat.S_ISDIR(root_stat.st_mode):
        _error("materialized source root changed")
    _verify_directory(source.root_fd, (), layout, reader, cache, set())
    after = os.fstat(source.root_fd)
    if (after.st_dev, after.st_ino) != source._root_identity:
        _error("materialized source root changed while being verified")
    return snapshot.tree_manifest_sha256
