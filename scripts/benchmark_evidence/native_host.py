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


def _error(message: str) -> None:
    raise NativeHostError(message)


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


def _stop_process(process: subprocess.Popen[bytes]) -> None:
    """Terminate and reap an owned command process group best-effort."""

    try:
        os.killpg(process.pid, signal.SIGTERM)
    except OSError:
        pass
    try:
        process.wait(timeout=0.5)
    except subprocess.TimeoutExpired:
        pass
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except OSError:
        pass
    try:
        process.wait(timeout=0.5)
    except subprocess.TimeoutExpired:
        _error("trusted command process group could not be reaped")


def _run_command(
    argv: Sequence[str],
    *,
    timeout_seconds: float = _COMMAND_TIMEOUT_SECONDS,
    output_limit: int = _MAX_COMMAND_OUTPUT,
    executable_fd: int | None = None,
) -> bytes:
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
    if executable_fd is not None and (type(executable_fd) is not int or executable_fd < 0):
        _error("trusted executable descriptor is invalid")
    command_to_run = command
    pass_fds: tuple[int, ...] = ()
    if executable_fd is not None:
        command_to_run = (f"/proc/self/fd/{executable_fd}", *command[1:])
        pass_fds = (executable_fd,)

    try:
        process = subprocess.Popen(
            command_to_run,
            stdin=subprocess.DEVNULL,
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

    assert process.stdout is not None
    assert process.stderr is not None
    stdout_fd = process.stdout.fileno()
    selector = selectors.DefaultSelector()
    streams = (process.stdout, process.stderr)
    buffers = {stream.fileno(): bytearray() for stream in streams}
    cleanup_done = False
    try:
        for stream in streams:
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
                if len(buffer) + len(chunk) > output_limit:
                    _error("trusted command output exceeded the fixed limit")
                buffer.extend(chunk)

        try:
            return_code = process.wait(timeout=max(0.0, deadline - time.monotonic()))
        except subprocess.TimeoutExpired as exc:
            raise NativeHostError("trusted command did not exit after closing output") from exc
        if return_code != 0:
            _error("trusted command exited nonzero")
        return bytes(buffers[stdout_fd])
    except BaseException:
        cleanup_done = True
        _stop_process(process)
        raise
    finally:
        if not cleanup_done:
            cleanup_done = True
            _stop_process(process)
        selector.close()
        for stream in streams:
            try:
                stream.close()
            except OSError:
                pass


def run_command(
    argv: Sequence[str],
    *,
    timeout_seconds: float = _COMMAND_TIMEOUT_SECONDS,
    output_limit: int = _MAX_COMMAND_OUTPUT,
) -> bytes:
    """Run one fixed path command without binding it to a retained descriptor."""

    return _run_command(argv, timeout_seconds=timeout_seconds, output_limit=output_limit)


def read_no_follow(path: str, *, limit: int = _MAX_SOURCE_BYTES) -> bytes:
    """Read one fixed regular file without following its final component."""

    if not isinstance(path, str) or not path.startswith("/") or "\x00" in path:
        _error("native source path is invalid")
    if type(limit) is not int or limit <= 0:
        _error("native source limit must be positive")
    flags = os.O_RDONLY
    flags |= getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    try:
        fd = os.open(path, flags)
    except OSError as exc:
        if exc.errno == errno.ENOENT:
            raise NativeHostSourceMissing(f"cannot open native source {path}") from exc
        raise NativeHostError(f"cannot open native source {path}") from exc
    try:
        metadata = os.fstat(fd)
        if not stat.S_ISREG(metadata.st_mode):
            _error(f"native source is not a regular file: {path}")
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


def _open_executable(path: str) -> int:
    if not isinstance(path, str) or not path.startswith("/") or "\x00" in path:
        _error("native executable path is invalid")
    flags = os.O_RDONLY
    flags |= getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    try:
        fd = os.open(path, flags)
    except OSError as exc:
        if exc.errno == errno.ENOENT:
            raise NativeHostSourceMissing(f"cannot open native executable {path}") from exc
        raise NativeHostError(f"cannot open native executable {path}") from exc
    try:
        metadata = os.fstat(fd)
        if not stat.S_ISREG(metadata.st_mode) or not metadata.st_mode & 0o111:
            _error(f"native executable is not a regular executable: {path}")
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


def run_docker_pair() -> tuple[bytes, bytes, str]:
    """Run version and info from the same retained Docker executable descriptor."""

    fd = _open_executable(DOCKER)
    try:
        client_hash = _hash_fd(fd, DOCKER)
        version = _run_command(DOCKER_VERSION_ARGV, executable_fd=fd)
        info = _run_command(DOCKER_INFO_ARGV, executable_fd=fd)
        return version, info, client_hash
    finally:
        try:
            os.close(fd)
        except OSError as exc:
            raise NativeHostError(f"native executable close failed: {DOCKER}") from exc


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


def _cpuinfo(value: str) -> dict[str, str]:
    record = value.split("\n\n", 1)[0]
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
    required = ("vendor_id", "cpu family", "model", "stepping", "microcode")
    if any(key not in fields for key in required):
        _error("/proc/cpuinfo is missing a required identity field")
    return fields


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


def _docker(reader: Reader, runner: Runner, hasher: Hasher) -> cj.Object:
    if runner is run_command and hasher is hash_executable:
        version_raw, info_raw, client_hash = run_docker_pair()
    else:
        version_raw = runner(DOCKER_VERSION_ARGV)
        info_raw = runner(DOCKER_INFO_ARGV)
        client_hash = hasher(DOCKER)
    version = _json(version_raw, "docker version output")
    client = _object(version.get("Client"), "docker version Client")
    server = _object(version.get("Server"), "docker version Server")
    info = _json(info_raw, "docker info output")
    daemon_architecture = _architecture(_json_string(server, "Arch", "docker version Server"), "docker daemon architecture")
    if not isinstance(client_hash, str) or _HEX64.fullmatch(client_hash) is None:
        _error("Docker client digest is not lowercase SHA-256")
    return cj.Object(
        (
            ("client_version", _json_string(client, "Version", "docker version Client")),
            ("client_sha256", client_hash),
            ("daemon_version", _json_string(server, "Version", "docker version Server")),
            ("daemon_architecture", daemon_architecture),
            ("storage_driver", _json_string(info, "Driver", "docker info")),
            ("cgroup_version", _json_string(info, "CgroupVersion", "docker info")),
            ("oci_runtime", _json_string(info, "DefaultRuntime", "docker info")),
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
    runner: Runner = run_command,
    hasher: Hasher = hash_executable,
    uname: Uname = os.uname,
    page_size: int | None = None,
) -> cj.Object:
    """Acquire one canonical host/daemon inspection from fixed native sources."""

    if not isinstance(profile, Mapping):
        _error("profile must be an object")
    system = uname()
    architecture = _architecture(getattr(system, "machine", ""), "host architecture")
    if architecture != "x86_64":
        _error("host architecture must be x86_64 before Docker qualification")
    kernel = _name(getattr(system, "release", ""), "host kernel")
    fields = _cpuinfo(_text(reader, CPUINFO_PATH))
    online = _line(reader, ONLINE_CPU_SET_PATH)
    benchmark = _line(reader, BENCHMARK_CPU_SET_PATH)
    numa = _line(reader, NUMA_SET_PATH)
    online_ranges = _parse_cpu_set(online, "online CPU set")
    benchmark_ranges = _parse_cpu_set(benchmark, "benchmark CPU set")
    _parse_cpu_set(numa, "NUMA set")
    if not _subset(benchmark_ranges, online_ranges):
        _error("benchmark CPU set is not a subset of online CPUs")
    expected_machine = _object(_profile_field(profile, ("machine",)), "profile.machine")
    expected_benchmark = expected_machine.get("benchmark_cpu_set")
    if not isinstance(expected_benchmark, str):
        _error("profile.machine.benchmark_cpu_set is not a string")
    _parse_cpu_set(expected_benchmark, "profile.machine.benchmark_cpu_set")
    if benchmark != expected_benchmark:
        _error("benchmark CPU set does not match the profile")
    memory = _memory_bytes(reader)
    quota = _quota_milli(reader)
    if quota != 0:
        _error("CPU quota must be zero before Docker qualification")
    if page_size is None:
        try:
            page_size = int(os.sysconf("SC_PAGESIZE"))
        except (AttributeError, OSError, ValueError) as exc:
            raise NativeHostError("cannot determine the native page size") from exc
    observations = [_observation(reader, phase, page_size, memory) for phase in ("pre", "between", "post")]
    _check_monotonic_counters(observations)
    return cj.Object(
        (
            ("host_id", _line(reader, HOST_ID_PATH)),
            (
                "machine",
                cj.Object(
                    (
                        ("architecture", architecture),
                        ("kernel", kernel),
                        ("cpu_vendor", _name(fields["vendor_id"], "CPU vendor")),
                        ("cpu_family", _decimal_uint(fields["cpu family"], "CPU family")),
                        ("cpu_model", _name(fields["model"], "CPU model")),
                        ("cpu_stepping", _decimal_uint(fields["stepping"], "CPU stepping")),
                        ("microcode", _name(fields["microcode"], "microcode")),
                        ("online_cpu_set", online),
                        ("benchmark_cpu_set", benchmark),
                        ("numa_set", numa),
                    )
                ),
            ),
            ("memory_bytes", memory),
            ("cpu_quota_milli", quota),
            ("docker", _docker(reader, runner, hasher)),
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
