"""Collect the trusted native host and Docker daemon qualification record.

The existing :mod:`host` module validates an already canonical observation.
This module owns the privileged acquisition boundary that precedes it: fixed
Linux source paths, the profile-owned host configuration, and the pinned
Docker client.  It never opens a repository, imports candidate code, runs an
image, starts a benchmark, or provisions a signing key.
"""

from __future__ import annotations

import errno
import hashlib
import json
import math
import os
import re
import selectors
import signal
import stat
import subprocess
import time
from dataclasses import dataclass
from typing import Any, Callable, Mapping, Sequence

from . import canonical_json as cj
from . import host as host_contract


class NativeHostError(RuntimeError):
    """A native host observation cannot cross the trusted acquisition boundary."""


class NativeHostSourceMissing(NativeHostError):
    """A fixed native source is absent, allowing an explicitly defined fallback."""


DOCKER = "/usr/bin/docker"
DOCKER_CONFIG = "/etc/align-evidence/docker-empty"
DOCKER_HOST = "unix:///var/run/docker.sock"
DOCKER_RUNTIME = "runc"
HOST_ID_PATH = "/etc/align-evidence/host-id"
BENCHMARK_CPU_SET_PATH = "/etc/align-evidence/benchmark-cpus"
CPUINFO_PATH = "/proc/cpuinfo"
MEMINFO_PATH = "/proc/meminfo"
VMSTAT_PATH = "/proc/vmstat"
LOADAVG_PATH = "/proc/loadavg"
CPU_PRESSURE_PATH = "/proc/pressure/cpu"
MEMORY_PRESSURE_PATH = "/proc/pressure/memory"
ONLINE_CPU_SET_PATH = "/sys/devices/system/cpu/online"
NUMA_SET_PATH = "/sys/devices/system/node/online"
CGROUP_V2_CPU_MAX_PATH = "/sys/fs/cgroup/cpu.max"
CGROUP_V1_CPU_QUOTA_PATH = "/sys/fs/cgroup/cpu/cpu.cfs_quota_us"
CGROUP_V1_CPU_PERIOD_PATH = "/sys/fs/cgroup/cpu/cpu.cfs_period_us"

DOCKER_VERSION_ARGV = (DOCKER, "--config", DOCKER_CONFIG, "--host", DOCKER_HOST, "version", "--format", "{{json .}}")
DOCKER_INFO_ARGV = (DOCKER, "--config", DOCKER_CONFIG, "--host", DOCKER_HOST, "info", "--format", "{{json .}}")

_ASCII_NAME = re.compile(r"[A-Za-z0-9._/:+=@-]{1,255}\Z")
_CPU_SET = re.compile(r"(?:0|[1-9][0-9]*)(?:-(?:0|[1-9][0-9]*))?(?:,(?:0|[1-9][0-9]*)(?:-(?:0|[1-9][0-9]*))?)*\Z")
_HEX64 = re.compile(r"[0-9a-f]{64}\Z")
_MAX_SOURCE_BYTES = 1 << 20
_MAX_COMMAND_OUTPUT = 64 << 10
_COMMAND_TIMEOUT_SECONDS = 5.0
_U64_MAX = (1 << 64) - 1

_FIXED_ENV = {
    "DOCKER_CONFIG": DOCKER_CONFIG,
    "DOCKER_HOST": DOCKER_HOST,
    "HOME": "",
    "LC_ALL": "C",
    "PATH": "/usr/bin:/bin",
    "TZ": "UTC",
}


Reader = Callable[[str], bytes]
Runner = Callable[[tuple[str, ...]], bytes]
Hasher = Callable[[str], str]
Uname = Callable[[], Any]
PhaseHook = Callable[[], None]


@dataclass(frozen=True)
class CommandCapture:
    """Bounded stdout and stderr from one completed trusted command."""

    stdout: bytes
    stderr: bytes

    def __post_init__(self) -> None:
        for label, value in (("stdout", self.stdout), ("stderr", self.stderr)):
            if type(value) is not bytes:
                raise NativeHostError(f"trusted command {label} capture is not bytes")
            if len(value) > _MAX_COMMAND_OUTPUT:
                raise NativeHostError(f"trusted command {label} capture exceeds the fixed limit")


def _error(message: str) -> None:
    raise NativeHostError(message)


def _fixed_open_flags(*, directory: bool = False) -> int:
    required = ["O_CLOEXEC", "O_NOFOLLOW", "O_NONBLOCK"]
    if directory:
        required.append("O_DIRECTORY")
    if any(not hasattr(os, name) for name in required):
        _error("native host lacks the required no-follow open flags")
    flags = os.O_RDONLY
    for name in required:
        flags |= getattr(os, name)
    return flags


def _validate_argv(argv: Sequence[str]) -> tuple[str, ...]:
    if not isinstance(argv, (tuple, list)) or not argv:
        _error("trusted command argv must be non-empty")
    result = tuple(argv)
    for index, value in enumerate(result):
        if not isinstance(value, str) or not value or "\x00" in value:
            _error(f"trusted command argument {index} is invalid")
        if any(ord(character) < 0x20 or ord(character) > 0x7E for character in value):
            _error(f"trusted command argument {index} is not printable ASCII")
    if not result[0].startswith("/"):
        _error("trusted command must use an absolute executable")
    return result


def _supports_nonreaping_wait() -> bool:
    return all(
        hasattr(os, name)
        for name in ("waitid", "P_PID", "WEXITED", "WNOHANG", "WNOWAIT", "CLD_EXITED")
    )


def _signal_process_group(process: subprocess.Popen[bytes], signum: int) -> None:
    try:
        os.killpg(process.pid, signum)
    except OSError as exc:
        if exc.errno != errno.ESRCH:
            raise NativeHostError("trusted command process-group signal failed") from exc


