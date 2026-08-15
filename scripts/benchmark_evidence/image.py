"""Bind a qualified image inspection to the immutable evidence profile.

The controller obtains the inspection through its privileged Docker/image
boundary.  This module does not invoke Docker or inspect the host; it only
rejects an observation that cannot be the profile-pinned Linux x86_64 image
and its complete offline toolchain.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Any, Mapping

from . import canonical_json as cj


class ImageQualificationError(ValueError):
    """An image inspection does not satisfy the immutable profile identity."""


_HEX64 = re.compile(r"[0-9a-f]{64}\Z")
_DIGEST = re.compile(r"sha256:[0-9a-f]{64}\Z")
_NAME = re.compile(r"[A-Za-z0-9._/:+=@-]{1,255}\Z")

_INSPECTION_KEYS = (
    "registry_digest",
    "image_id",
    "config_digest",
    "platform",
    "repo_tags",
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


@dataclass(frozen=True)
class QualifiedImage:
    """The profile-bound image and toolchain identity for a future child."""

    registry_digest: str
    image_id: str
    config_digest: str
    platform: str
    toolchain: tuple[tuple[str, str, str], ...]
    cargo_cache_manifest_sha256: str
    cargo_config_sha256: str


def _error(label: str, message: str) -> None:
    raise ImageQualificationError(f"{label}: {message}")


def _string(value: Any, pattern: re.Pattern[str], label: str) -> str:
    if not isinstance(value, str) or pattern.fullmatch(value) is None:
        _error(label, "has an invalid grammar")
    return value


def _profile_field(profile: Mapping[str, Any], path: tuple[str, ...]) -> Any:
    value: Any = profile
    for component in path:
        if not isinstance(value, Mapping) or component not in value:
            _error("profile", f"is missing {'.'.join(path)}")
        value = value[component]
    return value


def _profile_hash(profile: Mapping[str, Any], path: tuple[str, ...]) -> str:
    return _string(_profile_field(profile, path), _HEX64, f"profile.{'.'.join(path)}")


def _profile_digest(profile: Mapping[str, Any], path: tuple[str, ...]) -> str:
    return _string(_profile_field(profile, path), _DIGEST, f"profile.{'.'.join(path)}")


def _inspection(value: Any) -> cj.Object:
    try:
        return cj.require_object(value, _INSPECTION_KEYS, "image inspection")
    except cj.CanonicalJsonError as exc:
        raise ImageQualificationError(str(exc)) from exc


def _inspection_executable(value: Any, label: str) -> tuple[str, str]:
    try:
        obj = cj.require_object(value, _EXECUTABLE_KEYS, label)
        version = cj.require_string(obj["version"], f"{label}.version")
        digest = cj.require_string(obj["executable_sha256"], f"{label}.executable_sha256")
    except cj.CanonicalJsonError as exc:
        raise ImageQualificationError(str(exc)) from exc
    _string(version, _NAME, f"{label}.version")
    _string(digest, _HEX64, f"{label}.executable_sha256")
    return version, digest


def qualify(profile: Mapping[str, Any], value: Any) -> QualifiedImage:
    """Validate one canonical image/toolchain inspection against ``profile``.

    ``profile`` must already be accepted by :mod:`profile`; this function owns
    the observation-side identity and mutable-tag checks at the image boundary.
    """

    if not isinstance(profile, Mapping):
        _error("profile", "must be an object")
    inspection = _inspection(value)
    image = _profile_field(profile, ("image",))
    if not isinstance(image, Mapping):
        _error("profile.image", "must be an object")

    registry_digest = _string(inspection["registry_digest"], _DIGEST, "inspection.registry_digest")
    expected_registry_digest = _profile_digest(profile, ("image", "registry_digest"))
    if registry_digest != expected_registry_digest:
        _error("inspection.registry_digest", "does not match profile.image.registry_digest")

    image_id = _string(inspection["image_id"], _DIGEST, "inspection.image_id")
    expected_image_id = f"sha256:{_profile_hash(profile, ('image', 'local_image_id'))}"
    if image_id != expected_image_id:
        _error("inspection.image_id", "does not match profile.image.local_image_id")

    config_digest = _string(inspection["config_digest"], _DIGEST, "inspection.config_digest")
    expected_config_digest = f"sha256:{_profile_hash(profile, ('image', 'config_digest'))}"
    if config_digest != expected_config_digest:
        _error("inspection.config_digest", "does not match profile.image.config_digest")

    platform = _string(inspection["platform"], _NAME, "inspection.platform")
    if platform != "linux/amd64":
        _error("inspection.platform", "must be linux/amd64")
    if platform != _string(_profile_field(profile, ("image", "platform")), _NAME, "profile.image.platform"):
        _error("inspection.platform", "does not match profile.image.platform")

    tags = inspection["repo_tags"]
    if not isinstance(tags, list) or tags:
        _error("inspection.repo_tags", "must be an empty list")

    try:
        toolchain = cj.require_object(inspection["toolchain"], _TOOLCHAIN_KEYS, "inspection.toolchain")
    except cj.CanonicalJsonError as exc:
        raise ImageQualificationError(str(exc)) from exc
    expected_toolchain = _profile_field(profile, ("toolchain",))
    if not isinstance(expected_toolchain, Mapping):
        _error("profile.toolchain", "must be an object")
    qualified_toolchain: list[tuple[str, str, str]] = []
    for key in _TOOLCHAIN_KEYS:
        version, digest = _inspection_executable(toolchain[key], f"inspection.toolchain.{key}")
        expected = expected_toolchain.get(key)
        if not isinstance(expected, Mapping):
            _error(f"profile.toolchain.{key}", "must be an object")
        expected_version = _string(expected.get("version"), _NAME, f"profile.toolchain.{key}.version")
        expected_digest = _string(
            expected.get("executable_sha256"),
            _HEX64,
            f"profile.toolchain.{key}.executable_sha256",
        )
        if (version, digest) != (expected_version, expected_digest):
            _error(f"inspection.toolchain.{key}", "does not match profile.toolchain identity")
        qualified_toolchain.append((key, version, digest))

    cache_manifest = _string(
        inspection["cargo_cache_manifest_sha256"],
        _HEX64,
        "inspection.cargo_cache_manifest_sha256",
    )
    if cache_manifest != _profile_hash(profile, ("cargo_cache_manifest_sha256",)):
        _error("inspection.cargo_cache_manifest_sha256", "does not match profile")
    cargo_config = _string(inspection["cargo_config_sha256"], _HEX64, "inspection.cargo_config_sha256")
    if cargo_config != _profile_hash(profile, ("cargo_config_sha256",)):
        _error("inspection.cargo_config_sha256", "does not match profile")

    return QualifiedImage(
        registry_digest=registry_digest,
        image_id=image_id,
        config_digest=config_digest,
        platform=platform,
        toolchain=tuple(qualified_toolchain),
        cargo_cache_manifest_sha256=cache_manifest,
        cargo_config_sha256=cargo_config,
    )
