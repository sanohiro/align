"""Acquire and qualify the pinned evidence image through native Docker.

The pure :mod:`image` module validates an already assembled observation.  This
module owns the privileged acquisition boundary: one fixed host-side image
inspection and one fixed no-network self-inspection inside the image.  It does
not open a repository, execute candidate code, start a benchmark, or manage a
signing key.
"""

from __future__ import annotations

import hashlib
import re
from typing import Any, Callable, Mapping, Sequence

from . import canonical_json as cj
from . import container
from . import image
from . import native_host


class NativeImageError(RuntimeError):
    """A native image observation cannot cross the trusted boundary."""


DOCKER = native_host.DOCKER
DOCKER_CONFIG = native_host.DOCKER_CONFIG
DOCKER_HOST = native_host.DOCKER_HOST
IMAGE_INSPECT_FORMAT = (
    '{"image_id":{{json .Id}},"repo_digests":{{json .RepoDigests}},'
    '"repo_tags":{{json .RepoTags}},"os":{{json .Os}},'
    '"architecture":{{json .Architecture}}}'
)

_HOST_KEYS = ("image_id", "repo_digests", "repo_tags", "os", "architecture")
_SELF_KEYS = (
    "config_digest",
    "toolchain",
    "cargo_cache_manifest_sha256",
    "cargo_config_sha256",
)
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
_EXECUTABLE_KEYS = ("version", "executable_sha256")
_HEX64 = re.compile(r"[0-9a-f]{64}\Z")
_DIGEST = re.compile(r"sha256:[0-9a-f]{64}\Z")
_NAME = re.compile(r"[A-Za-z0-9._/:+=@-]{1,255}\Z")
_MAX_IDENTITY_BYTES = 64 << 10

Runner = Callable[[tuple[str, ...]], bytes]
Hasher = Callable[[str], str]


def _error(message: str) -> None:
    raise NativeImageError(message)


def _string(value: Any, pattern: re.Pattern[str], label: str) -> str:
    if not isinstance(value, str) or pattern.fullmatch(value) is None:
        _error(f"{label} has an invalid grammar")
    return value


def _profile_field(profile: Mapping[str, Any], path: Sequence[str]) -> Any:
    value: Any = profile
    for component in path:
        if not isinstance(value, Mapping) or component not in value:
            _error(f"profile is missing {'.'.join(path)}")
        value = value[component]
    return value


def _local_image_digest(profile: Mapping[str, Any]) -> str:
    image_profile = _profile_field(profile, ("image",))
    if not isinstance(image_profile, Mapping):
        _error("profile.image must be an object")
    local_id = _string(image_profile.get("local_image_id"), _HEX64, "profile.image.local_image_id")
    return f"sha256:{local_id}"


def _client_hash(profile: Mapping[str, Any]) -> str:
    docker_profile = _profile_field(profile, ("docker",))
    if not isinstance(docker_profile, Mapping):
        _error("profile.docker must be an object")
    return _string(docker_profile.get("client_sha256"), _HEX64, "profile.docker.client_sha256")


def _child_id(profile: Mapping[str, Any]) -> str:
    profile_id = _string(_profile_field(profile, ("profile_id",)), _NAME, "profile.profile_id")
    raw = (
        b"align-json-escape-evidence-image-self-inspection-v1\0"
        + profile_id.encode("ascii")
        + b"\0"
        + _local_image_digest(profile).encode("ascii")
    )
    return hashlib.sha256(raw).hexdigest()


def host_inspect_argv(profile: Mapping[str, Any]) -> tuple[str, ...]:
    """Return the fixed daemon-side image inspection argv."""

    return (
        DOCKER,
        "--config",
        DOCKER_CONFIG,
        "--host",
        DOCKER_HOST,
        "image",
        "inspect",
        "--platform=linux/amd64",
        f"--format={IMAGE_INSPECT_FORMAT}",
        _local_image_digest(profile),
    )


def _decode_object(raw: bytes, keys: Sequence[str], label: str) -> cj.Object:
    if not isinstance(raw, bytes) or len(raw) > _MAX_IDENTITY_BYTES:
        _error(f"{label} exceeds the fixed output limit")
    try:
        value = cj.decode(raw)
        return cj.require_object(value, keys, label)
    except cj.CanonicalJsonError as exc:
        raise NativeImageError(f"{label}: {exc}") from exc


def _parse_host(raw: bytes) -> cj.Object:
    value = _decode_object(raw, _HOST_KEYS, "Docker image inspection")
    image_id = _string(value["image_id"], _DIGEST, "image inspection.image_id")
    digests = value["repo_digests"]
    if not isinstance(digests, list) or len(digests) != 1 or not isinstance(digests[0], str):
        _error("image inspection.repo_digests must contain exactly one digest")
    if digests[0].count("@") != 1:
        _error("image inspection.repo_digests entry lacks a repository separator")
    repository, registry_digest = digests[0].rsplit("@", 1)
    _string(repository, _NAME, "image inspection.repo_digests repository")
    _string(registry_digest, _DIGEST, "image inspection.repo_digests digest")
    tags = value["repo_tags"]
    if not isinstance(tags, list) or tags:
        _error("image inspection.repo_tags must be an empty list")
    if value["os"] != "linux":
        _error("image inspection.os must be linux")
    if value["architecture"] != "amd64":
        _error("image inspection.architecture must be amd64")
    return cj.Object(
        (
            ("image_id", image_id),
            ("registry_digest", registry_digest),
            ("repo_tags", tags),
        )
    )


