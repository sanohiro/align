"""Bind a native host and daemon observation to the immutable profile.

The controller obtains this record at its privileged host boundary.  This
module is deliberately pure: it does not read ``/proc`` or sysfs, invoke
Docker, inspect cgroups, or start the monitor.  It only rejects an already
canonical observation that cannot be the profile-pinned evidence host.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Any, Mapping

from . import canonical_json as cj


class HostQualificationError(ValueError):
    """A host or daemon observation does not satisfy the fixed profile."""


_HEX64 = re.compile(r"[0-9a-f]{64}\Z")
_NAME = re.compile(r"[A-Za-z0-9._/:+=@-]{1,255}\Z")

_INSPECTION_KEYS = (
    "host_id",
    "machine",
    "memory_bytes",
    "cpu_quota_milli",
    "docker",
    "observations",
)
_MACHINE_KEYS = (
    "architecture",
    "kernel",
    "cpu_vendor",
    "cpu_family",
    "cpu_model",
    "cpu_stepping",
    "microcode",
    "online_cpu_set",
    "benchmark_cpu_set",
    "numa_set",
)
_DOCKER_KEYS = (
    "client_version",
    "client_sha256",
    "daemon_version",
    "daemon_architecture",
    "storage_driver",
    "cgroup_version",
    "cgroup_driver",
    "cgroup_parent",
    "oci_runtime",
)
_OBSERVATION_KEYS = (
    "phase",
    "load_milli",
    "cpu_pressure_total_us",
    "memory_pressure_total_us",
    "free_memory_bytes",
    "swap_read_bytes",
    "swap_write_bytes",
)
_PHASES = ("pre", "between", "post")
_U64_MAX = (1 << 64) - 1


@dataclass(frozen=True)
class HostObservation:
    """One profile phase's bounded resource observation."""

    phase: str
    load_milli: int
    cpu_pressure_total_us: int
    memory_pressure_total_us: int
    free_memory_bytes: int
    swap_read_bytes: int
    swap_write_bytes: int


@dataclass(frozen=True)
class QualifiedDocker:
    """The profile-bound Docker client/daemon identity."""

    client_version: str
    client_sha256: str
    daemon_version: str
    daemon_architecture: str
    storage_driver: str
    cgroup_version: str
    cgroup_driver: str
    cgroup_parent: str
    oci_runtime: str


@dataclass(frozen=True)
class QualifiedHost:
    """The profile-bound host identity consumed before image/container work."""

    host_id: str
    architecture: str
    kernel: str
    cpu_vendor: str
    cpu_family: int
    cpu_model: str
    cpu_stepping: int
    microcode: str
    online_cpu_set: str
    benchmark_cpu_set: str
    numa_set: str
    memory_bytes: int
    cpu_quota_milli: int
    docker: QualifiedDocker
    observations: tuple[HostObservation, ...]


def _error(label: str, message: str) -> None:
    raise HostQualificationError(f"{label}: {message}")


def _string(value: Any, pattern: re.Pattern[str], label: str) -> str:
    if not isinstance(value, str) or pattern.fullmatch(value) is None:
        _error(label, "has an invalid grammar")
    return value


def _uint(value: Any, label: str, maximum: int = _U64_MAX) -> int:
    if type(value) is not int or value < 0 or value > maximum:
        _error(label, "is not an unsigned integer in range")
    return value


def _profile_field(profile: Mapping[str, Any], path: tuple[str, ...]) -> Any:
    value: Any = profile
    for component in path:
        if not isinstance(value, Mapping) or component not in value:
            _error("profile", f"is missing {'.'.join(path)}")
        value = value[component]
    return value


def _profile_string(profile: Mapping[str, Any], path: tuple[str, ...]) -> str:
    return _string(_profile_field(profile, path), _NAME, f"profile.{'.'.join(path)}")


def _profile_uint(profile: Mapping[str, Any], path: tuple[str, ...]) -> int:
    return _uint(_profile_field(profile, path), f"profile.{'.'.join(path)}")


def _object(value: Any, keys: tuple[str, ...], label: str) -> cj.Object:
    try:
        return cj.require_object(value, keys, label)
    except cj.CanonicalJsonError as exc:
        raise HostQualificationError(str(exc)) from exc


def _inspection(value: Any) -> cj.Object:
    return _object(value, _INSPECTION_KEYS, "host inspection")


