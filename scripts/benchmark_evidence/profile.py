"""Validation and canonical encoding for the fixed evidence host profile.

The profile is a protected, immutable input to the future controller.  This
module only validates its canonical bytes and exposes the resulting ordered
record; it does not inspect the host, Docker daemon, image, or executable
paths.  Those observations belong to the controller's privilege boundary.
"""

from __future__ import annotations

import base64
import binascii
import os
import re
from typing import Any, Sequence

from . import canonical_json as cj


class ProfileError(cj.CanonicalJsonError):
    """A profile is malformed or violates the fixed profile contract."""


PROFILE_SCHEMA = "align.json_escape_benchmark_profile/v1"
TARGET_REF = "refs/heads/main"
THRESHOLD_NUMERATOR = 105
THRESHOLD_DENOMINATOR = 100
WARMUP_COUNT = 1
PAIR_COUNT = 10
BENCHMARKS = ("json_decode", "json_soa")
FIELDS = ("A-full", "A-proj", "soa ms", "aos ms", "proj ms")

_NAME = re.compile(r"[A-Za-z0-9._/:+=@-]{1,255}\Z")
_FIELD_NAME = re.compile(r"[A-Za-z0-9._/:+=@ -]{1,255}\Z")
_PATH = re.compile(r"/[A-Za-z0-9._/-]{1,4095}\Z")
_HEX64 = re.compile(r"[0-9a-f]{64}\Z")
_DIGEST = re.compile(r"sha256:[0-9a-f]{64}\Z")
_FINGERPRINT = re.compile(r"SHA256:[A-Za-z0-9+/]{43}\Z")
_BASE64 = re.compile(r"[A-Za-z0-9+/]{43}={0,1}\Z")

_PROFILE_KEYS = (
    "schema",
    "profile_id",
    "target_ref",
    "host_id",
    "host_lock_path",
    "machine",
    "observation_limits",
    "docker",
    "image",
    "toolchain",
    "cargo_cache_manifest_sha256",
    "cargo_config_sha256",
    "capture_limits",
    "phase_timeouts",
    "signing",
    "schedule",
    "benchmarks",
    "fields",
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
    "minimum_memory_bytes",
)
_OBSERVATION_KEYS = (
    "phase",
    "load_milli_max",
    "cpu_pressure_total_us_max",
    "memory_pressure_total_us_max",
    "free_memory_bytes_min",
    "swap_read_bytes_max",
    "swap_write_bytes_max",
)
_DOCKER_KEYS = (
    "client_version",
    "client_sha256",
    "daemon_version",
    "daemon_architecture",
    "storage_driver",
    "cgroup_version",
    "oci_runtime",
)
_IMAGE_KEYS = ("registry_digest", "local_image_id", "config_digest", "platform")
_EXECUTABLE_KEYS = ("version", "executable_sha256")
_TOOLCHAIN_KEYS = (
    "python",
    "git",
    "cargo",
    "rustc",
    "llvm",
    "cc",
    "linker",
    "ssh_keygen",
)
_CAPTURE_KEYS = ("stdout_max_bytes", "stderr_max_bytes", "stderr_tail_max_bytes")
_TIMEOUT_KEYS = ("prepare_ns", "warmup_ns", "sample_ns", "monitor_ns", "cleanup_ns")
_SIGNING_KEYS = ("key_type", "public_key_base64", "fingerprint")
_SCHEDULE_KEYS = (
    "threshold_numerator",
    "threshold_denominator",
    "warmup_count",
    "pair_count",
)

_OBSERVATION_PHASES = ("pre", "between", "post")
_MAX_PROFILE_BYTES = 1 << 20
_MAX_CAPTURE_BYTES = 1 << 30
_MAX_TIMEOUT_NS = 24 * 60 * 60 * 1_000_000_000


def _error(label: str, message: str) -> None:
    raise ProfileError(f"{label}: {message}")


def _object(value: Any, keys: Sequence[str], label: str) -> cj.Object:
    try:
        return cj.require_object(value, keys, label)
    except cj.CanonicalJsonError as exc:
        raise ProfileError(str(exc)) from exc


def _string(value: Any, pattern: re.Pattern[str], label: str) -> str:
    try:
        value = cj.require_string(value, label)
    except cj.CanonicalJsonError as exc:
        raise ProfileError(str(exc)) from exc
    if pattern.fullmatch(value) is None:
        _error(label, "has an invalid grammar")
    return value