def _stop_process(process: subprocess.Popen[bytes]) -> None:
    """Terminate and reap an owned command process group best-effort."""

    cleanup_error: NativeHostError | None = None
    try:
        _signal_process_group(process, signal.SIGTERM)
    except NativeHostError as exc:
        cleanup_error = exc
    if _supports_nonreaping_wait():
        try:
            _waitid_without_reap(process, 0.5)
        except NativeHostError as exc:
            cleanup_error = cleanup_error or exc
    try:
        _signal_process_group(process, signal.SIGKILL)
    except NativeHostError as exc:
        cleanup_error = cleanup_error or exc
    try:
        process.wait(timeout=0.5)
    except subprocess.TimeoutExpired:
        cleanup_error = cleanup_error or NativeHostError(
            "trusted command process group could not be reaped"
        )
    if cleanup_error is not None:
        raise cleanup_error


def _wait_without_reap(process: subprocess.Popen[bytes], timeout: float) -> int:
    """Observe a direct-child exit while retaining its PID for group cleanup."""

    result = _waitid_without_reap(process, timeout)
    if result is None:
        raise NativeHostError("trusted command did not exit after closing output")
    return result


def _waitid_without_reap(process: subprocess.Popen[bytes], timeout: float) -> int | None:
    """Return a direct-child status without releasing its PID for reuse."""

    if not _supports_nonreaping_wait():
        _error("trusted command requires non-reaping child wait support")
    deadline = time.monotonic() + timeout
    while True:
        try:
            result = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
        except ChildProcessError as exc:
            raise NativeHostError("trusted command child was reaped unexpectedly") from exc
        if result is not None and result.si_pid != 0:
            if result.si_code == os.CLD_EXITED:
                return result.si_status
            return -result.si_status
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return None
        time.sleep(min(0.01, remaining))


def _run_command_captured(
    argv: Sequence[str],
    *,
    timeout_seconds: float = _COMMAND_TIMEOUT_SECONDS,
    output_limit: int = _MAX_COMMAND_OUTPUT,
    stdout_limit: int | None = None,
    stderr_limit: int | None = None,
    executable_fd: int | None = None,
    extra_fds: Sequence[int] = (),
    stdin_fd: int | None = None,
) -> CommandCapture:
    """Run one fixed command with an empty environment and bounded output."""

    command = _validate_argv(argv)
    try:
        timeout = float(timeout_seconds)
    except (OverflowError, TypeError, ValueError):
        _error("trusted command timeout must be finite and positive")
    if type(timeout_seconds) not in (int, float) or not math.isfinite(timeout) or timeout <= 0:
        _error("trusted command timeout must be positive")
    if type(output_limit) is not int or output_limit <= 0:
        _error("trusted command output limit must be positive")
    stdout_limit = output_limit if stdout_limit is None else stdout_limit
    stderr_limit = output_limit if stderr_limit is None else stderr_limit
    if type(stdout_limit) is not int or stdout_limit <= 0:
        _error("trusted command stdout limit must be positive")
    if type(stderr_limit) is not int or stderr_limit <= 0:
        _error("trusted command stderr limit must be positive")
    if executable_fd is not None and (type(executable_fd) is not int or executable_fd < 0):
        _error("trusted executable descriptor is invalid")
    if not isinstance(extra_fds, (tuple, list)):
        _error("trusted inherited descriptors must be a sequence")
    inherited_fds = tuple(extra_fds)
    if any(type(fd) is not int or fd < 0 for fd in inherited_fds):
        _error("trusted inherited descriptor is invalid")
    if len(set(inherited_fds)) != len(inherited_fds):
        _error("trusted inherited descriptors must be unique")
    if executable_fd is not None and executable_fd in inherited_fds:
        _error("trusted executable descriptor is duplicated")
    if stdin_fd is not None and (type(stdin_fd) is not int or stdin_fd < 0):
        _error("trusted stdin descriptor is invalid")
    if stdin_fd is not None and (
        stdin_fd == executable_fd or stdin_fd in inherited_fds
    ):
        _error("trusted stdin descriptor is duplicated")
    command_to_run = command
    pass_fds: tuple[int, ...] = ()
    if executable_fd is not None:
        command_to_run = (f"/proc/self/fd/{executable_fd}", *command[1:])
        pass_fds = (executable_fd, *inherited_fds)
    elif inherited_fds:
        pass_fds = inherited_fds

    try:
        process = subprocess.Popen(
            command_to_run,
            stdin=subprocess.DEVNULL if stdin_fd is None else stdin_fd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=dict(_FIXED_ENV),
            close_fds=True,
            pass_fds=pass_fds,
            start_new_session=True,
            shell=False,
            text=False,
            bufsize=0,
        )
    except OSError as exc:
        raise NativeHostError(f"cannot start trusted command {command[0]}") from exc

    selector = None
    streams = (process.stdout, process.stderr)
    buffers: dict[int, bytearray] = {}
    stdout_fd: int | None = None
    stderr_fd: int | None = None
    cleanup_done = False
    try:
        assert process.stdout is not None
        assert process.stderr is not None
        stdout_fd = process.stdout.fileno()
        stderr_fd = process.stderr.fileno()
        selector = selectors.DefaultSelector()
        buffers = {stream.fileno(): bytearray() for stream in streams if stream is not None}
        for stream in streams:
            assert stream is not None
            os.set_blocking(stream.fileno(), False)
            selector.register(stream, selectors.EVENT_READ)
        deadline = time.monotonic() + timeout
        while selector.get_map():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                _error("trusted command timed out")
            events = selector.select(remaining)
            if not events:
                _error("trusted command timed out")
            for key, _mask in events:
                fd = key.fileobj.fileno()
                try:
                    chunk = os.read(fd, 64 * 1024)
                except BlockingIOError:
                    continue
                except OSError as exc:
                    raise NativeHostError("trusted command output read failed") from exc
                if not chunk:
                    selector.unregister(key.fileobj)
                    key.fileobj.close()
                    continue
                buffer = buffers[fd]
                stream_limit = stdout_limit if fd == stdout_fd else stderr_limit
                if len(buffer) + len(chunk) > stream_limit:
                    _error("trusted command output exceeded the fixed limit")
                buffer.extend(chunk)

        return_code = _wait_without_reap(process, max(0.0, deadline - time.monotonic()))
        if return_code != 0:
            _error("trusted command exited nonzero")
        assert stdout_fd is not None and stderr_fd is not None
        return CommandCapture(bytes(buffers[stdout_fd]), bytes(buffers[stderr_fd]))
    except BaseException:
        cleanup_done = True
        _stop_process(process)
        raise
    finally:
        if not cleanup_done:
            cleanup_done = True
            _stop_process(process)
        if selector is not None:
            selector.close()
        for stream in streams:
            if stream is None:
                continue
            try:
                stream.close()
            except OSError:
                pass


