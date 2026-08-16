#!/usr/bin/env bash
# Deterministic owner for benchmark_evidence_native_host_matrix.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - <<'PY'
import json
import os
import tempfile
import time
from types import SimpleNamespace

from scripts.benchmark_evidence import canonical_json as cj
from scripts.benchmark_evidence import native_host


H64 = "0" * 64


def expect_error(call, fragment):
    try:
        call()
    except native_host.NativeHostError as exc:
        assert fragment in str(exc), (fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid native host input: {fragment}")


PROFILE = {
    "host_id": "evidence-x86-01",
    "machine": {
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
    },
    "docker": {
        "client_version": "26.1.4",
        "client_sha256": H64,
        "daemon_version": "26.1.4",
        "daemon_architecture": "x86_64",
        "storage_driver": "overlay2",
        "cgroup_version": "2",
        "oci_runtime": "runc-1.1.12",
    },
    "observation_limits": [
        {
            "phase": phase,
            "load_milli_max": 250,
            "cpu_pressure_total_us_max": 100,
            "memory_pressure_total_us_max": 200,
            "free_memory_bytes_min": 4 * 1024 * 1024 * 1024,
            "swap_read_bytes_max": 0,
            "swap_write_bytes_max": 0,
        }
        for phase in ("pre", "between", "post")
    ],
}

DOCKER_VERSION = {
    "Client": {"Version": "26.1.4"},
    "Server": {"Version": "26.1.4", "Arch": "amd64"},
}
DOCKER_INFO = {
    "Driver": "overlay2",
    "CgroupVersion": "2",
    "DefaultRuntime": "runc-1.1.12",
}

FILES = {
    native_host.HOST_ID_PATH: b"evidence-x86-01\n",
    native_host.BENCHMARK_CPU_SET_PATH: b"2-3\n",
    native_host.CPUINFO_PATH: (
        b"processor : 0\n"
        b"vendor_id : GenuineIntel\n"
        b"cpu family : 6\n"
        b"model : 143\n"
        b"stepping : 8\n"
        b"microcode : 0x2b000643\n\n"
    ),
    native_host.MEMINFO_PATH: (
        b"MemTotal:       16777216 kB\n"
        b"MemAvailable:    8388608 kB\n"
    ),
    native_host.VMSTAT_PATH: b"pswpin 0\npswpout 0\n",
    native_host.LOADAVG_PATH: b"0.010 0.020 0.030 1/100 123\n",
    native_host.CPU_PRESSURE_PATH: b"some avg10=0.00 avg60=0.00 avg300=0.00 total=10\n",
    native_host.MEMORY_PRESSURE_PATH: b"some avg10=0.00 avg60=0.00 avg300=0.00 total=20\n",
    native_host.ONLINE_CPU_SET_PATH: b"0-7\n",
    native_host.NUMA_SET_PATH: b"0\n",
    native_host.CGROUP_V2_CPU_MAX_PATH: b"max 100000\n",
}


def reader(path):
    if path not in FILES:
        raise native_host.NativeHostError(f"missing fixture source: {path}")
    return FILES[path]


calls = []


def runner(argv):
    calls.append(argv)
    if argv == native_host.DOCKER_VERSION_ARGV:
        return json.dumps(DOCKER_VERSION, separators=(",", ":")).encode("utf-8")
    if argv == native_host.DOCKER_INFO_ARGV:
        return json.dumps(DOCKER_INFO, separators=(",", ":")).encode("utf-8")
    raise AssertionError(f"unexpected trusted command: {argv}")


qualified = native_host.qualify(
    PROFILE,
    reader=reader,
    runner=runner,
    hasher=lambda path: H64 if path == native_host.DOCKER else "1" * 64,
    uname=lambda: SimpleNamespace(machine="x86_64", release="6.8.0-evidence"),
    page_size=4096,
)
assert qualified.host_id == "evidence-x86-01"
assert qualified.architecture == "x86_64"
assert qualified.memory_bytes == 16 * 1024 * 1024 * 1024
assert qualified.cpu_quota_milli == 0
assert qualified.docker.client_sha256 == H64
assert qualified.docker.daemon_architecture == "x86_64"
assert tuple(calls) == (native_host.DOCKER_VERSION_ARGV, native_host.DOCKER_INFO_ARGV)
assert native_host._FIXED_ENV["DOCKER_CONFIG"] == native_host.DOCKER_CONFIG
assert native_host._FIXED_ENV["DOCKER_HOST"] == native_host.DOCKER_HOST

inspection = native_host.inspect(
    PROFILE,
    reader=reader,
    runner=runner,
    hasher=lambda _path: H64,
    uname=lambda: SimpleNamespace(machine="x86_64", release="6.8.0-evidence"),
    page_size=4096,
)
assert isinstance(inspection, cj.Object)
assert tuple(inspection) == (
    "host_id",
    "machine",
    "memory_bytes",
    "cpu_quota_milli",
    "docker",
    "observations",
)
assert [item["phase"] for item in inspection["observations"]] == ["pre", "between", "post"]


def with_file(path, value):
    updated = dict(FILES)
    updated[path] = value

    def updated_reader(candidate):
        if candidate not in updated:
            raise native_host.NativeHostError(f"missing fixture source: {candidate}")
        return updated[candidate]

    return updated_reader


def qualify_with(reader_value=reader, runner_value=runner, uname_value="x86_64", profile=PROFILE):
    return native_host.qualify(
        profile,
        reader=reader_value,
        runner=runner_value,
        hasher=lambda _path: H64,
        uname=lambda: SimpleNamespace(machine=uname_value, release="6.8.0-evidence"),
        page_size=4096,
    )


def forbidden_runner(_argv):
    raise AssertionError("Docker was invoked before native host state was eligible")


expect_error(
    lambda: qualify_with(uname_value="aarch64", runner_value=forbidden_runner),
    "host architecture must be x86_64 before Docker qualification",
)
expect_error(
    lambda: qualify_with(reader_value=with_file(native_host.BENCHMARK_CPU_SET_PATH, b"0-1\n")),
    "does not match the profile",
)
expect_error(
    lambda: qualify_with(
        reader_value=with_file(native_host.CGROUP_V2_CPU_MAX_PATH, b"100000 100000\n"),
        runner_value=forbidden_runner,
    ),
    "CPU quota must be zero before Docker qualification",
)
expect_error(
    lambda: qualify_with(reader_value=with_file(native_host.ONLINE_CPU_SET_PATH, b"0-2\n")),
    "not a subset",
)
expect_error(
    lambda: qualify_with(runner_value=lambda _argv: b'{"Client":{"Version":"26.1.4"},"Client":{"Version":"26.1.4"}}'),
    "duplicate object member",
)
expect_error(
    lambda: qualify_with(reader_value=with_file(native_host.CPU_PRESSURE_PATH, b"full total=1\n")),
    "no PSI some total",
)
expect_error(
    lambda: qualify_with(reader_value=with_file(native_host.MEMINFO_PATH, b"MemTotal: 1 kB\nMemAvailable: 2 kB\n")),
    "MemAvailable exceeds MemTotal",
)

pressure_with_duplicate_some = with_file(
    native_host.CPU_PRESSURE_PATH,
    b"some total=1\nsome total=2\n",
)
expect_error(
    lambda: qualify_with(reader_value=pressure_with_duplicate_some),
    "repeated PSI some lines",
)

memory_reads = 0


def drifting_memory_reader(path):
    global memory_reads
    if path == native_host.MEMINFO_PATH:
        memory_reads += 1
        if memory_reads > 1:
            return b"MemTotal: 16777216 kB\nMemAvailable: 16777217 kB\n"
    return reader(path)


expect_error(
    lambda: qualify_with(reader_value=drifting_memory_reader),
    "MemAvailable exceeds MemTotal",
)

v1_files = dict(FILES)
del v1_files[native_host.CGROUP_V2_CPU_MAX_PATH]
v1_files[native_host.CGROUP_V1_CPU_QUOTA_PATH] = b"-1\n"
v1_files[native_host.CGROUP_V1_CPU_PERIOD_PATH] = b"100000\n"


def mapping_reader(mapping):
    def read(path):
        if path not in mapping:
            raise native_host.NativeHostSourceMissing(f"missing fixture source: {path}")
        return mapping[path]

    return read


assert native_host._quota_milli(mapping_reader(v1_files)) == 0
malformed_v2 = dict(v1_files)
malformed_v2[native_host.CGROUP_V2_CPU_MAX_PATH] = b"100000\n"
expect_error(
    lambda: native_host._quota_milli(mapping_reader(malformed_v2)),
    "wrong shape",
)

small_quota = dict(FILES)
small_quota[native_host.CGROUP_V2_CPU_MAX_PATH] = b"1 100000\n"
expect_error(
    lambda: qualify_with(reader_value=mapping_reader(small_quota), runner_value=forbidden_runner),
    "CPU quota must be zero before Docker qualification",
)


swap_reads = 0


def resetting_counter_reader(path):
    global swap_reads
    if path == native_host.VMSTAT_PATH:
        swap_reads += 1
        if swap_reads == 1:
            return b"pswpin 1\npswpout 0\n"
        return b"pswpin 0\npswpout 0\n"
    return reader(path)


expect_error(
    lambda: qualify_with(reader_value=resetting_counter_reader),
    "counter reset before observation",
)

expect_error(
    lambda: native_host._parse_cpu_set("0-0", "fixture CPU set"),
    "singleton range",
)
expect_error(
    lambda: native_host._swap_bytes(reader, 0),
    "page size is zero",
)

expect_error(
    lambda: native_host.run_command(("/usr/bin/false",)),
    "exited nonzero",
)
assert native_host.run_command(("/usr/bin/printf", "native-ok")) == b"native-ok"
expect_error(
    lambda: native_host.run_command(("/usr/bin/printf", "12345"), output_limit=4),
    "exceeded the fixed limit",
)
expect_error(
    lambda: native_host.run_command(("/usr/bin/sleep", "1"), timeout_seconds=0.01),
    "timed out",
)
expect_error(
    lambda: native_host.run_command((1,)),
    "argument 0 is invalid",
)
expect_error(
    lambda: native_host.run_command(("/usr/bin/true",), timeout_seconds=float("nan")),
    "timeout must be positive",
)

with tempfile.TemporaryDirectory() as directory:
    regular = os.path.join(directory, "regular")
    with open(regular, "wb") as stream:
        stream.write(b"native-source")
    link = os.path.join(directory, "link")
    os.symlink(regular, link)
    expect_error(
        lambda: native_host.read_no_follow(link),
        "cannot open native source",
    )
    child_directory = os.path.join(directory, "directory")
    os.mkdir(child_directory)
    expect_error(
        lambda: native_host.read_no_follow(child_directory),
        "not a regular file",
    )

with tempfile.TemporaryDirectory() as directory:
    marker = os.path.join(directory, "descendant-marker")
    command = f"(sleep 1; printf leaked > {marker}) & exit 0"
    expect_error(
        lambda: native_host.run_command(("/bin/sh", "-c", command), timeout_seconds=0.05),
        "timed out",
    )
    time.sleep(1.2)
    assert not os.path.exists(marker), "a descendant survived command-group cleanup"

print("native host qualification checks passed")
PY
