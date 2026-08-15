"""Bind an installed evidence tree to its profile-owned manifest digest.

The manifest module owns the no-follow tree walk and canonical manifest
validation.  This layer adds the reviewed bootstrap identity check: the
caller supplies the profile's already trusted manifest SHA-256, while the
manifest path and all source execution/import decisions stay fixed.  Git raw
objects, installation replacement, and controller execution remain later
layers.
"""

from __future__ import annotations

import re
from dataclasses import dataclass

from . import manifest


class BootstrapError(ValueError):
    """An installed evidence tree does not match its trusted profile."""


MANIFEST_PATH = "manifest.json"
_SHA256 = re.compile(r"[0-9a-f]{64}\Z")


@dataclass(frozen=True)
class VerifiedManifest:
    """The identity result of a completed installed-tree verification."""

    manifest_sha256: str


def _expected_digest(value: object) -> str:
    if not isinstance(value, str) or _SHA256.fullmatch(value) is None:
        raise BootstrapError("expected manifest SHA-256 must be lowercase 64-hex")
    return value


def verify_profile_manifest(root: str, expected_manifest_sha256: object) -> VerifiedManifest:
    """Verify the fixed installed manifest and bind it to its profile digest.

    The expected digest is validated before touching ``root``.  The underlying
    verifier opens the root and manifest without following symlinks, checks
    canonical bytes and metadata, and hashes the exact manifest bytes.
    """

    expected = _expected_digest(expected_manifest_sha256)
    try:
        observed = manifest.verify_manifest(root, MANIFEST_PATH)
    except manifest.ManifestError as exc:
        raise BootstrapError(f"installed manifest verification failed: {exc}") from exc
    if observed != expected:
        raise BootstrapError("installed manifest SHA-256 does not match the profile")
    return VerifiedManifest(manifest_sha256=observed)