def _run_command(
    argv: Sequence[str],
    *,
    timeout_seconds: float = _COMMAND_TIMEOUT_SECONDS,
    output_limit: int = _MAX_COMMAND_OUTPUT,
    executable_fd: int | None = None,
    extra_fds: Sequence[int] = (),
    stdin_fd: int | None = None,
) -> bytes:
    """Run one fixed command and return stdout for legacy native-host callers."""

    return _run_command_captured(
        argv,
        timeout_seconds=timeout_seconds,
        output_limit=output_limit,
        executable_fd=executable_fd,
        extra_fds=extra_fds,
        stdin_fd=stdin_fd,
    ).stdout


def run_command_captured(
    argv: Sequence[str],
    *,
    timeout_seconds: float = _COMMAND_TIMEOUT_SECONDS,
    output_limit: int = _MAX_COMMAND_OUTPUT,
    stdout_limit: int | None = None,
    stderr_limit: int | None = None,
) -> CommandCapture:
    """Run one fixed path command and retain both bounded output streams."""

    return _run_command_captured(
        argv,
        timeout_seconds=timeout_seconds,
        output_limit=output_limit,
        stdout_limit=stdout_limit,
        stderr_limit=stderr_limit,
    )


def run_command(
    argv: Sequence[str],
    *,
    timeout_seconds: float = _COMMAND_TIMEOUT_SECONDS,
    output_limit: int = _MAX_COMMAND_OUTPUT,
) -> bytes:
    """Run one fixed path command without binding it to a retained descriptor."""

    return _run_command(argv, timeout_seconds=timeout_seconds, output_limit=output_limit)


def _validate_source_metadata(metadata: Any, path: str, *, require_trusted: bool) -> None:
    if not stat.S_ISREG(metadata.st_mode):
        _error(f"native source is not a regular file: {path}")
    if require_trusted and (metadata.st_uid != 0 or metadata.st_mode & 0o022):
        _error(f"native source is not root-owned and benchmark-account unwritable: {path}")


def _read_no_follow(path: str, *, limit: int, require_trusted: bool) -> bytes:
    """Read one fixed regular file without following its final component."""

    if not isinstance(path, str) or not path.startswith("/") or "\x00" in path:
        _error("native source path is invalid")
    if type(limit) is not int or limit <= 0:
        _error("native source limit must be positive")
    flags = _fixed_open_flags()
    try:
        fd = os.open(path, flags)
    except OSError as exc:
        if exc.errno == errno.ENOENT:
            raise NativeHostSourceMissing(f"cannot open native source {path}") from exc
        raise NativeHostError(f"cannot open native source {path}") from exc
    try:
        metadata = os.fstat(fd)
        _validate_source_metadata(metadata, path, require_trusted=require_trusted)
        chunks: list[bytes] = []
        size = 0
        while True:
            chunk = os.read(fd, min(64 * 1024, limit - size + 1))
            if not chunk:
                break
            size += len(chunk)
            if size > limit:
                _error(f"native source exceeded the fixed limit: {path}")
            chunks.append(chunk)
        return b"".join(chunks)
    except OSError as exc:
        raise NativeHostError(f"native source read failed: {path}") from exc
    finally:
        try:
            os.close(fd)
        except OSError as exc:
            raise NativeHostError(f"native source close failed: {path}") from exc


def read_no_follow(path: str, *, limit: int = _MAX_SOURCE_BYTES) -> bytes:
    return _read_no_follow(path, limit=limit, require_trusted=False)


def read_trusted_no_follow(path: str, *, limit: int = _MAX_SOURCE_BYTES) -> bytes:
    """Read a root-owned, benchmark-account-unwritable fixed source."""

    return _read_no_follow(path, limit=limit, require_trusted=True)


def _validate_executable_metadata(metadata: Any, path: str) -> None:
    if not stat.S_ISREG(metadata.st_mode) or not metadata.st_mode & 0o111:
        _error(f"native executable is not a regular executable: {path}")
    if metadata.st_uid != 0 or metadata.st_mode & 0o022:
        _error(f"native executable is not root-owned and benchmark-account unwritable: {path}")


def _open_executable(path: str) -> int:
    if not isinstance(path, str) or not path.startswith("/") or "\x00" in path:
        _error("native executable path is invalid")
    flags = _fixed_open_flags()
    try:
        fd = os.open(path, flags)
    except OSError as exc:
        if exc.errno == errno.ENOENT:
            raise NativeHostSourceMissing(f"cannot open native executable {path}") from exc
        raise NativeHostError(f"cannot open native executable {path}") from exc
    try:
        metadata = os.fstat(fd)
        _validate_executable_metadata(metadata, path)
        return fd
    except OSError as exc:
        try:
            os.close(fd)
        except OSError:
            pass
        raise NativeHostError(f"native executable stat failed: {path}") from exc
    except BaseException:
        try:
            os.close(fd)
        except OSError:
            pass
        raise


