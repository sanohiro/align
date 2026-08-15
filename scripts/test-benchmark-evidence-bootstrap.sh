#!/usr/bin/env bash
# Deterministic owner for profile-bound installed-source bootstrap.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
import os
import tempfile

from scripts.benchmark_evidence import bootstrap
from scripts.benchmark_evidence import manifest


def expect_error(call, fragment):
    try:
        call()
    except bootstrap.BootstrapError as exc:
        assert fragment in str(exc), str(exc)
    else:
        raise AssertionError("accepted invalid bootstrap input")


def main():
    with tempfile.TemporaryDirectory() as parent:
        root = os.path.join(parent, "install")
        os.mkdir(root, 0o700)
        os.mkdir(os.path.join(root, "bin"), 0o755)
        launcher = os.path.join(root, "bin", "launcher")
        with open(launcher, "wb") as stream:
            stream.write(b"launcher\n")
        os.chmod(launcher, 0o755)

        digest = manifest._write_manifest(root, "manifest.json")
        verified = bootstrap.verify_profile_manifest(root, digest)
        assert verified == bootstrap.VerifiedManifest(digest)

        expect_error(
            lambda: bootstrap.verify_profile_manifest(root, "f" * 64),
            "does not match",
        )
        expect_error(
            lambda: bootstrap.verify_profile_manifest(root, "F" * 64),
            "lowercase 64-hex",
        )
        expect_error(
            lambda: bootstrap.verify_profile_manifest(root, ""),
            "lowercase 64-hex",
        )
        expect_error(
            lambda: bootstrap.verify_profile_manifest(root, b"0" * 64),
            "lowercase 64-hex",
        )

        original_verify = bootstrap.manifest.verify_manifest

        def unexpected_verify(*args, **kwargs):
            raise AssertionError("invalid expected digest reached tree verification")

        bootstrap.manifest.verify_manifest = unexpected_verify
        expect_error(
            lambda: bootstrap.verify_profile_manifest(root, "not-a-digest"),
            "lowercase 64-hex",
        )
        bootstrap.manifest.verify_manifest = original_verify

        with open(launcher, "ab") as stream:
            stream.write(b"changed\n")
        expect_error(
            lambda: bootstrap.verify_profile_manifest(root, digest),
            "verification failed",
        )

        link = os.path.join(parent, "root-link")
        os.symlink(root, link)
        expect_error(
            lambda: bootstrap.verify_profile_manifest(link, digest),
            "verification failed",
        )

    print("benchmark evidence bootstrap checks passed")


main()
PY
