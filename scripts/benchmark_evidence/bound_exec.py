#!/usr/bin/env python3
"""Execute a prepared benchmark from the exact manifest-verified file descriptors."""

from __future__ import annotations

import argparse
import fcntl
import hashlib
import os
import stat
import sys
from typing import Any, Sequence

import manifest


class BoundExecError(ValueError):
    """Prepared artifacts cannot be bound safely for execution."""


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="benchmark-evidence-bound-exec", allow_abbrev=False)
    parser.add_argument("--root", required=True)
    parser.add_argument("--root-fd", required=True, type=int)
    parser.add_argument("--root-identity", required=True)
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--executable", required=True)
    return parser


def _entry_map(entries: Sequence[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    return {entry["path"]: entry for entry in entries}


def _open_directory(parent_fd: int, name: str) -> int:
    flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        flags |= os.O_DIRECTORY
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        fd = os.open(name, flags, dir_fd=parent_fd)
    except OSError as exc:
        raise BoundExecError(f"cannot open prepared directory {name}: {exc}") from exc
    if not stat.S_ISDIR(os.fstat(fd).st_mode):
        os.close(fd)
        raise BoundExecError(f"prepared path is not a directory: {name}")
    return fd


def _metadata_signature(value: os.stat_result) -> tuple[int, ...]:
    return (
        value.st_dev,
        value.st_ino,
        value.st_size,
        stat.S_IMODE(value.st_mode),
        value.st_uid,
        value.st_gid,
        value.st_mtime_ns,
        value.st_ctime_ns,
    )


def _copy_to_sealed_memfd(
    source_fd: int, name: str, expected_size: int
) -> tuple[int, int, str]:
    if not hasattr(os, "memfd_create") or not hasattr(os, "MFD_ALLOW_SEALING"):
        raise BoundExecError("sealed benchmark execution requires Linux memfd seals")
    flags = os.MFD_ALLOW_SEALING
    if hasattr(os, "MFD_CLOEXEC"):
        flags |= os.MFD_CLOEXEC
    target_fd = os.memfd_create(f"align-benchmark-{name}", flags)
    try:
        digest = hashlib.sha256()
        size = 0
        while True:
            block = os.read(source_fd, 1024 * 1024)
            if not block:
                break
            digest.update(block)
            size += len(block)
            if size > expected_size:
                raise BoundExecError(f"prepared artifact grew while binding: {name}")
            written = 0
            while written < len(block):
                count = os.write(target_fd, block[written:])
                if count <= 0:
                    raise BoundExecError(f"sealed artifact copy made no progress: {name}")
                written += count
        seals = fcntl.F_SEAL_SEAL | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_GROW | fcntl.F_SEAL_WRITE
        fcntl.fcntl(target_fd, fcntl.F_ADD_SEALS, seals)
        if fcntl.fcntl(target_fd, fcntl.F_GET_SEALS) != seals:
            raise BoundExecError(f"prepared artifact was not fully sealed: {name}")
        os.lseek(target_fd, 0, os.SEEK_SET)
        os.set_inheritable(target_fd, True)
        return target_fd, size, digest.hexdigest()
    except BaseException:
        os.close(target_fd)
        raise


def _open_bound_file(parent_fd: int, name: str, expected: dict[str, Any]) -> int:
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
        raise BoundExecError(f"cannot open prepared artifact {name}: {exc}") from exc
    try:
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode):
            raise BoundExecError(f"prepared artifact is not a regular file: {name}")
        observed_mode = f"100{stat.S_IMODE(before.st_mode):03o}"
        if (
            expected["kind"] != "file"
            or expected["mode"] != observed_mode
            or expected["uid"] != before.st_uid
            or expected["gid"] != before.st_gid
            or expected["size"] != before.st_size
        ):
            raise BoundExecError(f"prepared artifact metadata does not match the manifest: {name}")
        sealed_fd, size, digest = _copy_to_sealed_memfd(fd, name, expected["size"])
        after = os.fstat(fd)
        if _metadata_signature(before) != _metadata_signature(after):
            os.close(sealed_fd)
            raise BoundExecError(f"prepared artifact changed while binding: {name}")
        if expected["size"] != size or expected["sha256"] != digest:
            os.close(sealed_fd)
            raise BoundExecError(f"prepared artifact does not match the captured manifest: {name}")
        return sealed_fd
    except BaseException:
        raise
    finally:
        os.close(fd)


def _open_verified_file(parent_fd: int, name: str, expected: dict[str, Any]) -> int:
    """Open and verify a source descriptor for native macOS qualification."""

    flags = os.O_RDONLY
    if hasattr(os, "O_NONBLOCK"):
        flags |= os.O_NONBLOCK
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    fd = os.open(name, flags, dir_fd=parent_fd)
    try:
        before = os.fstat(fd)
        digest = hashlib.sha256()
        size = 0
        while True:
            block = os.read(fd, 1024 * 1024)
            if not block:
                break
            digest.update(block)
            size += len(block)
        after = os.fstat(fd)
        observed_mode = f"100{stat.S_IMODE(after.st_mode):03o}"
        if (
            not stat.S_ISREG(after.st_mode)
            or _metadata_signature(before) != _metadata_signature(after)
            or expected["kind"] != "file"
            or expected["mode"] != observed_mode
            or expected["uid"] != after.st_uid
            or expected["gid"] != after.st_gid
            or expected["size"] != size
            or expected["sha256"] != digest.hexdigest()
        ):
            raise BoundExecError(f"prepared artifact does not match the captured manifest: {name}")
        return fd
    except BaseException:
        os.close(fd)
        raise


def _fd_path(fd: int) -> str:
    if sys.platform.startswith("linux"):
        return f"/proc/self/fd/{fd}"
    if sys.platform == "darwin":
        return f"/dev/fd/{fd}"
    raise BoundExecError("fd-bound benchmark execution requires Linux or macOS")


def _parse_root_identity(value: str) -> tuple[int, int]:
    parts = value.split(":")
    if len(parts) != 2 or any(not part.isascii() or not part.isdecimal() for part in parts):
        raise BoundExecError("prepared root identity must be decimal device:inode")
    identity = (int(parts[0]), int(parts[1]))
    if any(part < 0 for part in identity):
        raise BoundExecError("prepared root identity must be decimal device:inode")
    return identity


def execute(
    root: str,
    root_fd_number: int,
    root_identity: str,
    expected_manifest_sha256: str,
    executable: str,
) -> None:
    if not os.path.isabs(root):
        raise BoundExecError("prepared root must be absolute")
    if "/" in executable or executable in ("", ".", ".."):
        raise BoundExecError("prepared executable must be one path component")
    if (
        len(expected_manifest_sha256) != 64
        or any(char not in "0123456789abcdef" for char in expected_manifest_sha256)
    ):
        raise BoundExecError("prepared manifest digest must be lowercase SHA-256")

    expected_root_identity = _parse_root_identity(root_identity)
    try:
        root_fd = os.dup(root_fd_number)
    except OSError as exc:
        raise BoundExecError(f"cannot retain prepared root descriptor: {exc}") from exc
    os.set_inheritable(root_fd_number, False)
    os.set_inheritable(root_fd, False)
    opened_root = os.fstat(root_fd)
    if not stat.S_ISDIR(opened_root.st_mode):
        raise BoundExecError("prepared root descriptor is not a directory")
    if (opened_root.st_dev, opened_root.st_ino) != expected_root_identity:
        raise BoundExecError("prepared root changed before descriptor binding")
    captured, raw = manifest.verify_manifest_fd(root_fd, "artifact-manifest.json")
    if manifest.manifest_sha256(raw) != expected_manifest_sha256:
        raise BoundExecError("prepared manifest does not match the prepare-time digest")
    entries = _entry_map(captured["entries"])
    executable_path = f"artifacts/{executable}"
    runtime_paths = [
        path
        for path in entries
        if path in ("artifacts/libalign_runtime.so", "artifacts/libalign_runtime.dylib")
    ]
    if executable_path not in entries or len(runtime_paths) != 1:
        raise BoundExecError("prepared manifest lacks the exact executable/runtime pair")
    expected_artifact_paths = {
        "artifacts",
        "artifacts/alignc",
        "artifacts/kernel.o",
        executable_path,
        runtime_paths[0],
    }
    observed_artifact_paths = {
        path for path in entries if path == "artifacts" or path.startswith("artifacts/")
    }
    if observed_artifact_paths != expected_artifact_paths:
        raise BoundExecError("prepared manifest contains an unexpected artifact set")

    artifacts_fd = _open_directory(root_fd, "artifacts")
    runtime_name = runtime_paths[0].split("/", 1)[1]

    environment = {
        "CARGO_NET_OFFLINE": "true",
        "HOME": "",
        "LC_ALL": "C",
        "PATH": "/usr/bin:/bin",
        "TZ": "UTC",
    }
    if sys.platform.startswith("linux"):
        executable_fd = _open_bound_file(artifacts_fd, executable, entries[executable_path])
        runtime_fd = _open_bound_file(artifacts_fd, runtime_name, entries[runtime_paths[0]])
        environment["LD_PRELOAD"] = _fd_path(runtime_fd)
        executable_fd_path = _fd_path(executable_fd)
        os.execve(executable_fd_path, [executable_path], environment)
    if sys.platform == "darwin":
        # macOS has neither fexecve(2) nor execveat(2). This path keeps native ARM developer
        # qualification usable; accepted evidence is restricted by the profile to Linux x86_64,
        # where verified bytes are copied to write-sealed anonymous descriptors above.
        _open_verified_file(artifacts_fd, executable, entries[executable_path])
        _open_verified_file(artifacts_fd, runtime_name, entries[runtime_paths[0]])
        environment["DYLD_LIBRARY_PATH"] = "artifacts"
        os.fchdir(root_fd)
        os.execve(executable_path, [executable_path], environment)
    raise BoundExecError("fd-bound benchmark execution requires Linux or macOS")


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        execute(
            args.root,
            args.root_fd,
            args.root_identity,
            args.manifest_sha256,
            args.executable,
        )
    except (BoundExecError, manifest.ManifestError, OSError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
