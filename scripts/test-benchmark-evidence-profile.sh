#!/usr/bin/env bash
# Deterministic owner for the fixed benchmark-evidence host profile.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
import base64
import hashlib

from scripts.benchmark_evidence import canonical_json as cj
from scripts.benchmark_evidence import profile


H64 = "0" * 64
KEY = base64.b64encode(b"\x00" * 32).decode("ascii")
FINGERPRINT = "SHA256:" + KEY.rstrip("=")


def O(*pairs):
    return cj.Object(pairs)


def executable(version):
    return O(("version", version), ("executable_sha256", H64))


def valid_profile():
    return O(
        ("schema", profile.PROFILE_SCHEMA),
        ("profile_id", "linux-x86_64-v1"),
        ("target_ref", profile.TARGET_REF),
        ("host_id", "evidence-host-1"),
        (
            "machine",
            O(
                ("architecture", "x86_64"),
                ("kernel", "6.1.0"),
                ("cpu_vendor", "AuthenticAMD"),
                ("cpu_family", 25),
                ("cpu_model", "33"),
                ("cpu_stepping", 2),
                ("microcode", "0x1234"),
                ("online_cpu_set", "0-15"),
                ("benchmark_cpu_set", "0-7"),
                ("numa_set", "0"),
                ("minimum_memory_bytes", 1 << 30),
            ),
        ),
        (
            "observation_limits",
            [
                O(
                    ("phase", phase),
                    ("load_milli_max", 500),
                    ("cpu_pressure_total_us_max", 0),
                    ("memory_pressure_total_us_max", 0),
                    ("free_memory_bytes_min", 1 << 29),
                    ("swap_read_bytes_max", 0),
                    ("swap_write_bytes_max", 0),
                )
                for phase in ("pre", "between", "post")
            ],
        ),
        (
            "docker",
            O(
                ("client_version", "20.10.7"),
                ("client_sha256", H64),
                ("daemon_version", "20.10.7"),
                ("daemon_architecture", "x86_64"),
                ("storage_driver", "overlay2"),
                ("cgroup_version", "2"),
                ("oci_runtime", "runc-1.1"),
            ),
        ),
        (
            "image",
            O(
                ("registry_digest", "sha256:" + H64),
                ("local_image_id", H64),
                ("config_digest", H64),
                ("platform", "linux/amd64"),
            ),
        ),
        (
            "toolchain",
            O(
                ("python", executable("Python-3.13.5")),
                ("git", executable("git-2.45.0")),
                ("cargo", executable("cargo-1.88.0")),
                ("rustc", executable("rustc-1.88.0")),
                ("llvm", executable("llvm-22.1.0")),
                ("cc", executable("clang-20.1.0")),
                ("linker", executable("ld.lld-20.1.0")),
                ("ssh_keygen", executable("OpenSSH_9.9")),
            ),
        ),
        ("cargo_cache_manifest_sha256", H64),
        ("cargo_config_sha256", H64),
        (
            "capture_limits",
            O(("stdout_max_bytes", 65536), ("stderr_max_bytes", 65536), ("stderr_tail_max_bytes", 4096)),
        ),
        (
            "phase_timeouts",
            O(
                ("prepare_ns", 60_000_000_000),
                ("warmup_ns", 60_000_000_000),
                ("sample_ns", 60_000_000_000),
                ("monitor_ns", 10_000_000_000),
                ("cleanup_ns", 10_000_000_000),
            ),
        ),
        (
            "signing",
            O(("key_type", "ssh-ed25519"), ("public_key_base64", KEY), ("fingerprint", FINGERPRINT)),
        ),
        (
            "schedule",
            O(
                ("threshold_numerator", 105),
                ("threshold_denominator", 100),
                ("warmup_count", 1),
                ("pair_count", 10),
            ),
        ),
        ("benchmarks", list(profile.BENCHMARKS)),
        ("fields", list(profile.FIELDS)),
    )


def update(value, path, replacement):
    if len(path) == 1:
        return O(*((key, replacement if key == path[0] else member) for key, member in value.pairs))
    child = update(value[path[0]], path[1:], replacement)
    return O(*((key, child if key == path[0] else member) for key, member in value.pairs))


def rejected(value, fragment):
    try:
        profile.encode_profile(value)
    except profile.ProfileError as exc:
        assert fragment in str(exc), (fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid profile: {fragment}")


def main():
    value = valid_profile()
    raw = profile.encode_profile(value)
    assert raw.endswith(b"\n")
    assert profile.decode_profile(raw).pairs == value.pairs
    assert profile.profile_sha256(raw) == hashlib.sha256(raw).hexdigest()
    assert cj.decode(raw).pairs == value.pairs

    rejected(update(value, ("machine", "architecture"), "aarch64"), "must be x86_64")
    rejected(update(value, ("target_ref",), "refs/heads/release"), "refs/heads/main")
    rejected(update(value, ("schedule", "threshold_numerator"), 106), "must be 105")
    rejected(update(value, ("schedule", "pair_count"), 9), "must be 10")
    rejected(update(value, ("image", "platform"), "linux/arm64"), "linux/amd64")
    rejected(update(value, ("docker", "daemon_architecture"), "aarch64"), "must be x86_64")
    rejected(update(value, ("observation_limits",), [*value["observation_limits"][1:], value["observation_limits"][0]]), "phase order")
    rejected(update(value, ("signing", "public_key_base64"), "not-base64"), "public_key_base64")
    rejected(update(value, ("signing", "fingerprint"), "SHA256:short"), "fingerprint")
    rejected(update(value, ("toolchain", "git", "executable_sha256"), "g" * 64), "executable_sha256")

    reordered = O(*reversed(value.pairs))
    rejected(reordered, "wrong member order")
    try:
        profile.decode_profile(raw[:-1])
    except profile.ProfileError as exc:
        assert "canonical" in str(exc)
    else:
        raise AssertionError("accepted profile without final LF")
    try:
        profile.decode_profile(b'{"schema":"x","schema":"y"}\n')
    except profile.ProfileError as exc:
        assert "duplicate" in str(exc)
    else:
        raise AssertionError("accepted duplicate profile member")

    oversized = raw + (b" " * (1 << 20))
    try:
        profile.decode_profile(oversized)
    except profile.ProfileError as exc:
        assert "fixed limit" in str(exc)
    else:
        raise AssertionError("accepted oversized profile")


main()
PY