def _machine(value: Any) -> cj.Object:
    obj = _object(value, _MACHINE_KEYS, "inspection.machine")
    for key in ("architecture", "kernel", "cpu_vendor", "cpu_model", "cpu_stepping", "microcode", "online_cpu_set", "benchmark_cpu_set", "numa_set"):
        if key == "cpu_stepping":
            _uint(obj[key], f"inspection.machine.{key}", 1_000_000)
        else:
            _string(obj[key], _NAME, f"inspection.machine.{key}")
    _uint(obj["cpu_family"], "inspection.machine.cpu_family", 1_000_000)
    return obj


def _docker(value: Any) -> cj.Object:
    obj = _object(value, _DOCKER_KEYS, "inspection.docker")
    for key in (
        "client_version",
        "daemon_version",
        "daemon_architecture",
        "storage_driver",
        "cgroup_version",
        "cgroup_driver",
        "cgroup_parent",
        "oci_runtime",
    ):
        _string(obj[key], _NAME, f"inspection.docker.{key}")
    _string(obj["client_sha256"], _HEX64, "inspection.docker.client_sha256")
    if obj["cgroup_driver"] not in ("cgroupfs", "systemd"):
        _error("inspection.docker.cgroup_driver", "must be cgroupfs or systemd")
    expected_parent = {"cgroupfs": "/", "systemd": "-.slice"}[obj["cgroup_driver"]]
    if obj["cgroup_parent"] != expected_parent:
        _error("inspection.docker.cgroup_parent", "does not match the cgroup driver")
    return obj


def _observation(value: Any, index: int) -> cj.Object:
    label = f"inspection.observations[{index}]"
    obj = _object(value, _OBSERVATION_KEYS, label)
    _string(obj["phase"], _NAME, f"{label}.phase")
    for key in _OBSERVATION_KEYS[1:]:
        _uint(obj[key], f"{label}.{key}")
    return obj


def _check_equal(actual: Any, expected: Any, label: str) -> None:
    if actual != expected or type(actual) is not type(expected):
        _error(label, "does not match profile")


def _qualified_observation(
    value: Any,
    index: int,
    expected: Mapping[str, Any],
) -> HostObservation:
    obj = _observation(value, index)
    phase = _string(obj["phase"], _NAME, f"inspection.observations[{index}].phase")
    expected_phase = _string(expected.get("phase"), _NAME, f"profile.observation_limits[{index}].phase")
    if phase != expected_phase:
        _error(f"inspection.observations[{index}].phase", "does not match profile phase")

    for key in ("load_milli", "cpu_pressure_total_us", "memory_pressure_total_us", "swap_read_bytes", "swap_write_bytes"):
        actual = _uint(obj[key], f"inspection.observations[{index}].{key}")
        maximum = _profile_uint_from_mapping(expected, key + "_max", f"profile.observation_limits[{index}].{key}_max")
        if actual > maximum:
            _error(f"inspection.observations[{index}].{key}", "exceeds profile limit")
    free_memory = _uint(obj["free_memory_bytes"], f"inspection.observations[{index}].free_memory_bytes")
    minimum = _profile_uint_from_mapping(expected, "free_memory_bytes_min", f"profile.observation_limits[{index}].free_memory_bytes_min")
    if free_memory < minimum:
        _error(f"inspection.observations[{index}].free_memory_bytes", "is below profile minimum")

    return HostObservation(
        phase=phase,
        load_milli=_uint(obj["load_milli"], f"inspection.observations[{index}].load_milli"),
        cpu_pressure_total_us=_uint(obj["cpu_pressure_total_us"], f"inspection.observations[{index}].cpu_pressure_total_us"),
        memory_pressure_total_us=_uint(obj["memory_pressure_total_us"], f"inspection.observations[{index}].memory_pressure_total_us"),
        free_memory_bytes=free_memory,
        swap_read_bytes=_uint(obj["swap_read_bytes"], f"inspection.observations[{index}].swap_read_bytes"),
        swap_write_bytes=_uint(obj["swap_write_bytes"], f"inspection.observations[{index}].swap_write_bytes"),
    )


def _profile_uint_from_mapping(value: Mapping[str, Any], key: str, label: str) -> int:
    if key not in value:
        _error(label, "is missing")
    return _uint(value[key], label)


