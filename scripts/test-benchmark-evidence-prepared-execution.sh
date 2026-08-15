#!/usr/bin/env bash
# Deterministic adversarial owners for the prepared-tree and bound-exec boundary.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - <<'PY'
import contextlib
import fcntl
import hashlib
import os
import stat
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path.cwd() / "scripts" / "benchmark_evidence"))

import manifest

from scripts.benchmark_evidence import bound_exec
from scripts.benchmark_evidence import prepared_tree


H64 = "0" * 64
SEALS = None
if all(hasattr(fcntl, name) for name in ("F_SEAL_SEAL", "F_SEAL_SHRINK", "F_SEAL_GROW", "F_SEAL_WRITE")):
    SEALS = (
        fcntl.F_SEAL_SEAL
        | fcntl.F_SEAL_SHRINK
        | fcntl.F_SEAL_GROW
        | fcntl.F_SEAL_WRITE
    )


def expect_error(call, fragment, error_types=(Exception,)):
    try:
        call()
    except error_types as exc:
        assert fragment in str(exc), (fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid input: {fragment}")


def expect_bound_error(call, fragment):
    before = fd_snapshot()
    try:
        expect_error(call, fragment, (bound_exec.BoundExecError,))
    finally:
        close_new_fds(before)


def fd_snapshot():
    if not sys.platform.startswith("linux"):
        return set()
    try:
        return {int(name) for name in os.listdir("/proc/self/fd") if name.isdigit()}
    except OSError:
        return set()


def close_new_fds(before):
    if not before:
        return
    for fd in fd_snapshot() - before:
        if fd > 2:
            with contextlib.suppress(OSError):
                os.close(fd)


def mkdir(path):
    path.mkdir(mode=0o700)
    return path


def write(path, data, mode=0o644):
    path.write_bytes(data)
    path.chmod(mode)
    return path


def open_dir(path):
    flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        flags |= os.O_DIRECTORY
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    return os.open(path, flags)


@contextlib.contextmanager
def prepared_fixture():
    with tempfile.TemporaryDirectory(prefix="align-prepared-owner-") as name:
        root = Path(name) / "prepared"
        mkdir(root)
        artifacts = mkdir(root / "artifacts")
        release = mkdir(mkdir(root / "root-target") / "release")
        write(release / "alignc", b"compiler-bytes", 0o755)
        write(release / "libalign_runtime.so", b"runtime-bytes", 0o755)
        root_fd = open_dir(root)
        artifacts_fd = open_dir(artifacts)
        old_root_fd, old_artifacts_fd = prepared_tree.ROOT_FD, prepared_tree.ARTIFACTS_FD
        prepared_tree.ROOT_FD = root_fd
        prepared_tree.ARTIFACTS_FD = artifacts_fd
        try:
            yield root, artifacts, release, root_fd, artifacts_fd
        finally:
            prepared_tree.ROOT_FD = old_root_fd
            prepared_tree.ARTIFACTS_FD = old_artifacts_fd
            os.close(artifacts_fd)
            os.close(root_fd)


def prepared_tree_owner():
    with prepared_fixture() as (root, artifacts, release, root_fd, artifacts_fd):
        prepared_tree._copy("root-target/release/alignc", "alignc", 0o755)
        assert (artifacts / "alignc").read_bytes() == b"compiler-bytes"
        assert stat.S_IMODE((artifacts / "alignc").stat().st_mode) == 0o755

        runtime_name = prepared_tree._copy_runtime()
        assert runtime_name == "libalign_runtime.so"
        prepared_tree._create_configuration("json_decode")
        assert (root / "configuration.json").read_bytes() == (
            b'{"schema":"align.json_escape_benchmark_artifacts/v1",'
            b'"benchmark":"json_decode","target":"native"}\n'
        )
        prepared_tree._chmod_artifact("alignc", 0o755)
        prepared_tree._verify_artifacts_directory()

        digest = manifest.write_manifest_fd(root_fd, "artifact-manifest.json")
        captured, raw = manifest.verify_manifest_fd(root_fd, "artifact-manifest.json")
        assert digest == manifest.manifest_sha256(raw)
        prepared_tree._verify_manifest_artifacts(captured["entries"])

        expect_error(
            lambda: prepared_tree._copy("root-target/release/alignc", "../escape", 0o755),
            "one path component",
            (prepared_tree.PreparedTreeError,),
        )
        expect_error(
            lambda: prepared_tree._copy("root-target/release/absent", "missing", 0o644),
            "cannot open file",
            (manifest.ManifestError,),
        )
        expect_error(
            lambda: prepared_tree._create_configuration("unknown"),
            "unsupported prepared benchmark",
            (prepared_tree.PreparedTreeError,),
        )
        expect_error(
            lambda: prepared_tree._chmod_artifact("../alignc", 0o755),
            "one path component",
            (prepared_tree.PreparedTreeError,),
        )
        expect_error(
            lambda: prepared_tree._clear_build_trees(["artifacts"]),
            "invalid prepared-tree clear target",
            (prepared_tree.PreparedTreeError,),
        )

        outside = Path(root.parent) / "outside"
        write(outside, b"outside-original")
        (artifacts / "escape").symlink_to(outside)
        expect_error(
            lambda: prepared_tree._copy("root-target/release/alignc", "escape", 0o755),
            "File exists",
            (OSError,),
        )
        assert outside.read_bytes() == b"outside-original"

        build = mkdir(root / "build")
        nested = mkdir(build / "nested")
        write(nested / "object", b"object")
        (build / "link").symlink_to(outside)
        prepared_tree._clear_build_trees(["build"])
        build_fd = open_dir(build)
        try:
            prepared_tree._verify_cleared_directory(build_fd)
            assert not (nested / "object").exists()
            assert not (build / "link").exists()
            write(build / "foreign", b"foreign")
            expect_error(
                lambda: prepared_tree._verify_cleared_directory(build_fd),
                "non-directory entry",
                (prepared_tree.PreparedTreeError,),
            )
        finally:
            os.close(build_fd)

        replacement = mkdir(root / "replacement")
        os.rename(artifacts, root / "old-artifacts")
        os.rename(replacement, artifacts)
        expect_error(
            prepared_tree._verify_artifacts_directory,
            "prepared artifacts directory was replaced",
            (prepared_tree.PreparedTreeError,),
        )

    with prepared_fixture() as (root, artifacts, release, root_fd, artifacts_fd):
        prepared_tree._copy("root-target/release/alignc", "alignc", 0o755)
        prepared_tree._copy_runtime()
        prepared_tree._create_configuration("json_soa")
        manifest.write_manifest_fd(root_fd, "artifact-manifest.json")
        (artifacts / "alignc").write_bytes(b"mutated")
        expect_error(
            lambda: manifest.verify_manifest_fd(root_fd, "artifact-manifest.json"),
            "does not match the manifest",
            (manifest.ManifestError,),
        )


def bound_fixture(extra=(), runtime_names=("libalign_runtime.so",)):
    temp = tempfile.TemporaryDirectory(prefix="align-bound-owner-")
    root = Path(temp.name) / "prepared"
    mkdir(root)
    artifacts = mkdir(root / "artifacts")
    write(artifacts / "alignc", b"alignc", 0o755)
    write(artifacts / "kernel.o", b"kernel", 0o644)
    write(artifacts / "bench", b"benchmark", 0o755)
    for runtime_name in runtime_names:
        write(artifacts / runtime_name, runtime_name.encode("ascii"), 0o755)
    for name, data, mode in extra:
        write(artifacts / name, data, mode)
    root_fd = open_dir(root)
    digest = manifest.write_manifest_fd(root_fd, "artifact-manifest.json")
    st = os.fstat(root_fd)
    return temp, root, artifacts, root_fd, digest, f"{st.st_dev}:{st.st_ino}"


def bound_call(root, root_fd, identity, digest, executable="bench"):
    return lambda: bound_exec.execute(
        str(root), root_fd, identity, digest, executable
    )


class ExecCaptured(Exception):
    pass


def bound_exec_owner():
    temp, root, artifacts, root_fd, digest, identity = bound_fixture()
    try:
        assert bound_exec._parse_root_identity(identity) == tuple(
            int(part) for part in identity.split(":")
        )
        for bad in ("", "1", "1:2:3", "x:2", "1:-2", "1:2 "):
            expect_error(
                lambda bad=bad: bound_exec._parse_root_identity(bad),
                "decimal device:inode",
                (bound_exec.BoundExecError,),
            )

        captured = {}

        def fake_execve(path, argv, environment):
            captured.update(path=path, argv=argv, environment=dict(environment))
            raise ExecCaptured()

        old_execve = bound_exec.os.execve
        bound_exec.os.execve = fake_execve
        before = fd_snapshot()
        try:
            expect_error(
                bound_call(root, root_fd, identity, digest),
                "",
                (ExecCaptured,),
            )
        finally:
            bound_exec.os.execve = old_execve
            close_new_fds(before)
        assert captured["argv"] == ["artifacts/bench"]
        if sys.platform.startswith("linux"):
            assert captured["path"].startswith("/proc/self/fd/")
            assert set(captured["environment"]) == {
                "CARGO_NET_OFFLINE", "HOME", "LC_ALL", "PATH", "TZ", "LD_PRELOAD"
            }
            assert captured["environment"]["LD_PRELOAD"].startswith("/proc/self/fd/")
        else:
            assert captured["path"] == "artifacts/bench"
            assert captured["environment"] == {
                "CARGO_NET_OFFLINE": "true",
                "HOME": "",
                "LC_ALL": "C",
                "PATH": "/usr/bin:/bin",
                "TZ": "UTC",
                "DYLD_LIBRARY_PATH": "artifacts",
                "DYLD_SHARED_REGION": "private",
            }

        expect_bound_error(
            bound_call(root, root_fd, identity, "g" * 64),
            "lowercase SHA-256",
        )
        expect_bound_error(
            bound_call(root, root_fd, "0:0", digest),
            "root changed",
        )
        expect_bound_error(
            bound_call(root, root_fd, identity, digest, "artifacts/bench"),
            "one path component",
        )
        expect_bound_error(
            lambda: bound_exec.execute("relative", root_fd, identity, digest, "bench"),
            "root must be absolute",
        )
        extra_fd = os.open(root / "artifact-manifest.json", os.O_RDONLY)
        try:
            expect_bound_error(
                lambda: bound_exec.execute(str(root), extra_fd, identity, digest, "bench"),
                "not a directory",
            )
        finally:
            os.close(extra_fd)

        entry = manifest.verify_manifest_fd(root_fd, "artifact-manifest.json")[0]["entries"]
        bench_entry = next(item for item in entry if item["path"] == "artifacts/bench")
        wrong_entry = dict(bench_entry)
        wrong_entry["sha256"] = H64
        bench_fd = -1
        wrong_artifacts_fd = open_dir(artifacts)
        try:
            bench_fd = os.open("bench", os.O_RDONLY, dir_fd=wrong_artifacts_fd)
            expect_bound_error(
                lambda: bound_exec._open_bound_file(wrong_artifacts_fd, "bench", wrong_entry),
                "does not match",
            )
        finally:
            with contextlib.suppress(OSError):
                os.close(bench_fd)
            os.close(wrong_artifacts_fd)
    finally:
        os.close(root_fd)
        temp.cleanup()

    temp, root, artifacts, root_fd, digest, identity = bound_fixture(extra=(("extra", b"extra", 0o644),))
    try:
        expect_bound_error(
            bound_call(root, root_fd, identity, digest),
            "unexpected artifact set",
        )
    finally:
        os.close(root_fd)
        temp.cleanup()

    temp, root, artifacts, root_fd, digest, identity = bound_fixture(runtime_names=("libalign_runtime.so", "libalign_runtime.dylib"))
    try:
        expect_bound_error(
            bound_call(root, root_fd, identity, digest),
            "exact executable/runtime pair",
        )
    finally:
        os.close(root_fd)
        temp.cleanup()

    if sys.platform.startswith("linux"):
        source = tempfile.TemporaryFile()
        source.write(b"sealed-bytes")
        source.seek(0)
        memfd_available = False
        probe = -1
        if hasattr(os, "memfd_create") and hasattr(os, "MFD_ALLOW_SEALING") and SEALS is not None:
            try:
                probe = os.memfd_create("align-owner-probe", os.MFD_ALLOW_SEALING | bound_exec.MFD_EXEC_FLAG)
                memfd_available = True
            except OSError:
                memfd_available = False
            finally:
                with contextlib.suppress(OSError):
                    os.close(probe)
        if memfd_available:
            fd, size, digest = bound_exec._copy_to_sealed_memfd(source.fileno(), "owner", 12)
            try:
                assert size == 12
                assert digest == hashlib.sha256(b"sealed-bytes").hexdigest()
                assert fcntl.fcntl(fd, fcntl.F_GET_SEALS) == SEALS
                os.lseek(fd, 0, os.SEEK_SET)
                assert os.read(fd, 12) == b"sealed-bytes"
                expect_error(
                    lambda: os.write(fd, b"x"),
                    "Operation not permitted",
                    (OSError,),
                )
                os.close(fd)
            finally:
                with contextlib.suppress(OSError):
                    os.close(fd)
            source.seek(0)
            expect_error(
                lambda: bound_exec._copy_to_sealed_memfd(source.fileno(), "short", 1),
                "grew",
                (bound_exec.BoundExecError,),
            )
        else:
            expect_error(
                lambda: bound_exec._copy_to_sealed_memfd(source.fileno(), "missing", 1),
                "requires Linux memfd seals",
                (bound_exec.BoundExecError,),
            )
        source.close()


prepared_tree_owner()
bound_exec_owner()
print("prepared execution boundary checks passed")
PY