def _uint(value: Any, label: str, maximum: int) -> int:
    try:
        return cj.require_uint(value, label, maximum)
    except cj.CanonicalJsonError as exc:
        raise ProfileError(str(exc)) from exc


def _hash(value: Any, label: str) -> str:
    return _string(value, _HEX64, label)


def _absolute_path(value: Any, label: str) -> str:
    path = _string(value, _PATH, label)
    if not os.path.isabs(path) or path == "/" or path.endswith("/"):
        _error(label, "must be an absolute non-root path without a trailing separator")
    parts = path.split("/")
    if any(part in ("", ".", "..") for part in parts[1:]):
        _error(label, "contains an empty or traversal component")
    return path


def _executable(value: Any, label: str) -> cj.Object:
    obj = _object(value, _EXECUTABLE_KEYS, label)
    _string(obj["version"], _NAME, f"{label}.version")
    _hash(obj["executable_sha256"], f"{label}.executable_sha256")
    return obj


def _validate_machine(value: Any) -> cj.Object:
    obj = _object(value, _MACHINE_KEYS, "machine")
    if _string(obj["architecture"], _NAME, "machine.architecture") != "x86_64":
        _error("machine.architecture", "must be x86_64")
    for key in ("kernel", "cpu_vendor", "cpu_model", "microcode", "online_cpu_set", "benchmark_cpu_set", "numa_set"):
        _string(obj[key], _NAME, f"machine.{key}")
    for key in ("cpu_family", "cpu_stepping"):
        _uint(obj[key], f"machine.{key}", 1_000_000)
    if _uint(obj["minimum_memory_bytes"], "machine.minimum_memory_bytes", (1 << 64) - 1) == 0:
        _error("machine.minimum_memory_bytes", "must be positive")
    return obj


def _validate_observation_limits(value: Any) -> list[cj.Object]:
    if not isinstance(value, list) or len(value) != len(_OBSERVATION_PHASES):
        _error("observation_limits", "must contain exactly three phase records")
    result: list[cj.Object] = []
    for index, expected_phase in enumerate(_OBSERVATION_PHASES):
        obj = _object(value[index], _OBSERVATION_KEYS, f"observation_limits[{index}]")
        if _string(obj["phase"], _NAME, f"observation_limits[{index}].phase") != expected_phase:
            _error(f"observation_limits[{index}].phase", "has the wrong phase order")
        _uint(obj["load_milli_max"], f"observation_limits[{index}].load_milli_max", 1_000_000)
        for key in ("cpu_pressure_total_us_max", "memory_pressure_total_us_max", "swap_read_bytes_max", "swap_write_bytes_max"):
            _uint(obj[key], f"observation_limits[{index}].{key}", (1 << 64) - 1)
        _uint(obj["free_memory_bytes_min"], f"observation_limits[{index}].free_memory_bytes_min", (1 << 64) - 1)
        result.append(obj)
    return result


def _validate_docker(value: Any) -> cj.Object:
    obj = _object(value, _DOCKER_KEYS, "docker")
    for key in ("client_version", "daemon_version", "daemon_architecture", "storage_driver", "cgroup_version", "oci_runtime"):
        _string(obj[key], _NAME, f"docker.{key}")
    _hash(obj["client_sha256"], "docker.client_sha256")
    if obj["daemon_architecture"] != "x86_64":
        _error("docker.daemon_architecture", "must be x86_64")
    return obj


def _validate_image(value: Any) -> cj.Object:
    obj = _object(value, _IMAGE_KEYS, "image")
    _string(obj["registry_digest"], _DIGEST, "image.registry_digest")
    _hash(obj["local_image_id"], "image.local_image_id")
    _hash(obj["config_digest"], "image.config_digest")
    if _string(obj["platform"], _NAME, "image.platform") != "linux/amd64":
        _error("image.platform", "must be linux/amd64")
    return obj


def _validate_toolchain(value: Any) -> cj.Object:
    obj = _object(value, _TOOLCHAIN_KEYS, "toolchain")
    for key in _TOOLCHAIN_KEYS:
        _executable(obj[key], f"toolchain.{key}")
    return obj


def _validate_limits(value: Any, keys: Sequence[str], label: str, maximum: int) -> cj.Object:
    obj = _object(value, keys, label)
    for key in keys:
        number = _uint(obj[key], f"{label}.{key}", maximum)
        if number == 0:
            _error(f"{label}.{key}", "must be positive")
    return obj


