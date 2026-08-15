#!/usr/bin/env bash
# Deterministic owner for profile-bound image and toolchain qualification.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
from scripts.benchmark_evidence import canonical_json as cj
from scripts.benchmark_evidence import image


H64 = "0" * 64
DIGEST = "sha256:" + H64
TOOLCHAIN_KEYS = ("python", "git", "cargo", "rustc", "llvm", "cc", "linker", "ssh_keygen")


def O(*pairs):
    return cj.Object(pairs)


def executable(key):
    return O(("version", key + "-1"), ("executable_sha256", H64))


PROFILE = {
    "image": {
        "registry_digest": DIGEST,
        "local_image_id": H64,
        "config_digest": H64,
        "platform": "linux/amd64",
    },
    "toolchain": {key: executable(key) for key in TOOLCHAIN_KEYS},
    "cargo_cache_manifest_sha256": H64,
    "cargo_config_sha256": H64,
}


def inspection():
    return O(
        ("registry_digest", DIGEST),
        ("image_id", DIGEST),
        ("config_digest", DIGEST),
        ("platform", "linux/amd64"),
        ("repo_tags", []),
        ("toolchain", O(*((key, executable(key)) for key in TOOLCHAIN_KEYS))),
        ("cargo_cache_manifest_sha256", H64),
        ("cargo_config_sha256", H64),
    )


def top_level(value, key, replacement):
    return O(*((name, replacement if name == key else member) for name, member in value.pairs))


def toolchain_value(value, key, replacement):
    toolchain = value["toolchain"]
    updated = O(*((name, replacement if name == key else member) for name, member in toolchain.pairs))
    return top_level(value, "toolchain", updated)


def rejected(label, value, fragment, profile=PROFILE):
    try:
        image.qualify(profile, value)
    except image.ImageQualificationError as exc:
        assert fragment in str(exc), (label, fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid inspection: {label}")


def main():
    qualified = image.qualify(PROFILE, inspection())
    assert qualified.registry_digest == DIGEST
    assert qualified.image_id == DIGEST
    assert qualified.config_digest == DIGEST
    assert qualified.platform == "linux/amd64"
    assert qualified.toolchain[0] == ("python", "python-1", H64)
    assert len(qualified.toolchain) == len(TOOLCHAIN_KEYS)
    assert qualified.cargo_cache_manifest_sha256 == H64
    assert qualified.cargo_config_sha256 == H64

    rejected("registry digest", top_level(inspection(), "registry_digest", "sha256:" + "1" * 64), "registry_digest")
    rejected("local image id", top_level(inspection(), "image_id", "sha256:" + "1" * 64), "image_id")
    rejected("config digest", top_level(inspection(), "config_digest", "sha256:" + "1" * 64), "config_digest")
    rejected("wrong platform", top_level(inspection(), "platform", "linux/arm64"), "linux/amd64")
    rejected("mutable tag", top_level(inspection(), "repo_tags", ["align:evidence"]), "empty list")
    rejected("tag shape", top_level(inspection(), "repo_tags", ""), "empty list")
    rejected(
        "toolchain version",
        toolchain_value(inspection(), "rustc", O(("version", "rustc-2"), ("executable_sha256", H64))),
        "toolchain identity",
    )
    rejected(
        "toolchain digest",
        toolchain_value(inspection(), "llvm", O(("version", "llvm-1"), ("executable_sha256", "1" * 64))),
        "toolchain identity",
    )
    rejected("cache manifest", top_level(inspection(), "cargo_cache_manifest_sha256", "1" * 64), "does not match")
    rejected("Cargo config", top_level(inspection(), "cargo_config_sha256", "1" * 64), "does not match")
    reordered = O(*reversed(inspection().pairs))
    rejected("inspection order", reordered, "wrong member order")
    missing_tool = O(*((key, member) for key, member in inspection()["toolchain"].pairs if key != "git"))
    rejected("missing toolchain member", top_level(inspection(), "toolchain", missing_tool), "wrong member order")

    bad_profile = {**PROFILE, "image": {**PROFILE["image"], "platform": "linux/arm64"}}
    rejected("profile platform", inspection(), "profile.image.platform", bad_profile)


main()
print("image qualification checks passed")
PY
