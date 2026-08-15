#!/usr/bin/env python3
"""Execute a prepared benchmark from the exact manifest-verified file descriptors."""

from __future__ import annotations

import argparse
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


def _open_bound_file(parent_fd: int, name: str, expected: dict[str, Any]) -> int:
    flags = os.O_RDONLY
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
        digest = hashlib.sha256()
        size = 0
        while True:
            block = os.read(fd, 1024 * 1024)
            if not block:
                break
            digest.update(block)
            size += len(block)
        after = os.fstat(fd)
        identity_before = (
            before.st_dev,
            before.st_ino,
            before.st_size,
            stat.S_IMODE(before.st_mode),
            before.st_uid,
            before.st_gid,
        )
        identity_after = (
            after.st_dev,
            after.st_ino,
            after.st_size,
            stat.S_IMODE(after.st_mode),
            after.st_uid,
            after.st_gid,
        )
        if identity_before != identity_after:
            raise BoundExecError(f"prepared artifact changed while binding: {name}")
        observed_mode = f"100{stat.S_IMODE(after.st_mode):03o}"
        if (
            expected["kind"] != "file"
            or expected["mode"] != observed_mode
            or expected["uid"] != after.st_uid
            or expected["gid"] != after.st_gid
            or expected["size"] != size
            or expected["sha256"] != digest.hexdigest()
        ):
            raise BoundExecError(f"prepared artifact does not match the captured manifest: {name}")
        os.lseek(fd, 0, os.SEEK_SET)
        os.set_inheritable(fd, True)
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


def execute(root: str, executable: str) -> None:
    if not os.path.isabs(root):
        raise BoundExecError("prepared root must be absolute")
    if "/" in executable or executable in ("", ".", ".."):
        raise BoundExecError("prepared executable must be one path component")

    manifest_path = os.path.join(root, "artifact-manifest.json")
    captured, raw = manifest.load_manifest(manifest_path)
    captured_digest = manifest.manifest_sha256(raw)
    if manifest.verify_manifest(root, "artifact-manifest.json") != captured_digest:
        raise BoundExecError("prepared manifest changed during verification")
    entries = _entry_map(captured["entries"])
    executable_path = f"artifacts/{executable}"
    runtime_paths = [
        path
        for path in entries
        if path in ("artifacts/libalign_runtime.so", "artifacts/libalign_runtime.dylib")
    ]
    if executable_path not in entries or len(runtime_paths) != 1:
        raise BoundExecError("prepared manifest lacks the exact executable/runtime pair")

    root_flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        root_flags |= os.O_DIRECTORY
    if hasattr(os, "O_CLOEXEC"):
        root_flags |= os.O_CLOEXEC
    if hasattr(os, "O_NOFOLLOW"):
        root_flags |= os.O_NOFOLLOW
    root_fd = os.open(root, root_flags)
    artifacts_fd = _open_directory(root_fd, "artifacts")
    executable_fd = _open_bound_file(artifacts_fd, executable, entries[executable_path])
    runtime_name = runtime_paths[0].split("/", 1)[1]
    runtime_fd = _open_bound_file(artifacts_fd, runtime_name, entries[runtime_paths[0]])

    environment = dict(os.environ)
    environment.pop("PYTHONDONTWRITEBYTECODE", None)
    if sys.platform.startswith("linux"):
        environment["LD_PRELOAD"] = _fd_path(runtime_fd)
        executable_fd_path = _fd_path(executable_fd)
        os.execve(executable_fd_path, [executable_path], environment)
    if sys.platform == "darwin":
        # macOS has neither fexecve(2) nor execveat(2). This path keeps native ARM developer
        # qualification usable; accepted evidence is restricted by the profile to Linux x86_64,
        # where both executable and runtime are descriptor-bound above.
        os.execve(os.path.join(root, executable_path), [executable_path], environment)
    raise BoundExecError("fd-bound benchmark execution requires Linux or macOS")


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        execute(args.root, args.executable)
    except (BoundExecError, manifest.ManifestError, OSError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
