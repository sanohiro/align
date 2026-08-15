#!/usr/bin/env python3
"""Descriptor-relative mutations for a prepared benchmark tree."""

from __future__ import annotations

import argparse
import os
import stat
import sys
from typing import Sequence

import manifest


ROOT_FD = 9
ARTIFACTS_FD = 8
CONFIG_SCHEMA = "align.json_escape_benchmark_artifacts/v1"


class PreparedTreeError(ValueError):
    """A prepared-tree mutation could not be performed safely."""


def _write_all(fd: int, data: bytes) -> None:
    written = 0
    while written < len(data):
        count = os.write(fd, data[written:])
        if count <= 0:
            raise PreparedTreeError("prepared-tree write made no progress")
        written += count


def _copy(source: str, destination: str, mode: int) -> None:
    if "/" in destination or destination in ("", ".", ".."):
        raise PreparedTreeError("artifact destination must be one path component")
    source_fd = manifest._open_relative_file(ROOT_FD, source)
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        destination_fd = os.open(destination, flags, mode, dir_fd=ARTIFACTS_FD)
    except BaseException:
        os.close(source_fd)
        raise
    try:
        before = os.fstat(source_fd)
        while True:
            block = os.read(source_fd, 1024 * 1024)
            if not block:
                break
            _write_all(destination_fd, block)
        after = os.fstat(source_fd)
        signature_before = (
            before.st_dev,
            before.st_ino,
            before.st_size,
            before.st_mtime_ns,
            before.st_ctime_ns,
        )
        signature_after = (
            after.st_dev,
            after.st_ino,
            after.st_size,
            after.st_mtime_ns,
            after.st_ctime_ns,
        )
        if signature_before != signature_after:
            raise PreparedTreeError(f"source changed while copying: {source}")
        os.fchmod(destination_fd, mode)
        os.fsync(destination_fd)
    finally:
        os.close(source_fd)
        os.close(destination_fd)


def _copy_runtime() -> str:
    candidates = (
        "root-target/release/libalign_runtime.so",
        "root-target/release/libalign_runtime.dylib",
    )
    present: list[str] = []
    for candidate in candidates:
        try:
            fd = manifest._open_relative_file(ROOT_FD, candidate)
        except manifest.ManifestError:
            continue
        else:
            os.close(fd)
            present.append(candidate)
    if len(present) != 1:
        raise PreparedTreeError("expected exactly one Align runtime dynamic library")
    destination = present[0].rsplit("/", 1)[1]
    _copy(present[0], destination, 0o755)
    return destination


def _create_configuration(benchmark: str) -> None:
    if benchmark not in ("json_decode", "json_soa"):
        raise PreparedTreeError("unsupported prepared benchmark name")
    raw = (
        f'{{"schema":"{CONFIG_SCHEMA}","benchmark":"{benchmark}","target":"native"}}\n'
    ).encode("ascii")
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    fd = os.open("configuration.json", flags, 0o644, dir_fd=ROOT_FD)
    try:
        _write_all(fd, raw)
        os.fchmod(fd, 0o644)
        os.fsync(fd)
    finally:
        os.close(fd)


def _chmod_artifact(name: str, mode: int) -> None:
    if "/" in name or name in ("", ".", ".."):
        raise PreparedTreeError("artifact name must be one path component")
    fd = manifest._open_file(ARTIFACTS_FD, name)
    try:
        os.fchmod(fd, mode)
    finally:
        os.close(fd)


def _clear_directory(fd: int) -> None:
    for entry in list(os.scandir(fd)):
        value = os.stat(entry.name, dir_fd=fd, follow_symlinks=False)
        if stat.S_ISDIR(value.st_mode):
            child_fd = manifest._open_directory(fd, entry.name)
            try:
                opened = os.fstat(child_fd)
                if (value.st_dev, value.st_ino) != (opened.st_dev, opened.st_ino):
                    raise PreparedTreeError("directory changed while opening for clearing")
                _clear_directory(child_fd)
            finally:
                os.close(child_fd)
        else:
            os.unlink(entry.name, dir_fd=fd)