def _hash_fd(fd: int, path: str, *, limit: int = 128 << 20) -> str:
    if type(limit) is not int or limit <= 0:
        _error("native executable limit must be positive")
    try:
        os.lseek(fd, 0, os.SEEK_SET)
        digest = hashlib.sha256()
        size = 0
        while True:
            chunk = os.read(fd, min(64 * 1024, limit - size + 1))
            if not chunk:
                break
            size += len(chunk)
            if size > limit:
                _error(f"native executable exceeded the fixed limit: {path}")
            digest.update(chunk)
        os.lseek(fd, 0, os.SEEK_SET)
    except OSError as exc:
        raise NativeHostError(f"native executable read failed: {path}") from exc
    result = digest.hexdigest()
    if _HEX64.fullmatch(result) is None:
        _error("native executable digest is malformed")
    return result


def hash_executable(path: str = DOCKER) -> str:
    """Hash one regular executable through a no-follow descriptor."""

    fd = _open_executable(path)
    try:
        return _hash_fd(fd, path)
    finally:
        try:
            os.close(fd)
        except OSError as exc:
            raise NativeHostError(f"native executable close failed: {path}") from exc


def _validate_docker_config_dir(
    path: str = DOCKER_CONFIG,
    *,
    opener: Callable[..., int] = os.open,
    statter: Callable[[int], os.stat_result] = os.fstat,
    lister: Callable[[int], list[str]] = os.listdir,
    closer: Callable[[int], None] = os.close,
) -> None:
    """Validate the fixed Docker config directory without following a path component."""

    if not isinstance(path, str) or not path.startswith("/") or "\x00" in path:
        _error("Docker config directory path is invalid")
    components = path.split("/")
    if len(components) < 2 or components[0] != "" or any(
        not component or component in (".", "..") for component in components[1:]
    ):
        _error("Docker config directory path is not canonical")
    flags = _fixed_open_flags(directory=True)
    descriptors: list[int] = []
    try:
        current = opener("/", flags)
        descriptors.append(current)
        for component in components[1:]:
            metadata = statter(current)
            if (
                not stat.S_ISDIR(metadata.st_mode)
                or metadata.st_uid != 0
                or metadata.st_mode & 0o022
            ):
                _error("Docker config directory has an untrusted parent")
            current = opener(component, flags, dir_fd=current)
            descriptors.append(current)
        metadata = statter(descriptors[-1])
        if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_mode & 0o022:
            _error("Docker config directory is not root-owned and benchmark-account unwritable")
        try:
            entries = lister(descriptors[-1])
        except OSError as exc:
            raise NativeHostError("Docker config directory cannot be listed") from exc
        if entries:
            _error("Docker config directory is not empty")
    except OSError as exc:
        raise NativeHostError("cannot validate Docker config directory") from exc
    finally:
        close_error: OSError | None = None
        for descriptor in reversed(descriptors):
            try:
                closer(descriptor)
            except OSError as exc:
                close_error = close_error or exc
        if close_error is not None:
            raise NativeHostError("Docker config directory close failed") from close_error


def _client_hash(value: object, label: str = "Docker client digest") -> str:
    if not isinstance(value, str) or _HEX64.fullmatch(value) is None:
        _error(f"{label} is not lowercase SHA-256")
    return value


def run_docker_pair(
    expected_client_hash: str | None = None,
    *,
    between: PhaseHook | None = None,
) -> tuple[bytes, bytes, str]:
    """Run version and info from the same retained Docker executable descriptor."""

    outputs, client_hash = run_docker_commands(
        (DOCKER_VERSION_ARGV, DOCKER_INFO_ARGV),
        expected_client_hash,
        between=between,
    )
    version, info = outputs
    return version, info, client_hash


def run_pinned_commands(
    executable: str,
    commands: Sequence[Sequence[str]],
    expected_executable_hash: str | None = None,
    *,
    between: PhaseHook | None = None,
    extra_fds: Sequence[int] = (),
    stdin_fd: int | None = None,
) -> tuple[tuple[bytes, ...], str]:
    """Run fixed commands through one retained executable descriptor.

    The returned bytes stay bounded by :func:`_run_command`.  ``between`` runs
    only between commands, after the preceding process has been fully reaped.
    """

    _validate_argv((executable,))
    if expected_executable_hash is not None:
        expected_executable_hash = _client_hash(
            expected_executable_hash,
            "profile executable digest",
        )
    if not isinstance(commands, (tuple, list)) or not commands:
        _error("pinned command sequence must be non-empty")
    command_sequence = tuple(_validate_argv(command) for command in commands)
    if any(command[0] != executable for command in command_sequence):
        _error("pinned command sequence must use the selected executable")
    if not isinstance(extra_fds, (tuple, list)):
        _error("pinned inherited descriptors must be a sequence")
    inherited_fds = tuple(extra_fds)
    if any(type(fd) is not int or fd < 0 for fd in inherited_fds):
        _error("pinned inherited descriptor is invalid")
    if len(set(inherited_fds)) != len(inherited_fds):
        _error("pinned inherited descriptors must be unique")
    if stdin_fd is not None and (type(stdin_fd) is not int or stdin_fd < 0):
        _error("pinned stdin descriptor is invalid")
    if stdin_fd is not None and stdin_fd in inherited_fds:
        _error("pinned stdin descriptor is duplicated")
    fd = _open_executable(executable)
    try:
        executable_hash = _hash_fd(fd, executable)
        if expected_executable_hash is not None and executable_hash != expected_executable_hash:
            _error("pinned executable digest does not match profile before execution")
        outputs: list[bytes] = []
        for index, command in enumerate(command_sequence):
            run_kwargs: dict[str, Any] = {"executable_fd": fd}
            if inherited_fds:
                run_kwargs["extra_fds"] = inherited_fds
            if stdin_fd is not None:
                run_kwargs["stdin_fd"] = stdin_fd
            outputs.append(_run_command(command, **run_kwargs))
            if between is not None and index + 1 < len(command_sequence):
                between()
        return tuple(outputs), executable_hash
    finally:
        try:
            os.close(fd)
        except OSError as exc:
            raise NativeHostError(f"native executable close failed: {executable}") from exc


