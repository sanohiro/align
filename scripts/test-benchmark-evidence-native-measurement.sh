#!/usr/bin/env bash
# Deterministic owner for the native benchmark measurement adapter.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - <<'PY'
import hashlib
from dataclasses import replace

from scripts.benchmark_evidence import container
from scripts.benchmark_evidence import native_host
from scripts.benchmark_evidence import native_measurement as measurement
from scripts.benchmark_evidence import schedule


H64 = "0" * 64
PROFILE = {
    "image": {
        "registry_digest": "sha256:" + "1" * 64,
        "local_image_id": H64,
        "platform": "linux/amd64",
    },
    "machine": {
        "architecture": "x86_64",
        "benchmark_cpu_set": "0-7",
        "numa_set": "0",
    },
    "docker": {
        "cgroup_driver": "cgroupfs",
        "cgroup_parent": "/",
    },
    "capture_limits": {
        "stdout_max_bytes": 65536,
        "stderr_max_bytes": 32768,
        "stderr_tail_max_bytes": 4096,
    },
    "phase_timeouts": {
        "prepare_ns": 60_000_000_000,
        "warmup_ns": 60_000_000_000,
        "sample_ns": 60_000_000_000,
    },
}


def expect_error(call, fragment):
    try:
        call()
    except measurement.NativeMeasurementError as exc:
        assert fragment in str(exc), (fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid native measurement input: {fragment}")


def expect_schedule_error(call, fragment):
    try:
        call()
    except schedule.ScheduleError as exc:
        assert fragment in str(exc), (fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid schedule transition: {fragment}")


def expect_container_error(call, fragment):
    try:
        call()
    except container.ContainerError as exc:
        assert fragment in str(exc), (fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid container request: {fragment}")


def workspace(index, digest=None):
    root = f"/evidence/product-{index}"
    return measurement.ChildWorkspace(
        source=f"{root}/source",
        target=f"{root}/target",
        work=f"{root}/work",
        cargo_home=f"{root}/cargo",
        toolchain=f"{root}/toolchain",
        artifact_manifest_sha256=digest,
    )


def config(digest=None):
    return measurement.NativeMeasurementConfig(
        profile=PROFILE,
        docker_client_sha256=H64,
        workspaces={
            product: workspace(index, digest)
            for index, product in enumerate(measurement.PRODUCTS)
        },
    )


def prepare_output(digest):
    return (
        f"{measurement.PREPARED_PATH}\n"
        f"artifact-manifest-sha256: {digest}\n"
    ).encode("ascii")


def decode_output():
    return (
        "target: native\n"
        + measurement.DECODE_TITLE
        + "\n"
        + measurement.DECODE_HEADER
        + "\n"
        + "{:>9} {:>8} | {:>9} {:>9} {:>9} | {:>9} {:>9} {:>9}\n".format(
            10000, 498, "0.100", "0.200", "2.00x", "0.080", "0.160", "2.00x"
        )
        + "{:>9} {:>8} | {:>9} {:>9} {:>9} | {:>9} {:>9} {:>9}\n".format(
            100000, 5083, "1.000", "2.000", "2.00x", "0.800", "1.600", "2.00x"
        )
        + "{:>9} {:>8} | {:>9} {:>9} {:>9} | {:>9} {:>9} {:>9}\n".format(
            1000000, 51814, "10.000", "20.000", "2.00x", "8.000", "16.000", "2.00x"
        )
    ).encode("utf-8")


def soa_output():
    return (
        "target: native\n"
        + measurement.SOA_TITLE
        + "\n"
        + measurement.SOA_HEADER
        + "\n"
        + "{:>9}  {:>8}  {:>9}  {:>9}  {:>9}  {:>9}  {:>7.2f}x  {:>7.2f}x  {:>8.2f}x\n".format(
            10000, 498, "0.100", "0.120", "0.080", "0.200", 2.0, 1.67, 2.5
        )
        + "{:>9}  {:>8}  {:>9}  {:>9}  {:>9}  {:>9}  {:>7.2f}x  {:>7.2f}x  {:>8.2f}x\n".format(
            100000, 5083, "1.000", "1.200", "0.800", "2.000", 2.0, 1.67, 2.5
        )
        + "{:>9}  {:>8}  {:>9}  {:>9}  {:>9}  {:>9}  {:>7.2f}x  {:>7.2f}x  {:>8.2f}x\n".format(
            1000000, 51814, "10.000", "12.000", "8.000", "20.000", 2.0, 1.67, 2.5
        )
    ).encode("utf-8")


def runner_for(payload, calls, *, actual=H64, stderr=b""):
    def runner(commands, expected, **kwargs):
        calls.append((commands, expected, kwargs))
        return (native_host.CommandCapture(payload, stderr),), actual

    return runner


plans = schedule.full_schedule()
assert len(plans) == 48
assert measurement.DECODE_HEADER == (
    "  records  json KB |    A-full   rs-full     full× |    A-proj   rs-proj     proj×"
)
assert measurement.SOA_HEADER == (
    "  records   json KB     soa ms     aos ms    proj ms    rust ms  soa/rust  aos/rust  proj/rustP"
)

prepare_digest = "a" * 64
prepare_calls = []
prepared = measurement.execute_child(
    config(),
    plans[0],
    "1" * 64,
    runner=runner_for(prepare_output(prepare_digest), prepare_calls),
    clock=iter((10, 25)).__next__,
)
assert prepared.phase == "prepare"
assert prepared.build_attempted is True
assert prepared.artifact_manifest_sha256 == prepare_digest
assert prepared.measurement is None
assert prepared.elapsed_ns == 15
assert prepared.stdout_sha256 == hashlib.sha256(prepare_output(prepare_digest)).hexdigest()
assert prepared.stderr_sha256 == hashlib.sha256(b"").hexdigest()
assert prepared.stderr_tail_hex == ""
assert len(prepare_calls) == 1
prepare_argv = prepare_calls[0][0][0]
assert prepare_calls[0][1] == H64
assert prepare_calls[0][2] == {
    "timeout_seconds": 60.0,
    "stdout_limit": 65536,
    "stderr_limit": 32768,
}
assert prepare_argv[-4:] == (
    "sha256:" + H64,
    "/src/bench/json_decode/run.sh",
    "prepare",
    "native",
)
assert "--env=ALIGN_BENCH_ARTIFACT_MANIFEST_SHA256=" + prepare_digest not in prepare_argv

native_workspaces = {
    product: replace(item, artifact_manifest_sha256=prepare_digest)
    for product, item in config().workspaces.items()
}
native_config = replace(config(), workspaces=native_workspaces)
alias_workspaces = dict(config().workspaces)
alias_workspaces[measurement.PRODUCTS[1]] = replace(
    alias_workspaces[measurement.PRODUCTS[1]],
    source=alias_workspaces[measurement.PRODUCTS[0]].source,
)
expect_error(
    lambda: measurement.NativeMeasurementConfig(
        profile=PROFILE,
        docker_client_sha256=H64,
        workspaces=alias_workspaces,
    ),
    "workspace paths must be distinct",
)
oversized_stderr_profile = {
    **PROFILE,
    "capture_limits": {
        **PROFILE["capture_limits"],
        "stderr_max_bytes": measurement._MAX_OUTPUT + 1,
    },
}
expect_error(
    lambda: measurement.NativeMeasurementConfig(
        profile=oversized_stderr_profile,
        docker_client_sha256=H64,
        workspaces=config().workspaces,
    ),
    "capture limit exceeds",
)
native_calls = []
native_plan = plans[4]
native_result = measurement.execute_child(
    native_config,
    native_plan,
    "2" * 64,
    runner=runner_for(decode_output(), native_calls),
    clock=iter((100, 160)).__next__,
)
assert native_result.phase == "warmup"
assert native_result.build_attempted is False
assert native_result.artifact_manifest_sha256 == prepare_digest
assert native_result.elapsed_ns == 60
assert native_result.measurement is not None
assert native_result.measurement.fields == (("A-full", 10000), ("A-proj", 8000))
native_argv = native_calls[0][0][0]
assert native_calls[0][2] == {
    "timeout_seconds": 60.0,
    "stdout_limit": 65536,
    "stderr_limit": 32768,
}
assert native_argv[-3:] == (
    "sha256:" + H64,
    "/src/bench/json_decode/run.sh",
    "native",
)
assert "--env=ALIGN_BENCH_ARTIFACT_MANIFEST_SHA256=" + prepare_digest in native_argv

soa = measurement.parse_native_output(soa_output(), "json_soa")
assert soa.fields == (("soa ms", 10000), ("aos ms", 12000), ("proj ms", 8000))
assert tuple(row.records for row in soa.rows) == (10000, 100000, 1000000)

assert measurement.parse_prepare_output(prepare_output("b" * 64)) == "b" * 64
expect_error(lambda: measurement.parse_prepare_output(b"/work/prepared/x\n"), "fixed manifest path")
expect_error(
    lambda: measurement.parse_native_output(decode_output().replace(b"  10000", b"\t10000", 1), "json_decode"),
    "forbidden control",
)
expect_error(
    lambda: measurement.parse_native_output(
        decode_output().replace(b"1000000", b"10000", 1), "json_decode"
    ),
    "wrong record count",
)
expect_error(
    lambda: measurement.parse_native_output(decode_output().replace(b"2.00x", b"2.00", 1), "json_decode"),
    "ratio",
)
expect_error(
    lambda: measurement.parse_native_output(decode_output().replace(b"0.100", b"0.000", 1), "json_decode"),
    "positive u64",
)
expect_error(
    lambda: measurement.parse_native_output(
        decode_output().replace(b"0.100", b"18446744073709551.616", 1), "json_decode"
    ),
    "positive u64",
)
expect_error(
    lambda: measurement.parse_native_output(decode_output().replace(b"A-full", b"A-proj", 1), "json_decode"),
    "wrong target, title, or header",
)
expect_error(
    lambda: measurement.parse_native_output(decode_output() + b"profile 1M:\n", "json_decode"),
    "three rows",
)
expect_error(
    lambda: measurement.parse_native_output(decode_output()[:-1] + b"x", "json_decode"),
    "one LF",
)
expect_error(
    lambda: measurement.parse_native_output(
        decode_output().replace(b"  10000", "\u00a010000".encode("utf-8"), 1), "json_decode"
    ),
    "non-ASCII",
)

missing_digest_calls = []
expect_error(
    lambda: measurement.execute_child(
        config(),
        plans[4],
        "3" * 64,
        runner=runner_for(decode_output(), missing_digest_calls),
        clock=iter((1, 2)).__next__,
    ),
    "prepare-time",
)
assert not missing_digest_calls
expect_error(
    lambda: measurement.execute_child(
        native_config,
        replace(plans[4], sequence=5),
        "4" * 64,
        runner=runner_for(decode_output(), []),
    ),
    "fixed evidence schedule",
)

hash_calls = []
expect_error(
    lambda: measurement.execute_child(
        native_config,
        native_plan,
        "5" * 64,
        runner=runner_for(decode_output(), hash_calls, actual="f" * 64),
        clock=iter((1, 2)).__next__,
    ),
    "changed during measurement",
)
assert len(hash_calls) == 1
expect_error(
    lambda: measurement.execute_child(
        native_config,
        native_plan,
        "6" * 64,
        runner=runner_for(decode_output(), [], stderr=b"warning\n"),
        clock=iter((1, 2)).__next__,
    ),
    "stderr is not empty",
)
expect_error(
    lambda: measurement.execute_child(
        native_config,
        native_plan,
        "7" * 64,
        runner=lambda _commands, _expected, **_kwargs: (_ for _ in ()).throw(RuntimeError("boom")),
        clock=iter((1, 2)).__next__,
    ),
    "before a child result",
)

bad_launch = container.ContainerLaunch(
    child_id="9" * 64,
    source="/evidence/bad/source",
    target="/evidence/bad/target",
    work="/evidence/bad/work",
    cargo_home="/evidence/bad/cargo",
    toolchain="/evidence/bad/toolchain",
    command=("/src/bench/json_decode/run.sh", "native"),
    artifact_manifest_sha256="not-a-digest",
)
expect_container_error(lambda: container.build_argv(PROFILE, bad_launch), "artifact_manifest_sha256")

real_capture = native_host.run_command_captured(
    ("/bin/sh", "-c", "printf out; printf err >&2")
)
assert real_capture.stdout == b"out"
assert real_capture.stderr == b"err"
pinned_hash = native_host.hash_executable("/usr/bin/printf")
pinned_outputs, actual_pinned_hash = native_host.run_pinned_commands_captured(
    "/usr/bin/printf",
    (("/usr/bin/printf", "pinned"),),
    pinned_hash,
)
assert actual_pinned_hash == pinned_hash
assert pinned_outputs[0].stdout == b"pinned"
assert pinned_outputs[0].stderr == b""

state = schedule.ScheduleState()
state.start(plans[0], "8" * 64)
expect_schedule_error(lambda: state.start(plans[0], "8" * 64), "overlap")
state.finish(artifact_manifest_sha256=prepare_digest, build_attempted=True)
expect_schedule_error(lambda: state.start(plans[1], "8" * 64), "reused")

print("native measurement checks passed")
PY