def qualify(profile: Mapping[str, Any], value: Any) -> QualifiedHost:
    """Validate one canonical host/daemon observation against ``profile``.

    ``profile`` must already be accepted by :mod:`profile`; this function
    repeats the fixed identity predicates at the observation boundary so a
    malformed profile cannot silently broaden acceptance.
    """

    if not isinstance(profile, Mapping):
        _error("profile", "must be an object")
    inspection = _inspection(value)

    host_id = _string(inspection["host_id"], _NAME, "inspection.host_id")
    expected_host_id = _profile_string(profile, ("host_id",))
    _check_equal(host_id, expected_host_id, "inspection.host_id")

    machine = _machine(inspection["machine"])
    expected_machine = _profile_field(profile, ("machine",))
    if not isinstance(expected_machine, Mapping):
        _error("profile.machine", "must be an object")
    expected_architecture = _profile_string(profile, ("machine", "architecture"))
    if expected_architecture != "x86_64":
        _error("profile.machine.architecture", "must be x86_64")
    for key in _MACHINE_KEYS:
        if key in ("cpu_family", "cpu_stepping"):
            actual = _uint(machine[key], f"inspection.machine.{key}", 1_000_000)
            expected = _profile_uint(profile, ("machine", key))
        else:
            actual = _string(machine[key], _NAME, f"inspection.machine.{key}")
            expected = _profile_string(profile, ("machine", key))
        _check_equal(actual, expected, f"inspection.machine.{key}")
    if _string(machine["architecture"], _NAME, "inspection.machine.architecture") != "x86_64":
        _error("inspection.machine.architecture", "must be x86_64")

    memory_bytes = _uint(inspection["memory_bytes"], "inspection.memory_bytes")
    minimum_memory = _profile_uint(profile, ("machine", "minimum_memory_bytes"))
    if memory_bytes < minimum_memory:
        _error("inspection.memory_bytes", "is below profile minimum")
    cpu_quota_milli = _uint(inspection["cpu_quota_milli"], "inspection.cpu_quota_milli", 1_000_000)
    if cpu_quota_milli != 0:
        _error("inspection.cpu_quota_milli", "must be zero for an unquotaed host")

    docker = _docker(inspection["docker"])
    expected_docker = _profile_field(profile, ("docker",))
    if not isinstance(expected_docker, Mapping):
        _error("profile.docker", "must be an object")
    for key in _DOCKER_KEYS:
        pattern = _HEX64 if key == "client_sha256" else _NAME
        actual = _string(docker[key], pattern, f"inspection.docker.{key}")
        expected = expected_docker.get(key)
        _check_equal(actual, _string(expected, pattern, f"profile.docker.{key}"), f"inspection.docker.{key}")
    if docker["daemon_architecture"] != "x86_64":
        _error("inspection.docker.daemon_architecture", "must be x86_64")

    limits = _profile_field(profile, ("observation_limits",))
    if not isinstance(limits, list) or len(limits) != len(_PHASES):
        _error("profile.observation_limits", "must contain exactly three phases")
    observations = inspection["observations"]
    if not isinstance(observations, list) or len(observations) != len(_PHASES):
        _error("inspection.observations", "must contain exactly three phases")
    qualified_observations: list[HostObservation] = []
    for index, phase in enumerate(_PHASES):
        expected = limits[index]
        if not isinstance(expected, Mapping):
            _error(f"profile.observation_limits[{index}]", "must be an object")
        qualified = _qualified_observation(observations[index], index, expected)
        if qualified.phase != phase:
            _error(f"inspection.observations[{index}].phase", "has the wrong phase order")
        qualified_observations.append(qualified)

    return QualifiedHost(
        host_id=host_id,
        architecture=machine["architecture"],
        kernel=machine["kernel"],
        cpu_vendor=machine["cpu_vendor"],
        cpu_family=machine["cpu_family"],
        cpu_model=machine["cpu_model"],
        cpu_stepping=machine["cpu_stepping"],
        microcode=machine["microcode"],
        online_cpu_set=machine["online_cpu_set"],
        benchmark_cpu_set=machine["benchmark_cpu_set"],
        numa_set=machine["numa_set"],
        memory_bytes=memory_bytes,
        cpu_quota_milli=cpu_quota_milli,
        docker=QualifiedDocker(
            client_version=docker["client_version"],
            client_sha256=docker["client_sha256"],
            daemon_version=docker["daemon_version"],
            daemon_architecture=docker["daemon_architecture"],
            storage_driver=docker["storage_driver"],
            cgroup_version=docker["cgroup_version"],
            cgroup_driver=docker["cgroup_driver"],
            cgroup_parent=docker["cgroup_parent"],
            oci_runtime=docker["oci_runtime"],
        ),
        observations=tuple(qualified_observations),
    )
