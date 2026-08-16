#!/usr/bin/env bash
# Deterministic owner for profile-bound native host qualification.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
from scripts.benchmark_evidence import canonical_json as cj
from scripts.benchmark_evidence import host


H64 = "0" * 64
MACHINE_KEYS = (
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
DOCKER_KEYS = (
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


def O(*pairs):
    return cj.Object(pairs)


MACHINE = {
    "architecture": "x86_64",
    "kernel": "6.8.0-evidence",
    "cpu_vendor": "GenuineIntel",
    "cpu_family": 6,
    "cpu_model": "143",
    "cpu_stepping": 8,
    "microcode": "0x2b000643",
    "online_cpu_set": "0-7",
    "benchmark_cpu_set": "2-3",
    "numa_set": "0",
    "minimum_memory_bytes": 8 * 1024 * 1024 * 1024,
}
DOCKER = {
    "client_version": "26.1.4",
    "client_sha256": H64,
    "daemon_version": "26.1.4",
    "daemon_architecture": "x86_64",
    "storage_driver": "overlay2",
    "cgroup_version": "2",
    "cgroup_driver": "cgroupfs",
    "cgroup_parent": "/",
    "oci_runtime": "runc-1.1.12",
}
LIMITS = [
    {
        "phase": "pre",
        "load_milli_max": 250,
        "cpu_pressure_total_us_max": 100,
        "memory_pressure_total_us_max": 200,
        "free_memory_bytes_min": 4 * 1024 * 1024 * 1024,
        "swap_read_bytes_max": 0,
        "swap_write_bytes_max": 0,
    },
    {
        "phase": "between",
        "load_milli_max": 300,
        "cpu_pressure_total_us_max": 120,
        "memory_pressure_total_us_max": 220,
        "free_memory_bytes_min": 4 * 1024 * 1024 * 1024,
        "swap_read_bytes_max": 0,
        "swap_write_bytes_max": 0,
    },
    {
        "phase": "post",
        "load_milli_max": 250,
        "cpu_pressure_total_us_max": 100,
        "memory_pressure_total_us_max": 200,
        "free_memory_bytes_min": 4 * 1024 * 1024 * 1024,
        "swap_read_bytes_max": 0,
        "swap_write_bytes_max": 0,
    },
]
PROFILE = {
    "host_id": "evidence-x86-01",
    "machine": MACHINE,
    "docker": DOCKER,
    "observation_limits": LIMITS,
}


def machine():
    return O(*((key, MACHINE[key]) for key in MACHINE_KEYS))


def docker():
    return O(*((key, DOCKER[key]) for key in DOCKER_KEYS))


def observation(limit):
    return O(
        ("phase", limit["phase"]),
        ("load_milli", min(limit["load_milli_max"], 100)),
        ("cpu_pressure_total_us", min(limit["cpu_pressure_total_us_max"], 10)),
        ("memory_pressure_total_us", min(limit["memory_pressure_total_us_max"], 20)),
        ("free_memory_bytes", limit["free_memory_bytes_min"] + 1024),
        ("swap_read_bytes", limit["swap_read_bytes_max"]),
        ("swap_write_bytes", limit["swap_write_bytes_max"]),
    )


def inspection():
    return O(
        ("host_id", PROFILE["host_id"]),
        ("machine", machine()),
        ("memory_bytes", 16 * 1024 * 1024 * 1024),
        ("cpu_quota_milli", 0),
        ("docker", docker()),
        ("observations", [observation(limit) for limit in LIMITS]),
    )


def replace_top(value, key, replacement):
    return O(*((name, replacement if name == key else member) for name, member in value.pairs))


def replace_nested(value, parent, key, replacement):
    nested = value[parent]
    updated = O(*((name, replacement if name == key else member) for name, member in nested.pairs))
    return replace_top(value, parent, updated)


def replace_observation(value, index, key, replacement):
    observations = list(value["observations"])
    observations[index] = O(
        *((name, replacement if name == key else member) for name, member in observations[index].pairs)
    )
    return replace_top(value, "observations", observations)


def rejected(label, value, fragment, profile=PROFILE):
    try:
        host.qualify(profile, value)
    except host.HostQualificationError as exc:
        assert fragment in str(exc), (label, fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid host inspection: {label}")


def main():
    qualified = host.qualify(PROFILE, inspection())
    assert qualified.host_id == "evidence-x86-01"
    assert qualified.architecture == "x86_64"
    assert qualified.cpu_family == 6
    assert qualified.memory_bytes == 16 * 1024 * 1024 * 1024
    assert qualified.cpu_quota_milli == 0
    assert qualified.docker.client_version == "26.1.4"
    assert qualified.docker.cgroup_driver == "cgroupfs"
    assert qualified.docker.cgroup_parent == "/"
    assert tuple(item.phase for item in qualified.observations) == ("pre", "between", "post")

    systemd_docker = {**DOCKER, "cgroup_driver": "systemd", "cgroup_parent": "-.slice"}
    systemd_profile = {**PROFILE, "docker": systemd_docker}
    systemd_inspection = replace_top(
        inspection(),
        "docker",
        O(*((key, systemd_docker[key]) for key in DOCKER_KEYS)),
    )
    systemd_qualified = host.qualify(systemd_profile, systemd_inspection)
    assert systemd_qualified.docker.cgroup_driver == "systemd"
    assert systemd_qualified.docker.cgroup_parent == "-.slice"

    rejected("host ID", replace_top(inspection(), "host_id", "other-host"), "inspection.host_id")
    rejected("architecture", replace_nested(inspection(), "machine", "architecture", "aarch64"), "machine.architecture")
    rejected("CPU family", replace_nested(inspection(), "machine", "cpu_family", 7), "machine.cpu_family")
    rejected("machine order", replace_top(inspection(), "machine", O(*reversed(machine().pairs))), "wrong member order")
    rejected("memory minimum", replace_top(inspection(), "memory_bytes", 1), "below profile minimum")
    rejected("CPU quota", replace_top(inspection(), "cpu_quota_milli", 1), "must be zero")
    rejected("Docker version", replace_nested(inspection(), "docker", "daemon_version", "27.0.0"), "inspection.docker.daemon_version")
    rejected("Docker client digest", replace_nested(inspection(), "docker", "client_sha256", "1" * 64), "inspection.docker.client_sha256")
    rejected("Docker architecture", replace_nested(inspection(), "docker", "daemon_architecture", "aarch64"), "daemon_architecture")
    rejected("Docker cgroup driver", replace_nested(inspection(), "docker", "cgroup_driver", "systemd"), "cgroup_parent")
    rejected("Docker cgroup parent", replace_nested(inspection(), "docker", "cgroup_parent", "relative"), "cgroup_parent")
    rejected("Docker order", replace_top(inspection(), "docker", O(*reversed(docker().pairs))), "wrong member order")
    rejected("observation count", replace_top(inspection(), "observations", []), "exactly three")
    reordered = list(inspection()["observations"])
    reordered[0], reordered[1] = reordered[1], reordered[0]
    rejected("observation phase order", replace_top(inspection(), "observations", reordered), "phase")
    rejected("load limit", replace_observation(inspection(), 0, "load_milli", 251), "exceeds profile limit")
    rejected("CPU pressure", replace_observation(inspection(), 0, "cpu_pressure_total_us", 101), "exceeds profile limit")
    rejected("memory pressure", replace_observation(inspection(), 1, "memory_pressure_total_us", 221), "exceeds profile limit")
    rejected("free memory", replace_observation(inspection(), 2, "free_memory_bytes", 1), "below profile minimum")
    rejected("swap read", replace_observation(inspection(), 0, "swap_read_bytes", 1), "exceeds profile limit")
    rejected("swap write", replace_observation(inspection(), 2, "swap_write_bytes", 1), "exceeds profile limit")
    rejected("missing observation member", replace_observation(inspection(), 0, "load_milli", None), "unsigned integer")
    bad_profile = {**PROFILE, "machine": {**MACHINE, "architecture": "aarch64"}}
    rejected("profile architecture", inspection(), "profile.machine.architecture", bad_profile)


main()
print("host qualification checks passed")
PY
