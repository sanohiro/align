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


def make_tree(parent, name):
    root = os.path.join(parent, name)
    os.mkdir(root, 0o700)
    os.mkdir(os.path.join(root, "bin"), 0o755)
    os.mkdir(os.path.join(root, "lib"), 0o755)
    launcher = os.path.join(root, "bin", "launcher")
    module = os.path.join(root, "lib", "module.py")
    with open(launcher, "wb") as stream:
        stream.write(b"launcher\n")
    with open(module, "wb") as stream:
        stream.write(b"module\n")
    os.chmod(launcher, 0o755)
    os.chmod(module, 0o644)
    return root, manifest._write_manifest(root, "manifest.json")


def main():
    with tempfile.TemporaryDirectory() as parent:
        root, digest = make_tree(parent, "install")
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

        missing = os.path.join(parent, "missing")
        expect_error(
            lambda: bootstrap.verify_profile_manifest(missing, digest),
            "verification failed",
        )
        non_directory = os.path.join(parent, "not-directory")
        with open(non_directory, "wb") as stream:
            stream.write(b"file\n")
        expect_error(
            lambda: bootstrap.verify_profile_manifest(non_directory, digest),
            "verification failed",
        )

        extra_root, extra_digest = make_tree(parent, "extra")
        with open(os.path.join(extra_root, "extra"), "wb") as stream:
            stream.write(b"extra\n")
        os.chmod(os.path.join(extra_root, "extra"), 0o644)
        expect_error(
            lambda: bootstrap.verify_profile_manifest(extra_root, extra_digest),
            "verification failed",
        )

        malformed_root, malformed_digest = make_tree(parent, "malformed")
        with open(os.path.join(malformed_root, "manifest.json"), "wb") as stream:
            stream.write(b"{}\n")
        expect_error(
            lambda: bootstrap.verify_profile_manifest(malformed_root, malformed_digest),
            "verification failed",
        )

        manifest_link_root, manifest_link_digest = make_tree(parent, "manifest-link")
        manifest_path = os.path.join(manifest_link_root, "manifest.json")
        saved_manifest = os.path.join(manifest_link_root, "manifest.saved")
        os.rename(manifest_path, saved_manifest)
        os.symlink(saved_manifest, manifest_path)
        expect_error(
            lambda: bootstrap.verify_profile_manifest(manifest_link_root, manifest_link_digest),
            "verification failed",
        )

        nested_link_root, nested_link_digest = make_tree(parent, "nested-link")
        os.symlink("module.py", os.path.join(nested_link_root, "lib", "link"))
        expect_error(
            lambda: bootstrap.verify_profile_manifest(nested_link_root, nested_link_digest),
            "verification failed",
        )

        metadata_root, metadata_digest = make_tree(parent, "metadata")
        os.chmod(os.path.join(metadata_root, "lib", "module.py"), 0o755)
        expect_error(
            lambda: bootstrap.verify_profile_manifest(metadata_root, metadata_digest),
            "verification failed",
        )

        race_root, race_digest = make_tree(parent, "race")
        original_scan = bootstrap.manifest._scan_directory
        added = False

        def add_after_root_scan(fd, prefix, excluded):
            nonlocal added
            entries = original_scan(fd, prefix, excluded)
            if not added and prefix == "":
                added = True
                late_fd = os.open(
                    "late-entry",
                    os.O_WRONLY | os.O_CREAT | os.O_EXCL,
                    0o644,
                    dir_fd=fd,
                )
                os.close(late_fd)
            return entries

        bootstrap.manifest._scan_directory = add_after_root_scan
        try:
            expect_error(
                lambda: bootstrap.verify_profile_manifest(race_root, race_digest),
                "verification failed",
            )
        finally:
            bootstrap.manifest._scan_directory = original_scan

        root_race, root_race_digest = make_tree(parent, "root-race")
        original_scan = bootstrap.manifest._scan_directory
        changed_root = False

        def change_root_after_scan(fd, prefix, excluded):
            nonlocal changed_root
            entries = original_scan(fd, prefix, excluded)
            if not changed_root and prefix == "":
                changed_root = True
                os.fchmod(fd, 0o755)
            return entries

        bootstrap.manifest._scan_directory = change_root_after_scan
        try:
            expect_error(
                lambda: bootstrap.verify_profile_manifest(root_race, root_race_digest),
                "verification failed",
            )
        finally:
            bootstrap.manifest._scan_directory = original_scan

        launcher = os.path.join(root, "bin", "launcher")
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