def _validate_signing(value: Any) -> cj.Object:
    obj = _object(value, _SIGNING_KEYS, "signing")
    if _string(obj["key_type"], _NAME, "signing.key_type") != "ssh-ed25519":
        _error("signing.key_type", "must be ssh-ed25519")
    key = _string(obj["public_key_base64"], _BASE64, "signing.public_key_base64")
    try:
        decoded = base64.b64decode(key.encode("ascii"), validate=True)
    except (binascii.Error, ValueError) as exc:
        raise ProfileError("signing.public_key_base64 is not valid base64") from exc
    if len(decoded) != 32:
        _error("signing.public_key_base64", "must decode to 32 bytes")
    _string(obj["fingerprint"], _FINGERPRINT, "signing.fingerprint")
    return obj


def _validate_schedule(value: Any) -> cj.Object:
    obj = _object(value, _SCHEDULE_KEYS, "schedule")
    if _uint(obj["threshold_numerator"], "schedule.threshold_numerator", 1_000_000) != THRESHOLD_NUMERATOR:
        _error("schedule.threshold_numerator", "must be 105")
    if _uint(obj["threshold_denominator"], "schedule.threshold_denominator", 1_000_000) != THRESHOLD_DENOMINATOR:
        _error("schedule.threshold_denominator", "must be 100")
    if _uint(obj["warmup_count"], "schedule.warmup_count", 100) != WARMUP_COUNT:
        _error("schedule.warmup_count", "must be 1")
    if _uint(obj["pair_count"], "schedule.pair_count", 100) != PAIR_COUNT:
        _error("schedule.pair_count", "must be 10")
    return obj


def _validate_inventory(
    value: Any,
    expected: Sequence[str],
    label: str,
    pattern: re.Pattern[str],
) -> list[str]:
    if not isinstance(value, list) or tuple(value) != tuple(expected):
        _error(label, f"must equal the fixed {label} inventory")
    for index, member in enumerate(value):
        _string(member, pattern, f"{label}[{index}]")
    return value


def validate_profile(value: Any) -> cj.Object:
    """Validate one decoded profile and return its ordered object unchanged."""

    obj = _object(value, _PROFILE_KEYS, "profile")
    if _string(obj["schema"], _NAME, "profile.schema") != PROFILE_SCHEMA:
        _error("profile.schema", "has the wrong schema")
    _string(obj["profile_id"], _NAME, "profile.profile_id")
    if _string(obj["target_ref"], _NAME, "profile.target_ref") != TARGET_REF:
        _error("profile.target_ref", "must be refs/heads/main")
    _string(obj["host_id"], _NAME, "profile.host_id")
    _absolute_path(obj["host_lock_path"], "profile.host_lock_path")
    _validate_machine(obj["machine"])
    _validate_observation_limits(obj["observation_limits"])
    _validate_docker(obj["docker"])
    _validate_image(obj["image"])
    _validate_toolchain(obj["toolchain"])
    _hash(obj["cargo_cache_manifest_sha256"], "profile.cargo_cache_manifest_sha256")
    _hash(obj["cargo_config_sha256"], "profile.cargo_config_sha256")
    _validate_limits(obj["capture_limits"], _CAPTURE_KEYS, "capture_limits", _MAX_CAPTURE_BYTES)
    _validate_limits(obj["phase_timeouts"], _TIMEOUT_KEYS, "phase_timeouts", _MAX_TIMEOUT_NS)
    _validate_signing(obj["signing"])
    _validate_schedule(obj["schedule"])
    _validate_inventory(obj["benchmarks"], BENCHMARKS, "benchmarks", _NAME)
    _validate_inventory(obj["fields"], FIELDS, "fields", _FIELD_NAME)
    return obj


def encode_profile(value: Any) -> bytes:
    """Validate and encode one profile with canonical JSON plus one LF."""

    try:
        profile = validate_profile(value)
        return cj.encode(profile)
    except ProfileError:
        raise
    except cj.CanonicalJsonError as exc:
        raise ProfileError(str(exc)) from exc


def decode_profile(raw: bytes) -> cj.Object:
    """Decode canonical profile bytes and validate all semantic fields."""

    if not isinstance(raw, bytes) or len(raw) > _MAX_PROFILE_BYTES:
        raise ProfileError("profile bytes exceed the fixed limit")
    try:
        return validate_profile(cj.decode(raw))
    except ProfileError:
        raise
    except cj.CanonicalJsonError as exc:
        raise ProfileError(str(exc)) from exc


def profile_sha256(raw: bytes) -> str:
    """Return the digest of already-validated canonical profile bytes."""

    decode_profile(raw)
    return cj.sha256(raw)
