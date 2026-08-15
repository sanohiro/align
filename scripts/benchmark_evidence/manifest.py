#!/usr/bin/env python3
"""Generate and verify the evidence-controller installation manifest.

The manifest is intentionally small and deterministic.  It records the root
directory and every regular file or directory below it, excluding the manifest
itself.  Verification walks and opens the tree without following symlinks,
hashes file descriptors rather than path names, and compares the observed
tree with the canonical bytes supplied by the caller.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import stat
import sys
from dataclasses import dataclass
from typing import Any, Iterable, Iterator, Sequence


SCHEMA = "align.json_escape_benchmark_install_manifest/v1"
MAX_U32 = (1 << 32) - 1
MAX_U64 = (1 << 64) - 1
_MANIFEST_KEYS = ("schema", "manifest_path", "root", "entries")
_ROOT_KEYS = ("mode", "uid", "gid")
_ENTRY_KEYS = ("path", "kind", "mode", "uid", "gid", "size", "sha256")
_FILE_MODES = (0o644, 0o755)
_DIRECTORY_MODES = (0o700, 0o755)


class ManifestError(ValueError):
    """A manifest or installed tree violates the trusted contract."""


class _Object(dict):
    """A JSON object retaining source key order for canonical validation."""

    def __init__(self, pairs: Sequence[tuple[str, Any]]) -> None:
        super().__init__(pairs)
        self.pairs = tuple(pairs)


def _object(pairs: Sequence[tuple[str, Any]]) -> _Object:
    keys = [key for key, _ in pairs]
    if len(keys) != len(set(keys)):
        raise ManifestError("duplicate JSON object member")
    return _Object(pairs)


def _canonical_json(value: Any) -> bytes:
    return (
        json.dumps(
            value,
            ensure_ascii=True,
            allow_nan=False,
            separators=(",", ":"),
        ).encode("ascii")
        + b"\n"
    )


def canonical_bytes(manifest: dict[str, Any]) -> bytes:
    """Return the only accepted manifest serialization."""

    return _canonical_json(manifest)


def manifest_sha256(manifest_bytes: bytes) -> str:
    return hashlib.sha256(manifest_bytes).hexdigest()


def _absolute_directory(path: str) -> str:
    if not path or not os.path.isabs(path):
        raise ManifestError("root must be an absolute path")
    if os.path.islink(path):
        raise ManifestError("root final component must not be a symlink")
    try:
        resolved = os.path.realpath(path)
        st = os.stat(path, follow_symlinks=False)
    except OSError as exc:
        raise ManifestError(f"cannot inspect root: {exc}") from exc
    if not stat.S_ISDIR(st.st_mode):
        raise ManifestError("root is not a directory")
    if resolved == "/":
        raise ManifestError("root must not be the filesystem root")
    return resolved


def _relative_manifest_path(path: str) -> str:
    if not path or os.path.isabs(path) or "\\" in path or "\x00" in path:
        raise ManifestError("manifest path must be a relative POSIX path")
    normalized = path.replace(os.sep, "/")
    parts = normalized.split("/")
    if any(part in ("", ".", "..") for part in parts):
        raise ManifestError("manifest path contains an empty or traversal component")
    if any(ord(char) < 0x20 or ord(char) > 0x7E for char in normalized):
        raise ManifestError("manifest path must be printable ASCII")
    return normalized


def _mode(kind: str, mode: int) -> str:
    permissions = stat.S_IMODE(mode)
    if kind == "file":
        if permissions not in _FILE_MODES:
            raise ManifestError(f"unsupported installed file mode: {permissions:o}")
        return f"100{permissions:03o}"
    if kind == "directory":
        if permissions not in _DIRECTORY_MODES:
            raise ManifestError(f"unsupported installed directory mode: {permissions:o}")
        return f"040{permissions:03o}"
    raise ManifestError(f"unsupported installed entry kind: {kind}")


def _hash_fd(fd: int, expected_size: int) -> tuple[int, str]:
    digest = hashlib.sha256()
    total = 0
    while True:
        block = os.read(fd, 1024 * 1024)
        if not block:
            break
        digest.update(block)
        total += len(block)
        if total > MAX_U64:
            raise ManifestError("file size exceeds u64")
    if total != expected_size:
        raise ManifestError("file changed while it was being hashed")
    return total, digest.hexdigest()


def _open_directory(parent_fd: int, name: str) -> int:
    flags = os.O_RDONLY | os.O_DIRECTORY
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        fd = os.open(name, flags, dir_fd=parent_fd)
    except OSError as exc:
        raise ManifestError(f"cannot open directory component {name!r}: {exc}") from exc
    st = os.fstat(fd)
    if not stat.S_ISDIR(st.st_mode):
        os.close(fd)
        raise ManifestError(f"directory component {name!r} is not a directory")
    return fd


def _open_file(parent_fd: int, name: str) -> int:
    flags = os.O_RDONLY
    if hasattr(os, "O_NONBLOCK"):
        flags |= os.O_NONBLOCK
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        fd = os.open(name, flags, dir_fd=parent_fd)
    except OSError as exc:
        raise ManifestError(f"cannot open file {name!r}: {exc}") from exc
    st = os.fstat(fd)
    if not stat.S_ISREG(st.st_mode):
        os.close(fd)
        raise ManifestError(f"installed entry {name!r} is not a regular file")
    return fd


def _open_root(root: str) -> int:
    flags = os.O_RDONLY | os.O_DIRECTORY
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        expected = os.stat(root, follow_symlinks=False)
        fd = os.open(root, flags)
    except OSError as exc:
        raise ManifestError(f"cannot open root without following symlinks: {exc}") from exc
    try:
        actual = os.fstat(fd)
    except OSError as exc:
        os.close(fd)
        raise ManifestError(f"cannot stat opened root: {exc}") from exc
    if (
        expected.st_dev,
        expected.st_ino,
        stat.S_IMODE(expected.st_mode),
        expected.st_uid,
        expected.st_gid,
    ) != (
        actual.st_dev,
        actual.st_ino,
        stat.S_IMODE(actual.st_mode),
        actual.st_uid,
        actual.st_gid,
    ):
        os.close(fd)
        raise ManifestError("root changed while it was being opened")
    return fd


def _open_relative_file(root_fd: int, path: str) -> int:
    """Open a relative file below root without following any path component."""

    parts = path.split("/")
    parent_fd = os.dup(root_fd)
    os.set_inheritable(parent_fd, False)
    try:
        for part in parts[:-1]:
            child_fd = _open_directory(parent_fd, part)
            os.close(parent_fd)
            parent_fd = child_fd
        return _open_file(parent_fd, parts[-1])
    finally:
        os.close(parent_fd)


def _open_absolute_file(path: str) -> int:
    """Open an absolute file without following any ancestor component."""

    if not path.startswith("/"):
        raise ManifestError("manifest file must be absolute")
    relative = path[1:]
    _relative_manifest_path(relative)
    flags = os.O_RDONLY | os.O_DIRECTORY
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        root_fd = os.open("/", flags)
    except OSError as exc:
        raise ManifestError(f"cannot open filesystem root: {exc}") from exc
    try:
        return _open_relative_file(root_fd, relative)
    finally:
        os.close(root_fd)


def _read_fd(fd: int) -> bytes:
    try:
        os.lseek(fd, 0, os.SEEK_SET)
    except OSError as exc:
        raise ManifestError(f"cannot seek manifest: {exc}") from exc
    chunks: list[bytes] = []
    total = 0
    while True:
        try:
            block = os.read(fd, 1024 * 1024)
        except OSError as exc:
            raise ManifestError(f"cannot read manifest: {exc}") from exc
        if not block:
            return b"".join(chunks)
        total += len(block)
        if total > MAX_U64:
            raise ManifestError("manifest exceeds u64 size")
        chunks.append(block)


@dataclass(frozen=True)
class _Observed:
    path: str
    kind: str
    mode: str
    uid: int
    gid: int
    size: int
    sha256: str

    def as_dict(self) -> dict[str, Any]:
        return {
            "path": self.path,
            "kind": self.kind,
            "mode": self.mode,
            "uid": self.uid,
            "gid": self.gid,
            "size": self.size,
            "sha256": self.sha256,
        }


def _scan_directory(
    fd: int,
    prefix: str,
    excluded: str,
) -> list[tuple[str, str, os.stat_result]]:
    try:
        entries = sorted(os.scandir(fd), key=lambda entry: os.fsencode(entry.name))
    except OSError as exc:
        raise ManifestError(f"cannot enumerate installed tree: {exc}") from exc
    scanned: list[tuple[str, str, os.stat_result]] = []
    for entry in entries:
        name = entry.name
        if name in (".", "..") or "/" in name or "\\" in name or "\x00" in name:
            raise ManifestError("installed path contains an invalid component")
        path = f"{prefix}/{name}" if prefix else name
        path = _relative_manifest_path(path)
        try:
            lstat = os.stat(name, dir_fd=fd, follow_symlinks=False)
        except OSError as exc:
            raise ManifestError(f"cannot inspect installed entry {path}: {exc}") from exc
        if not (stat.S_ISDIR(lstat.st_mode) or stat.S_ISREG(lstat.st_mode)):
            if stat.S_ISLNK(lstat.st_mode):
                raise ManifestError(f"installed symlink is not permitted: {path}")
            raise ManifestError(f"installed special file is not permitted: {path}")
        scanned.append((name, path, lstat))
    return scanned


def _directory_signature(
    entries: list[tuple[str, str, os.stat_result]],
) -> tuple[tuple[object, ...], ...]:
    return tuple(
        (
            path,
            "directory" if stat.S_ISDIR(lstat.st_mode) else "file",
            lstat.st_dev,
            lstat.st_ino,
            stat.S_IMODE(lstat.st_mode),
            lstat.st_uid,
            lstat.st_gid,
            lstat.st_size,
            lstat.st_mtime_ns,
            lstat.st_ctime_ns,
        )
        for _, path, lstat in entries
    )


def _metadata_signature(stat_result: os.stat_result) -> tuple[object, ...]:
    return (
        stat_result.st_dev,
        stat_result.st_ino,
        stat.S_IMODE(stat_result.st_mode),
        stat_result.st_uid,
        stat_result.st_gid,
        stat_result.st_size,
        stat_result.st_mtime_ns,
        stat_result.st_ctime_ns,
    )


def _iter_tree(
    fd: int,
    prefix: str = "",
    excluded: str = "",
    seen_files: set[tuple[int, int]] | None = None,
) -> Iterator[_Observed]:
    if seen_files is None:
        seen_files = set()
    entries = _scan_directory(fd, prefix, excluded)
    before_signature = _directory_signature(entries)
    for name, path, lstat in entries:
        if path == excluded:
            continue
        if stat.S_ISDIR(lstat.st_mode):
            child_fd = _open_directory(fd, name)
            try:
                child_stat = os.fstat(child_fd)
                if (
                    lstat.st_dev,
                    lstat.st_ino,
                    stat.S_IMODE(lstat.st_mode),
                    lstat.st_uid,
                    lstat.st_gid,
                ) != (
                    child_stat.st_dev,
                    child_stat.st_ino,
                    stat.S_IMODE(child_stat.st_mode),
                    child_stat.st_uid,
                    child_stat.st_gid,
                ):
                    raise ManifestError(f"installed directory changed while opening: {path}")
                try:
                    observed_mode = _mode("directory", child_stat.st_mode)
                except ManifestError as exc:
                    raise ManifestError(f"{exc}: {path}") from exc
                yield _Observed(
                    path=path,
                    kind="directory",
                    mode=observed_mode,
                    uid=child_stat.st_uid,
                    gid=child_stat.st_gid,
                    size=0,
                    sha256="",
                )
                yield from _iter_tree(child_fd, path, excluded, seen_files)
            finally:
                os.close(child_fd)
            continue
        if not stat.S_ISREG(lstat.st_mode):
            raise ManifestError(f"installed special file is not permitted: {path}")
        file_fd = _open_file(fd, name)
        try:
            before = os.fstat(file_fd)
            if before.st_dev != lstat.st_dev or before.st_ino != lstat.st_ino:
                raise ManifestError(f"installed entry changed while opening: {path}")
            identity = (before.st_dev, before.st_ino)
            if identity in seen_files:
                raise ManifestError(f"hard-linked installed files are not permitted: {path}")
            seen_files.add(identity)
            size, digest = _hash_fd(file_fd, before.st_size)
            after = os.fstat(file_fd)
            if (
                before.st_dev,
                before.st_ino,
                before.st_size,
                stat.S_IMODE(before.st_mode),
                before.st_uid,
                before.st_gid,
            ) != (
                after.st_dev,
                after.st_ino,
                after.st_size,
                stat.S_IMODE(after.st_mode),
                after.st_uid,
                after.st_gid,
            ):
                raise ManifestError(f"installed file changed while being hashed: {path}")
            try:
                observed_mode = _mode("file", before.st_mode)
            except ManifestError as exc:
                raise ManifestError(f"{exc}: {path}") from exc
            yield _Observed(
                path=path,
                kind="file",
                mode=observed_mode,
                uid=before.st_uid,
                gid=before.st_gid,
                size=size,
                sha256=digest,
            )
        finally:
            os.close(file_fd)
    after_signature = _directory_signature(_scan_directory(fd, prefix, excluded))
    if after_signature != before_signature:
        raise ManifestError(
            f"installed directory changed while walking: {prefix or '.'}"
        )


def _observed_tree_fd(root_fd: int, manifest_path: str) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    excluded = _relative_manifest_path(manifest_path)
    try:
        root_stat = os.fstat(root_fd)
        root_signature = _metadata_signature(root_stat)
        root_record = {
            "mode": _mode("directory", root_stat.st_mode),
            "uid": root_stat.st_uid,
            "gid": root_stat.st_gid,
        }
        entries = [item.as_dict() for item in _iter_tree(root_fd, excluded=excluded, seen_files=set())]
        entries.sort(key=lambda item: os.fsencode(item["path"]))
        if _metadata_signature(os.fstat(root_fd)) != root_signature:
            raise ManifestError("installed root changed while being walked")
        return root_record, entries
    except OSError as exc:
        raise ManifestError(f"cannot inspect installed root: {exc}") from exc


def _observed_tree(root: str, manifest_path: str) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    root = _absolute_directory(root)
    root_fd = _open_root(root)
    try:
        return _observed_tree_fd(root_fd, manifest_path)
    finally:
        os.close(root_fd)


def build_manifest(root: str, manifest_path: str = "manifest.json") -> dict[str, Any]:
    """Observe an installed tree and return its canonical manifest object."""

    manifest_path = _relative_manifest_path(manifest_path)
    root_record, entries = _observed_tree(root, manifest_path)
    return {
        "schema": SCHEMA,
        "manifest_path": manifest_path,
        "root": root_record,
        "entries": entries,
    }


def build_manifest_fd(root_fd: int, manifest_path: str = "manifest.json") -> dict[str, Any]:
    """Observe an already-opened installed root and return its manifest object."""

    manifest_path = _relative_manifest_path(manifest_path)
    root_record, entries = _observed_tree_fd(root_fd, manifest_path)
    return {
        "schema": SCHEMA,
        "manifest_path": manifest_path,
        "root": root_record,
        "entries": entries,
    }


def _expect_object(value: Any, keys: Sequence[str], label: str) -> _Object:
    if not isinstance(value, _Object) or tuple(key for key, _ in value.pairs) != tuple(keys):
        raise ManifestError(f"{label} has the wrong member order or shape")
    return value


def _expect_string(value: Any, label: str) -> str:
    if not isinstance(value, str):
        raise ManifestError(f"{label} is not a string")
    return value


def _expect_uint(value: Any, label: str, maximum: int) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0 or value > maximum:
        raise ManifestError(f"{label} is not an unsigned integer in range")
    return value


def _validate_manifest_object(value: Any) -> dict[str, Any]:
    obj = _expect_object(value, _MANIFEST_KEYS, "manifest")
    if _expect_string(obj["schema"], "schema") != SCHEMA:
        raise ManifestError("unsupported manifest schema")
    manifest_path = _relative_manifest_path(_expect_string(obj["manifest_path"], "manifest_path"))
    root = _expect_object(obj["root"], _ROOT_KEYS, "root")
    root_mode = _expect_string(root["mode"], "root.mode")
    if root_mode not in ("040700", "040755"):
        raise ManifestError("root.mode is not a supported directory mode")
    _expect_uint(root["uid"], "root.uid", MAX_U32)
    _expect_uint(root["gid"], "root.gid", MAX_U32)
    entries = obj["entries"]
    if not isinstance(entries, list):
        raise ManifestError("entries is not an array")
    last_path = ""
    seen: set[str] = set()
    for index, entry_value in enumerate(entries):
        entry = _expect_object(entry_value, _ENTRY_KEYS, f"entries[{index}]")
        path = _relative_manifest_path(_expect_string(entry["path"], f"entries[{index}].path"))
        if path == manifest_path or path in seen or (last_path and os.fsencode(path) <= os.fsencode(last_path)):
            raise ManifestError("entries are not unique and path-byte ordered")
        seen.add(path)
        last_path = path
        kind = _expect_string(entry["kind"], f"entries[{index}].kind")
        if kind not in ("file", "directory"):
            raise ManifestError("entry kind is not supported")
        mode = _expect_string(entry["mode"], f"entries[{index}].mode")
        if kind == "file" and mode not in ("100644", "100755"):
            raise ManifestError("file mode is not supported")
        if kind == "directory" and mode not in ("040700", "040755"):
            raise ManifestError("directory mode is not supported")
        _expect_uint(entry["uid"], f"entries[{index}].uid", MAX_U32)
        _expect_uint(entry["gid"], f"entries[{index}].gid", MAX_U32)
        size = _expect_uint(entry["size"], f"entries[{index}].size", MAX_U64)
        digest = _expect_string(entry["sha256"], f"entries[{index}].sha256")
        if kind == "directory" and (size != 0 or digest != ""):
            raise ManifestError("directory size/digest must be empty values")
        if kind == "file" and (len(digest) != 64 or any(char not in "0123456789abcdef" for char in digest)):
            raise ManifestError("file digest is not lowercase SHA-256")
    return dict(value)


def _parse_manifest_bytes(raw: bytes) -> tuple[dict[str, Any], bytes]:
    """Parse and canonicality-check manifest bytes already read from an fd."""

    def reject_constant(_: str) -> Any:
        raise ManifestError("non-finite JSON number")

    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=_object, parse_constant=reject_constant)
    except (UnicodeDecodeError, json.JSONDecodeError, ManifestError) as exc:
        raise ManifestError(f"manifest is not valid UTF-8 JSON: {exc}") from exc
    manifest = _validate_manifest_object(value)
    canonical = canonical_bytes(manifest)
    if raw != canonical:
        raise ManifestError("manifest is not canonical JSON")
    return manifest, raw


def load_manifest(path: str) -> tuple[dict[str, Any], bytes]:
    """Read, parse, and canonicality-check a manifest file."""

    if not os.path.isabs(path) or os.path.islink(path):
        raise ManifestError("manifest file must be an absolute non-symlink path")
    try:
        fd = _open_absolute_file(path)
    except ManifestError:
        raise
    try:
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode):
            raise ManifestError("manifest is not a regular file")
        if stat.S_IMODE(before.st_mode) != 0o644:
            raise ManifestError("manifest file mode must be 0644")
        raw = _read_fd(fd)
        after = os.fstat(fd)
        if (
            before.st_dev,
            before.st_ino,
            before.st_size,
            stat.S_IMODE(before.st_mode),
            before.st_uid,
            before.st_gid,
        ) != (
            after.st_dev,
            after.st_ino,
            after.st_size,
            stat.S_IMODE(after.st_mode),
            after.st_uid,
            after.st_gid,
        ):
            raise ManifestError("manifest changed while being read")
    finally:
        os.close(fd)
    return _parse_manifest_bytes(raw)


def verify_manifest_fd(
    root_fd: int, manifest_path: str = "manifest.json"
) -> tuple[dict[str, Any], bytes]:
    """Verify an already-opened root and return its parsed manifest and exact bytes."""

    manifest_path = _relative_manifest_path(manifest_path)
    manifest_fd = _open_relative_file(root_fd, manifest_path)
    try:
        before = os.fstat(manifest_fd)
        raw = _read_fd(manifest_fd)
        after = os.fstat(manifest_fd)
        if _metadata_signature(before) != _metadata_signature(after):
            raise ManifestError("manifest changed while being read")
    finally:
        os.close(manifest_fd)
    manifest, raw = _parse_manifest_bytes(raw)
    if manifest["manifest_path"] != manifest_path:
        raise ManifestError("manifest path does not match the requested path")
    expected_root, expected_entries = manifest["root"], manifest["entries"]
    if stat.S_IMODE(before.st_mode) != 0o644:
        raise ManifestError("manifest file mode must be 0644")
    if (before.st_uid, before.st_gid) != (expected_root["uid"], expected_root["gid"]):
        raise ManifestError("manifest owner does not match the installed root")
    observed_root, observed_entries = _observed_tree_fd(root_fd, manifest_path)
    if observed_root != expected_root or observed_entries != expected_entries:
        raise ManifestError("installed tree does not match the manifest")
    return manifest, raw


def verify_manifest(root: str, manifest_path: str = "manifest.json") -> str:
    """Verify the installed tree and return the manifest SHA-256."""

    manifest_path = _relative_manifest_path(manifest_path)
    root = _absolute_directory(root)
    root_fd = _open_root(root)
    try:
        _, raw = verify_manifest_fd(root_fd, manifest_path)
        return manifest_sha256(raw)
    finally:
        os.close(root_fd)


def write_manifest_fd(root_fd: int, path: str = "manifest.json") -> str:
    """Write a new manifest below an already-opened root."""

    path = _relative_manifest_path(path)
    manifest = build_manifest_fd(root_fd, path)
    raw = canonical_bytes(manifest)
    parts = path.split("/")
    parent_fd = os.dup(root_fd)
    os.set_inheritable(parent_fd, False)
    try:
        for part in parts[:-1]:
            child_fd = _open_directory(parent_fd, part)
            os.close(parent_fd)
            parent_fd = child_fd
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
        if hasattr(os, "O_CLOEXEC"):
            flags |= os.O_CLOEXEC
        if hasattr(os, "O_NOFOLLOW"):
            flags |= os.O_NOFOLLOW
        try:
            fd = os.open(parts[-1], flags, 0o644, dir_fd=parent_fd)
        except FileExistsError as exc:
            raise ManifestError("refusing to overwrite an existing manifest") from exc
    finally:
        os.close(parent_fd)
    try:
        os.fchmod(fd, 0o644)
        written = 0
        while written < len(raw):
            count = os.write(fd, raw[written:])
            if count <= 0:
                raise ManifestError("manifest write made no progress")
            written += count
        os.fsync(fd)
        if os.fstat(fd).st_size != len(raw):
            raise ManifestError("manifest write was truncated")
    finally:
        os.close(fd)
    return manifest_sha256(raw)


def _write_manifest(root: str, path: str) -> str:
    path = _relative_manifest_path(path)
    root = _absolute_directory(root)
    absolute = os.path.join(root, *path.split("/"))
    os.makedirs(os.path.dirname(absolute), mode=0o700, exist_ok=True)
    root_fd = _open_root(root)
    try:
        return write_manifest_fd(root_fd, path)
    finally:
        os.close(root_fd)


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="benchmark-evidence-manifest", allow_abbrev=False)
    subparsers = parser.add_subparsers(dest="command", required=True)
    write = subparsers.add_parser("write", allow_abbrev=False)
    write.add_argument("--root", required=True)
    write.add_argument("--manifest", default="manifest.json")
    verify = subparsers.add_parser("verify", allow_abbrev=False)
    verify.add_argument("--root", required=True)
    verify.add_argument("--manifest", default="manifest.json")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        if args.command == "write":
            digest = _write_manifest(args.root, args.manifest)
        else:
            digest = verify_manifest(args.root, args.manifest)
    except ManifestError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    print(digest)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