def run_pinned_commands_captured(
    executable: str,
    commands: Sequence[Sequence[str]],
    expected_executable_hash: str | None = None,
    *,
    between: PhaseHook | None = None,
    extra_fds: Sequence[int] = (),
    stdin_fd: int | None = None,
    timeout_seconds: float = _COMMAND_TIMEOUT_SECONDS,
    output_limit: int = _MAX_COMMAND_OUTPUT,
    stdout_limit: int | None = None,
    stderr_limit: int | None = None,
) -> tuple[tuple[CommandCapture, ...], str]:
    """Run fixed commands through one retained executable and retain both streams."""

    _validate_argv((executable,))
    if expected_executable_hash is not None:
        expected_executable_hash = _client_hash(
            expected_executable_hash,
            "profile executable digest",
        )
    if not isinstance(commands, (tuple, list)) or not commands:
        _error("pinned command sequence must be non-empty")
    command_sequence = tuple(_validate_argv(command) for command in commands)
    if any(command[0] != executable for command in command_sequence):
        _error("pinned command sequence must use the selected executable")
    if not isinstance(extra_fds, (tuple, list)):
        _error("pinned inherited descriptors must be a sequence")
    inherited_fds = tuple(extra_fds)
    if any(type(fd) is not int or fd < 0 for fd in inherited_fds):
        _error("pinned inherited descriptor is invalid")
    if len(set(inherited_fds)) != len(inherited_fds):
        _error("pinned inherited descriptors must be unique")
    if stdin_fd is not None and (type(stdin_fd) is not int or stdin_fd < 0):
        _error("pinned stdin descriptor is invalid")
    if stdin_fd is not None and stdin_fd in inherited_fds:
        _error("pinned stdin descriptor is duplicated")
    fd = _open_executable(executable)
    try:
        executable_hash = _hash_fd(fd, executable)
        if expected_executable_hash is not None and executable_hash != expected_executable_hash:
            _error("pinned executable digest does not match profile before execution")
        outputs: list[CommandCapture] = []
        for index, command in enumerate(command_sequence):
            outputs.append(
                _run_command_captured(
                    command,
                    executable_fd=fd,
                    extra_fds=inherited_fds,
                    stdin_fd=stdin_fd,
                    timeout_seconds=timeout_seconds,
                    output_limit=output_limit,
                    stdout_limit=stdout_limit,
                    stderr_limit=stderr_limit,
                )
            )
            if between is not None and index + 1 < len(command_sequence):
                between()
        return tuple(outputs), executable_hash
    finally:
        try:
            os.close(fd)
        except OSError as exc:
            raise NativeHostError(f"native executable close failed: {executable}") from exc


def run_docker_commands(
    commands: Sequence[Sequence[str]],
    expected_client_hash: str | None = None,
    *,
    between: PhaseHook | None = None,
) -> tuple[tuple[bytes, ...], str]:
    """Run fixed Docker commands through one retained executable descriptor."""

    _validate_docker_config_dir()
    return run_pinned_commands(
        DOCKER,
        commands,
        expected_client_hash,
        between=between,
    )


def run_docker_commands_captured(
    commands: Sequence[Sequence[str]],
    expected_client_hash: str | None = None,
    *,
    between: PhaseHook | None = None,
    timeout_seconds: float = _COMMAND_TIMEOUT_SECONDS,
    output_limit: int = _MAX_COMMAND_OUTPUT,
    stdout_limit: int | None = None,
    stderr_limit: int | None = None,
) -> tuple[tuple[CommandCapture, ...], str]:
    """Run fixed Docker commands through one retained client with both streams."""

    _validate_docker_config_dir()
    return run_pinned_commands_captured(
        DOCKER,
        commands,
        expected_client_hash,
        between=between,
        timeout_seconds=timeout_seconds,
        output_limit=output_limit,
        stdout_limit=stdout_limit,
        stderr_limit=stderr_limit,
    )


def _text(reader: Reader, path: str) -> str:
    try:
        raw = reader(path)
    except NativeHostError:
        raise
    except BaseException as exc:
        raise NativeHostError(f"native source reader failed: {path}") from exc
    if not isinstance(raw, bytes) or len(raw) > _MAX_SOURCE_BYTES:
        _error(f"native source has an invalid bounded byte value: {path}")
    try:
        value = raw.decode("ascii")
    except UnicodeDecodeError as exc:
        raise NativeHostError(f"native source is not ASCII: {path}") from exc
    if "\x00" in value:
        _error(f"native source contains NUL: {path}")
    return value


def _line(reader: Reader, path: str) -> str:
    value = _text(reader, path)
    if value.endswith("\n"):
        value = value[:-1]
    if "\n" in value or not value or value != value.strip():
        _error(f"native source is not one canonical line: {path}")
    return value


def _uint(value: object, label: str, maximum: int = _U64_MAX) -> int:
    if type(value) is not int or value < 0 or value > maximum:
        _error(f"{label} is not an unsigned integer in range")
    return value


def _name(value: object, label: str) -> str:
    if not isinstance(value, str) or _ASCII_NAME.fullmatch(value) is None:
        _error(f"{label} has invalid name grammar")
    return value


def _profile_field(profile: Mapping[str, Any], path: tuple[str, ...]) -> Any:
    value: Any = profile
    for component in path:
        if not isinstance(value, Mapping) or component not in value:
            _error(f"profile is missing {'.'.join(path)}")
        value = value[component]
    return value


def _json(raw: bytes, label: str) -> cj.Object:
    if not isinstance(raw, bytes) or len(raw) > _MAX_COMMAND_OUTPUT:
        _error(f"{label} has an invalid bounded byte value")

    def object_pairs(pairs: list[tuple[str, Any]]) -> cj.Object:
        keys = [key for key, _member in pairs]
        if len(keys) != len(set(keys)):
            raise ValueError("duplicate object member")
        return cj.Object(pairs)

    try:
        value = json.loads(
            raw.decode("utf-8"),
            object_pairs_hook=object_pairs,
            parse_float=lambda _value: (_ for _ in ()).throw(ValueError("float")),
            parse_constant=lambda _value: (_ for _ in ()).throw(ValueError("constant")),
        )
    except (UnicodeDecodeError, ValueError, json.JSONDecodeError) as exc:
        raise NativeHostError(f"{label} is not valid JSON: {exc}") from exc
    if not isinstance(value, cj.Object):
        _error(f"{label} is not a JSON object")
    return value