def _clear_build_trees(names: Sequence[str]) -> None:
    for name in names:
        if "/" in name or name in ("", ".", "..", "artifacts"):
            raise PreparedTreeError("invalid prepared-tree clear target")
        value = os.stat(name, dir_fd=ROOT_FD, follow_symlinks=False)
        if not stat.S_ISDIR(value.st_mode):
            os.unlink(name, dir_fd=ROOT_FD)
            continue
        child_fd = manifest._open_directory(ROOT_FD, name)
        try:
            opened = os.fstat(child_fd)
            if (value.st_dev, value.st_ino) != (opened.st_dev, opened.st_ino):
                raise PreparedTreeError(f"clear target changed while opening: {name}")
            _clear_directory(child_fd)
        finally:
            os.close(child_fd)


def _verify_cleared_directory(fd: int) -> None:
    for entry in list(os.scandir(fd)):
        value = os.stat(entry.name, dir_fd=fd, follow_symlinks=False)
        if not stat.S_ISDIR(value.st_mode):
            raise PreparedTreeError("cleared prepared tree contains a non-directory entry")
        child_fd = manifest._open_directory(fd, entry.name)
        try:
            opened = os.fstat(child_fd)
            if (value.st_dev, value.st_ino) != (opened.st_dev, opened.st_ino):
                raise PreparedTreeError("cleared directory changed while opening")
            _verify_cleared_directory(child_fd)
        finally:
            os.close(child_fd)


def _verify_artifacts_directory() -> None:
    expected = os.stat("artifacts", dir_fd=ROOT_FD, follow_symlinks=False)
    actual = os.fstat(ARTIFACTS_FD)
    if not stat.S_ISDIR(expected.st_mode) or (expected.st_dev, expected.st_ino) != (
        actual.st_dev,
        actual.st_ino,
    ):
        raise PreparedTreeError("prepared artifacts directory was replaced")


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="benchmark-evidence-prepared-tree", allow_abbrev=False)
    subparsers = parser.add_subparsers(dest="command", required=True)
    copy = subparsers.add_parser("copy", allow_abbrev=False)
    copy.add_argument("--source", required=True)
    copy.add_argument("--destination", required=True)
    copy.add_argument("--mode", required=True, choices=("0644", "0755"))
    subparsers.add_parser("copy-runtime", allow_abbrev=False)
    configuration = subparsers.add_parser("write-configuration", allow_abbrev=False)
    configuration.add_argument("--benchmark", required=True)
    chmod = subparsers.add_parser("chmod-artifact", allow_abbrev=False)
    chmod.add_argument("--name", required=True)
    chmod.add_argument("--mode", required=True, choices=("0644", "0755"))
    clear_build_trees = subparsers.add_parser("clear-build-trees", allow_abbrev=False)
    clear_build_trees.add_argument("names", nargs="+")
    subparsers.add_parser("verify-artifacts", allow_abbrev=False)
    subparsers.add_parser("write-manifest", allow_abbrev=False)
    subparsers.add_parser("clear", allow_abbrev=False)
    subparsers.add_parser("verify-cleared", allow_abbrev=False)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        if args.command == "copy":
            _copy(args.source, args.destination, int(args.mode, 8))
        elif args.command == "copy-runtime":
            print(_copy_runtime())
        elif args.command == "write-configuration":
            _create_configuration(args.benchmark)
        elif args.command == "chmod-artifact":
            _chmod_artifact(args.name, int(args.mode, 8))
        elif args.command == "clear-build-trees":
            _clear_build_trees(args.names)
        elif args.command == "verify-artifacts":
            _verify_artifacts_directory()
        elif args.command == "write-manifest":
            digest = manifest.write_manifest_fd(ROOT_FD, "artifact-manifest.json")
            _, raw = manifest.verify_manifest_fd(ROOT_FD, "artifact-manifest.json")
            if digest != manifest.manifest_sha256(raw):
                raise PreparedTreeError("prepared manifest changed after writing")
            print(digest)
        elif args.command == "verify-cleared":
            _verify_cleared_directory(ROOT_FD)
        else:
            _clear_directory(ROOT_FD)
    except (OSError, manifest.ManifestError, PreparedTreeError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