def _parse_executable(value: Any, label: str) -> cj.Object:
    try:
        executable = cj.require_object(value, _EXECUTABLE_KEYS, label)
        version = cj.require_string(executable["version"], f"{label}.version")
        digest = cj.require_string(executable["executable_sha256"], f"{label}.executable_sha256")
    except cj.CanonicalJsonError as exc:
        raise NativeImageError(str(exc)) from exc
    _string(version, _NAME, f"{label}.version")
    _string(digest, _HEX64, f"{label}.executable_sha256")
    return executable


def _parse_self(raw: bytes) -> cj.Object:
    value = _decode_object(raw, _SELF_KEYS, "image self-inspection")
    config_digest = _string(value["config_digest"], _DIGEST, "self-inspection.config_digest")
    try:
        toolchain = cj.require_object(value["toolchain"], _TOOLCHAIN_KEYS, "self-inspection.toolchain")
    except cj.CanonicalJsonError as exc:
        raise NativeImageError(str(exc)) from exc
    parsed_toolchain = cj.Object(
        tuple((key, _parse_executable(toolchain[key], f"self-inspection.toolchain.{key}")) for key in _TOOLCHAIN_KEYS)
    )
    cache_manifest = _string(
        value["cargo_cache_manifest_sha256"],
        _HEX64,
        "self-inspection.cargo_cache_manifest_sha256",
    )
    cargo_config = _string(
        value["cargo_config_sha256"],
        _HEX64,
        "self-inspection.cargo_config_sha256",
    )
    return cj.Object(
        (
            ("config_digest", config_digest),
            ("toolchain", parsed_toolchain),
            ("cargo_cache_manifest_sha256", cache_manifest),
            ("cargo_config_sha256", cargo_config),
        )
    )


def _merge(host: cj.Object, self_inspection: cj.Object) -> cj.Object:
    return cj.Object(
        (
            ("registry_digest", host["registry_digest"]),
            ("image_id", host["image_id"]),
            ("config_digest", self_inspection["config_digest"]),
            ("platform", "linux/amd64"),
            ("repo_tags", host["repo_tags"]),
            ("toolchain", self_inspection["toolchain"]),
            ("cargo_cache_manifest_sha256", self_inspection["cargo_cache_manifest_sha256"]),
            ("cargo_config_sha256", self_inspection["cargo_config_sha256"]),
        )
    )


def _run_one(
    profile: Mapping[str, Any],
    argv: tuple[str, ...],
    *,
    runner: Runner,
    hasher: Hasher,
) -> bytes:
    expected = _client_hash(profile)
    try:
        if runner is native_host.run_command and hasher is native_host.hash_executable:
            outputs, _actual = native_host.run_docker_commands((argv,), expected)
            if len(outputs) != 1:
                _error("Docker command returned the wrong output count")
            return outputs[0]
        actual = hasher(DOCKER)
    except NativeImageError:
        raise
    except native_host.NativeHostError as exc:
        raise NativeImageError(str(exc)) from exc
    if not isinstance(actual, str) or _HEX64.fullmatch(actual) is None:
        _error("Docker client hash is malformed")
    if actual != expected:
        _error("Docker client digest does not match profile before Docker execution")
    try:
        return runner(argv)
    except NativeImageError:
        raise
    except BaseException as exc:
        raise NativeImageError("Docker image command failed") from exc


def inspect(
    profile: Mapping[str, Any],
    *,
    runner: Runner = native_host.run_command,
    hasher: Hasher = native_host.hash_executable,
) -> cj.Object:
    """Acquire one fixed host/image observation without qualifying it."""

    if not isinstance(profile, Mapping):
        _error("profile must be an object")
    child_id = _child_id(profile)
    run_argv = container.build_image_inspection_argv(profile, child_id)
    host = _parse_host(_run_one(profile, host_inspect_argv(profile), runner=runner, hasher=hasher))
    try:
        self_raw = _run_one(profile, run_argv, runner=runner, hasher=hasher)
    except NativeImageError as run_error:
        cleanup_argv = container.build_image_inspection_cleanup_argv(profile, child_id)
        try:
            _run_one(profile, cleanup_argv, runner=runner, hasher=hasher)
        except NativeImageError as cleanup_error:
            raise NativeImageError("image inspection failed and container cleanup is uncertain") from cleanup_error
        raise NativeImageError("image self-inspection failed after fixed container cleanup") from run_error
    self_inspection = _parse_self(self_raw)
    return _merge(host, self_inspection)


def qualify(
    profile: Mapping[str, Any],
    **kwargs: Any,
) -> image.QualifiedImage:
    """Acquire and validate the native image observation against ``profile``."""

    try:
        return image.qualify(profile, inspect(profile, **kwargs))
    except image.ImageQualificationError as exc:
        raise NativeImageError(f"native image qualification rejected: {exc}") from exc