def _object(value: object, label: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        _error(f"{label} is not an object")
    return value


def _json_string(value: Mapping[str, Any], key: str, label: str) -> str:
    member = value.get(key)
    if not isinstance(member, str) or not member or "\x00" in member:
        _error(f"{label}.{key} is not a nonempty string")
    return member


def _architecture(value: str, label: str) -> str:
    normalized = {"amd64": "x86_64", "x86_64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}.get(value)
    if normalized is None:
        _error(f"{label} has an unknown architecture")
    return normalized


def _parse_cpu_set(value: str, label: str) -> tuple[tuple[int, int], ...]:
    if _CPU_SET.fullmatch(value) is None:
        _error(f"{label} is not a canonical CPU set")
    intervals: list[tuple[int, int]] = []
    for item in value.split(","):
        bounds = item.split("-")
        start = int(bounds[0])
        end = int(bounds[-1])
        if len(bounds) == 2 and start == end:
            _error(f"{label} contains a non-canonical singleton range")
        if end < start or (intervals and start <= intervals[-1][1] + 1):
            _error(f"{label} is unsorted, overlapping, or not coalesced")
        intervals.append((start, end))
    return tuple(intervals)


def _subset(subset: tuple[tuple[int, int], ...], superset: tuple[tuple[int, int], ...]) -> bool:
    for start, end in subset:
        if not any(parent_start <= start and end <= parent_end for parent_start, parent_end in superset):
            return False
    return True


def _cpuinfo(value: str) -> dict[int, dict[str, str]]:
    records = value.split("\n\n")
    if records and not records[-1].strip():
        records.pop()
    if not records or any(not record.strip() for record in records):
        _error("/proc/cpuinfo has an empty processor record")
    result: dict[int, dict[str, str]] = {}
    required = ("processor", "vendor_id", "cpu family", "model", "stepping", "microcode")
    for record in records:
        fields: dict[str, str] = {}
        for line in record.splitlines():
            if ":" not in line:
                continue
            key, member = line.split(":", 1)
            key = key.strip()
            member = member.strip()
            if not key or not member or key in fields:
                _error("/proc/cpuinfo has a missing or repeated field")
            fields[key] = member
        if any(key not in fields for key in required):
            _error("/proc/cpuinfo is missing a required identity field")
        processor = _decimal_uint(fields["processor"], "/proc/cpuinfo processor")
        if processor in result:
            _error("/proc/cpuinfo has a repeated processor ID")
        result[processor] = fields
    return result


def _cpu_set_count(ranges: tuple[tuple[int, int], ...], label: str) -> int:
    total = 0
    for start, end in ranges:
        total = _uint(total + end - start + 1, label)
    return total


def _cpu_set_contains(ranges: tuple[tuple[int, int], ...], value: int) -> bool:
    return any(start <= value <= end for start, end in ranges)


def _selected_cpu_fields(
    records: Mapping[int, Mapping[str, str]],
    benchmark_ranges: tuple[tuple[int, int], ...],
    expected_machine: Mapping[str, Any],
) -> Mapping[str, str]:
    expected_vendor = _name(expected_machine.get("cpu_vendor"), "profile.machine.cpu_vendor")
    expected_family = _uint(expected_machine.get("cpu_family"), "profile.machine.cpu_family", 1_000_000)
    expected_model = _name(expected_machine.get("cpu_model"), "profile.machine.cpu_model")
    expected_stepping = _uint(
        expected_machine.get("cpu_stepping"), "profile.machine.cpu_stepping", 1_000_000
    )
    expected_microcode = _name(expected_machine.get("microcode"), "profile.machine.microcode")
    selected = [
        (processor, fields)
        for processor, fields in sorted(records.items())
        if _cpu_set_contains(benchmark_ranges, processor)
    ]
    expected_count = _cpu_set_count(benchmark_ranges, "benchmark CPU set size")
    if len(selected) != expected_count:
        _error("/proc/cpuinfo is missing a selected benchmark CPU record")
    for processor, fields in selected:
        vendor = _name(fields["vendor_id"], f"CPU {processor} vendor")
        family = _decimal_uint(fields["cpu family"], f"CPU {processor} family")
        model = _name(fields["model"], f"CPU {processor} model")
        stepping = _decimal_uint(fields["stepping"], f"CPU {processor} stepping")
        microcode = _name(fields["microcode"], f"CPU {processor} microcode")
        if (vendor, family, model, stepping, microcode) != (
            expected_vendor,
            expected_family,
            expected_model,
            expected_stepping,
            expected_microcode,
        ):
            _error(f"CPU {processor} identity does not match the profile")
    return selected[0][1]


def _decimal_uint(value: str, label: str) -> int:
    if not value or not value.isascii() or not value.isdecimal():
        _error(f"{label} is not an unsigned decimal")
    return _uint(int(value), label)


def _load_milli(value: str) -> int:
    token = value.split()[0] if value.split() else ""
    parts = token.split(".")
    if len(parts) > 2 or not parts[0].isdigit() or (len(parts) == 2 and not parts[1].isdigit()):
        _error("/proc/loadavg has invalid load grammar")
    fraction = parts[1] if len(parts) == 2 else ""
    if len(fraction) > 3:
        _error("/proc/loadavg has excessive load precision")
    return _uint(int(parts[0]) * 1000 + int(fraction.ljust(3, "0") or "0"), "load milli")


def _pressure_total(value: str, label: str) -> int:
    found = False
    total: str | None = None
    for line in value.splitlines():
        parts = line.split()
        if parts and parts[0] == "some":
            if found:
                _error(f"{label} has repeated PSI some lines")
            found = True
            for member in parts[1:]:
                if member.startswith("total="):
                    if total is not None:
                        _error(f"{label} has repeated PSI totals")
                    total = member[6:]
    if total is not None:
        return _decimal_uint(total, label)
    _error(f"{label} has no PSI some total")


def _keyed_lines(value: str, label: str) -> dict[str, tuple[str, ...]]:
    result: dict[str, tuple[str, ...]] = {}
    for line in value.splitlines():
        if not line.strip():
            continue
        parts = tuple(line.split())
        if not parts or parts[0] in result:
            _error(f"{label} has a missing or repeated field")
        result[parts[0]] = parts[1:]
    return result


def _memory_snapshot(reader: Reader) -> tuple[int, int]:
    fields = _keyed_lines(_text(reader, MEMINFO_PATH), MEMINFO_PATH)
    member = fields.get("MemTotal:")
    available = fields.get("MemAvailable:")
    if member is None or available is None or len(member) != 2 or len(available) != 2:
        _error("/proc/meminfo lacks bounded kB memory fields")
    if member[1] != "kB" or available[1] != "kB":
        _error("/proc/meminfo memory units are not kB")
    total = _uint(_decimal_uint(member[0], "MemTotal") * 1024, "MemTotal bytes")
    available = _uint(_decimal_uint(available[0], "MemAvailable") * 1024, "MemAvailable bytes")
    if available > total:
        _error("MemAvailable exceeds MemTotal")
    return total, available


def _memory_bytes(reader: Reader) -> int:
    return _memory_snapshot(reader)[0]


def _swap_bytes(reader: Reader, page_size: int) -> tuple[int, int]:
    page_size = _uint(page_size, "page size", 1 << 20)
    if page_size == 0:
        _error("page size is zero")
    fields = _keyed_lines(_text(reader, VMSTAT_PATH), VMSTAT_PATH)
    read_pages = fields.get("pswpin")
    write_pages = fields.get("pswpout")
    if read_pages is None or write_pages is None or len(read_pages) != 1 or len(write_pages) != 1:
        _error("/proc/vmstat lacks swap counters")
    return (
        _uint(_decimal_uint(read_pages[0], "pswpin") * page_size, "swap read bytes"),
        _uint(_decimal_uint(write_pages[0], "pswpout") * page_size, "swap write bytes"),
    )


def _quota_milli(reader: Reader) -> int:
    try:
        value = _line(reader, CGROUP_V2_CPU_MAX_PATH)
        fields = value.split()
        if len(fields) != 2:
            _error("cgroup v2 cpu.max has the wrong shape")
        period = _decimal_uint(fields[1], "cgroup v2 period")
        if period == 0:
            _error("CPU quota period is zero")
        if fields[0] == "max":
            return 0
        quota = _decimal_uint(fields[0], "cgroup v2 quota")
    except NativeHostSourceMissing:
        quota_text = _line(reader, CGROUP_V1_CPU_QUOTA_PATH)
        period = _decimal_uint(_line(reader, CGROUP_V1_CPU_PERIOD_PATH), "cgroup v1 period")
        if period == 0:
            _error("CPU quota period is zero")
        if quota_text == "-1":
            return 0
        quota = _decimal_uint(quota_text, "cgroup v1 quota")
    if quota == 0:
        _error("CPU quota is zero")
    return _uint((quota * 1000 + period - 1) // period, "CPU quota milli")


def _docker(
    profile: Mapping[str, Any],
    reader: Reader,
    runner: Runner,
    hasher: Hasher,
    expected_client_hash: str,
    *,
    between: PhaseHook | None = None,
) -> cj.Object:
    expected_docker = _object(_profile_field(profile, ("docker",)), "profile.docker")
    expected_cgroup_driver = _name(
        expected_docker.get("cgroup_driver"), "profile.docker.cgroup_driver"
    )
    expected_cgroup_parent = _name(
        expected_docker.get("cgroup_parent"), "profile.docker.cgroup_parent"
    )
    expected_parent = {"cgroupfs": "/", "systemd": "-.slice"}.get(expected_cgroup_driver)
    if expected_parent is None:
        _error("profile.docker.cgroup_driver must be cgroupfs or systemd")
    if expected_cgroup_parent != expected_parent:
        _error("profile.docker.cgroup_parent does not match the cgroup driver")
    if runner is run_command and hasher is hash_executable:
        version_raw, info_raw, client_hash = run_docker_pair(
            expected_client_hash, between=between
        )
    else:
        client_hash = _client_hash(hasher(DOCKER))
        if client_hash != expected_client_hash:
            _error("Docker client digest does not match profile before Docker execution")
        version_raw = runner(DOCKER_VERSION_ARGV)
        if between is not None:
            between()
        info_raw = runner(DOCKER_INFO_ARGV)
    version = _json(version_raw, "docker version output")
    client = _object(version.get("Client"), "docker version Client")
    info = _json(info_raw, "docker info output")
    daemon_version = _json_string(info, "ServerVersion", "docker info")
    daemon_architecture = _architecture(
        _json_string(info, "Architecture", "docker info"), "docker daemon architecture"
    )
    cgroup_driver = _json_string(info, "CgroupDriver", "docker info")
    if cgroup_driver != expected_cgroup_driver:
        _error("Docker cgroup driver does not match the profile")
    default_runtime = _json_string(info, "DefaultRuntime", "docker info")
    if default_runtime != DOCKER_RUNTIME:
        _error("Docker default runtime is not the profiled runc runtime")
    runtimes = info.get("Runtimes")
    if not isinstance(runtimes, Mapping) or not isinstance(
        runtimes.get(DOCKER_RUNTIME), Mapping
    ):
        _error("Docker info does not register the profiled runc runtime")
    runtime_commit = _object(info.get("RuncCommit"), "docker info RuncCommit")
    return cj.Object(
        (
            ("client_version", _json_string(client, "Version", "docker version Client")),
            ("client_sha256", client_hash),
            ("daemon_version", daemon_version),
            ("daemon_architecture", daemon_architecture),
            ("storage_driver", _json_string(info, "Driver", "docker info")),
            ("cgroup_version", _json_string(info, "CgroupVersion", "docker info")),
            ("cgroup_driver", cgroup_driver),
            ("cgroup_parent", expected_cgroup_parent),
            ("oci_runtime", _json_string(runtime_commit, "ID", "docker info RuncCommit")),
        )
    )


def _observation(reader: Reader, phase: str, page_size: int, memory_bytes: int) -> cj.Object:
    swap_read, swap_write = _swap_bytes(reader, page_size)
    snapshot_memory, free_memory = _memory_snapshot(reader)
    if snapshot_memory != memory_bytes:
        _error("MemTotal changed during qualification")
    return cj.Object(
        (
            ("phase", phase),
            ("load_milli", _load_milli(_text(reader, LOADAVG_PATH))),
            ("cpu_pressure_total_us", _pressure_total(_text(reader, CPU_PRESSURE_PATH), "CPU pressure")),
            ("memory_pressure_total_us", _pressure_total(_text(reader, MEMORY_PRESSURE_PATH), "memory pressure")),
            ("free_memory_bytes", free_memory),
            ("swap_read_bytes", swap_read),
            ("swap_write_bytes", swap_write),
        )
    )


_MONOTONIC_COUNTERS = (
    "cpu_pressure_total_us",
    "memory_pressure_total_us",
    "swap_read_bytes",
    "swap_write_bytes",
)


def _check_monotonic_counters(observations: Sequence[Mapping[str, Any]]) -> None:
    previous: Mapping[str, Any] | None = None
    for index, observation in enumerate(observations):
        if previous is not None:
            for key in _MONOTONIC_COUNTERS:
                current = observation[key]
                prior = previous[key]
                if current < prior:
                    _error(f"{key} counter reset before observation {index}")
        previous = observation


def inspect(
    profile: Mapping[str, Any],
    *,
    reader: Reader = read_no_follow,
    trusted_reader: Reader | None = None,
    runner: Runner = run_command,
    hasher: Hasher = hash_executable,
    uname: Uname = os.uname,
    page_size: int | None = None,
) -> cj.Object:
    """Acquire one canonical host/daemon inspection from fixed native sources."""

    if not isinstance(profile, Mapping):
        _error("profile must be an object")
    if trusted_reader is None:
        trusted_reader = read_trusted_no_follow if reader is read_no_follow else reader
    expected_docker = _object(_profile_field(profile, ("docker",)), "profile.docker")
    expected_client_hash = _client_hash(
        expected_docker.get("client_sha256"), "profile Docker client digest"
    )
    system = uname()
    architecture = _architecture(getattr(system, "machine", ""), "host architecture")
    if architecture != "x86_64":
        _error("host architecture must be x86_64 before Docker qualification")
    kernel = _name(getattr(system, "release", ""), "host kernel")
    expected_machine = _object(_profile_field(profile, ("machine",)), "profile.machine")
    fields = _cpuinfo(_text(reader, CPUINFO_PATH))
    online = _line(reader, ONLINE_CPU_SET_PATH)
    benchmark = _line(trusted_reader, BENCHMARK_CPU_SET_PATH)
    numa = _line(reader, NUMA_SET_PATH)
    online_ranges = _parse_cpu_set(online, "online CPU set")
    benchmark_ranges = _parse_cpu_set(benchmark, "benchmark CPU set")
    _parse_cpu_set(numa, "NUMA set")
    if not _subset(benchmark_ranges, online_ranges):
        _error("benchmark CPU set is not a subset of online CPUs")
    expected_benchmark = expected_machine.get("benchmark_cpu_set")
    if not isinstance(expected_benchmark, str):
        _error("profile.machine.benchmark_cpu_set is not a string")
    _parse_cpu_set(expected_benchmark, "profile.machine.benchmark_cpu_set")
    if benchmark != expected_benchmark:
        _error("benchmark CPU set does not match the profile")
    selected_fields = _selected_cpu_fields(fields, benchmark_ranges, expected_machine)
    memory = _memory_bytes(reader)
    quota = _quota_milli(reader)
    if quota != 0:
        _error("CPU quota must be zero before Docker qualification")
    host_id = _line(trusted_reader, HOST_ID_PATH)
    if page_size is None:
        try:
            page_size = int(os.sysconf("SC_PAGESIZE"))
        except (AttributeError, OSError, ValueError) as exc:
            raise NativeHostError("cannot determine the native page size") from exc
    observations = [_observation(reader, "pre", page_size, memory)]

    def capture_between() -> None:
        observations.append(_observation(reader, "between", page_size, memory))

    docker = _docker(profile, reader, runner, hasher, expected_client_hash, between=capture_between)
    observations.append(_observation(reader, "post", page_size, memory))
    _check_monotonic_counters(observations)
    return cj.Object(
        (
            ("host_id", host_id),
            (
                "machine",
                cj.Object(
                    (
                        ("architecture", architecture),
                        ("kernel", kernel),
                        ("cpu_vendor", _name(selected_fields["vendor_id"], "CPU vendor")),
                        ("cpu_family", _decimal_uint(selected_fields["cpu family"], "CPU family")),
                        ("cpu_model", _name(selected_fields["model"], "CPU model")),
                        ("cpu_stepping", _decimal_uint(selected_fields["stepping"], "CPU stepping")),
                        ("microcode", _name(selected_fields["microcode"], "microcode")),
                        ("online_cpu_set", online),
                        ("benchmark_cpu_set", benchmark),
                        ("numa_set", numa),
                    )
                ),
            ),
            ("memory_bytes", memory),
            ("cpu_quota_milli", quota),
            ("docker", docker),
            ("observations", observations),
        )
    )


def qualify(
    profile: Mapping[str, Any],
    **kwargs: Any,
) -> host_contract.QualifiedHost:
    """Acquire and validate the native observation against the immutable profile."""

    try:
        return host_contract.qualify(profile, inspect(profile, **kwargs))
    except host_contract.HostQualificationError as exc:
        raise NativeHostError(f"native host qualification rejected: {exc}") from exc
