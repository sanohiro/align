#!/usr/bin/env bash
# Deterministic owner for benchmark_evidence_native_image_matrix.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
from scripts.benchmark_evidence import canonical_json as cj
from scripts.benchmark_evidence import container
from scripts.benchmark_evidence import native_host
from scripts.benchmark_evidence import native_image


H64 = "0" * 64
ONE64 = "1" * 64
DIGEST = "sha256:" + H64
REGISTRY = "sha256:" + ONE64
REPOSITORY = "registry.example/align/evidence"
TOOLCHAIN_KEYS = ("python", "git", "cargo", "rustc", "llvm", "cc", "linker", "ssh_keygen")


def O(*pairs):
    return cj.Object(pairs)


def executable(key):
    return O(("version", key + "-1"), ("executable_sha256", H64))


PROFILE = {
    "profile_id": "linux-x86_64-v1",
    "image": {
        "registry_digest": REGISTRY,
        "local_image_id": H64,
        "config_digest": H64,
        "platform": "linux/amd64",
    },
    "machine": {
        "architecture": "x86_64",
        "benchmark_cpu_set": "2-3",
        "numa_set": "0",
    },
    "docker": {
        "client_sha256": H64,
        "cgroup_driver": "cgroupfs",
        "cgroup_parent": "/",
    },
    "toolchain": {key: executable(key) for key in TOOLCHAIN_KEYS},
    "cargo_cache_manifest_sha256": H64,
    "cargo_config_sha256": H64,
}


def host_bytes(**overrides):
    value = O(
        ("image_id", DIGEST),
        ("repo_digests", [REPOSITORY + "@" + REGISTRY]),
        ("repo_tags", []),
        ("os", "linux"),
        ("architecture", "amd64"),
    )
    return cj.encode(O(*((key, overrides.get(key, member)) for key, member in value.pairs)))


def self_bytes(**overrides):
    value = O(
        ("config_digest", DIGEST),
        ("toolchain", O(*((key, executable(key)) for key in TOOLCHAIN_KEYS))),
        ("cargo_cache_manifest_sha256", H64),
        ("cargo_config_sha256", H64),
    )
    return cj.encode(O(*((key, overrides.get(key, member)) for key, member in value.pairs)))


def expect_error(call, fragment):
    try:
        call()
    except native_image.NativeImageError as exc:
        assert fragment in str(exc), (fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid native image operation: {fragment}")


def expect_container_error(call, fragment):
    try:
        call()
    except container.ContainerError as exc:
        assert fragment in str(exc), (fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid image container operation: {fragment}")


def fixture_runner(host_value=None, self_value=None, *, failures=None, events=None):
    host_value = host_bytes() if host_value is None else host_value
    self_value = self_bytes() if self_value is None else self_value
    failures = {} if failures is None else dict(failures)
    events = [] if events is None else events
    host_argv = native_image.host_inspect_argv(PROFILE)
    run_argv = container.build_image_inspection_argv(PROFILE, native_image._child_id(PROFILE))
    cleanup_argv = container.build_image_inspection_cleanup_argv(PROFILE, native_image._child_id(PROFILE))

    def runner(argv):
        events.append(("run", argv))
        if argv in failures:
            failure = failures[argv]
            if isinstance(failure, BaseException):
                raise failure
            raise AssertionError(failure)
        if argv == host_argv:
            return host_value
        if argv == run_argv:
            return self_value
        if argv == cleanup_argv:
            return b"removed\n"
        raise AssertionError(f"unexpected native image argv: {argv!r}")

    return runner, events, host_argv, run_argv, cleanup_argv


def main():
    runner, events, host_argv, run_argv, cleanup_argv = fixture_runner()

    def hasher(path):
        events.append(("hash", path))
        assert path == native_image.DOCKER
        return H64

    qualified = native_image.qualify(PROFILE, runner=runner, hasher=hasher)
    assert qualified.registry_digest == REGISTRY
    assert qualified.image_id == DIGEST
    assert qualified.config_digest == DIGEST
    assert qualified.platform == "linux/amd64"
    assert qualified.toolchain[0] == ("python", "python-1", H64)
    assert len(qualified.toolchain) == len(TOOLCHAIN_KEYS)
    assert qualified.cargo_cache_manifest_sha256 == H64
    assert qualified.cargo_config_sha256 == H64
    assert [event[0] for event in events] == ["hash", "run", "hash", "run"]
    assert events[1][1] == host_argv
    assert events[3][1] == run_argv

    assert host_argv[:6] == (
        native_image.DOCKER,
        "--config",
        native_image.DOCKER_CONFIG,
        "--host",
        native_image.DOCKER_HOST,
        "image",
    )
    assert host_argv[-2] == "--format=" + native_image.IMAGE_INSPECT_FORMAT
    assert host_argv[-1] == DIGEST
    assert "--pull=never" not in host_argv
    assert run_argv[0:3] == (container.DOCKER, "run", "--rm")
    assert "--pull=never" in run_argv
    assert "--network=none" in run_argv
    assert "--read-only" in run_argv
    assert "--cap-drop=ALL" in run_argv
    assert "--security-opt=no-new-privileges" in run_argv
    assert "--security-opt=seccomp=/opt/align-evidence/seccomp.json" in run_argv
    assert "--security-opt=apparmor=align-evidence-v1" in run_argv
    assert "--user=65532:65532" in run_argv
    assert "--cpuset-cpus=2-3" in run_argv
    assert "--cpuset-mems=0" in run_argv
    assert "--cgroup-parent=/" in run_argv
    assert "--env=PATH=/toolchain/bin:/usr/bin:/bin" in run_argv
    assert "--env=LC_ALL=C" in run_argv
    assert "--env=TZ=UTC" in run_argv
    assert "--env=HOME=/nonexistent" in run_argv
    assert "--env=CARGO_NET_OFFLINE=true" in run_argv
    assert "--env=CARGO_HOME=/cargo" in run_argv
    assert "--env=TMPDIR=/tmp" in run_argv
    assert run_argv.count("--tmpfs=/tmp:rw,noexec,nosuid,nodev,size=67108864") == 1
    assert "--entrypoint=" + container.IMAGE_INSPECTION_EXECUTABLE in run_argv
    assert not any(argument.startswith("--mount=") for argument in run_argv)
    assert not any("network=host" in argument or argument == "--privileged" for argument in run_argv)
    assert run_argv[-1] == DIGEST
    assert cleanup_argv == (
        container.DOCKER,
        "container",
        "rm",
        "--force",
        "--volumes",
        "align-evidence-image-" + native_image._child_id(PROFILE),
    )
    expect_container_error(
        lambda: container.build_image_inspection_argv(PROFILE, "not-a-child-id"),
        "child_id",
    )

    production_events = []
    original_run_docker_commands = native_host.run_docker_commands

    def production_runner(commands, expected_client_hash=None, *, between=None):
        assert expected_client_hash == H64
        assert between is None
        production_events.append(tuple(commands))
        if tuple(commands) == (host_argv,):
            return (host_bytes(),), H64
        if tuple(commands) == (run_argv,):
            return (self_bytes(),), H64
        raise AssertionError(commands)

    native_host.run_docker_commands = production_runner
    try:
        native_image.inspect(PROFILE)
    finally:
        native_host.run_docker_commands = original_run_docker_commands
    assert production_events == [(host_argv,), (run_argv,)]

    host_mismatch_events = []
    host_mismatch_runner, _unused, _host, _run, _cleanup = fixture_runner(
        host_value=host_bytes(image_id=REGISTRY), events=host_mismatch_events
    )
    expect_error(
        lambda: native_image.qualify(PROFILE, runner=host_mismatch_runner, hasher=lambda _path: H64),
        "does not match profile",
    )
    assert [event[1] for event in host_mismatch_events] == [host_argv]
    registry_mismatch_events = []
    registry_mismatch_runner, _unused, _host, _run, _cleanup = fixture_runner(
        host_value=host_bytes(repo_digests=[REPOSITORY + "@" + DIGEST]), events=registry_mismatch_events
    )
    expect_error(
        lambda: native_image.inspect(PROFILE, runner=registry_mismatch_runner, hasher=lambda _path: H64),
        "registry_digest does not match profile",
    )
    assert [event[1] for event in registry_mismatch_events] == [host_argv]
    expect_error(
        lambda: native_image.inspect(
            PROFILE,
            runner=fixture_runner(host_value=host_bytes(repo_digests=[REPOSITORY + "@" + REGISTRY, REPOSITORY + "@" + REGISTRY]))[0],
            hasher=lambda _path: H64,
        ),
        "exactly one digest",
    )
    expect_error(
        lambda: native_image.inspect(
            PROFILE,
            runner=fixture_runner(host_value=host_bytes(repo_tags=["align:evidence"]))[0],
            hasher=lambda _path: H64,
        ),
        "repo_tags",
    )
    expect_error(
        lambda: native_image.inspect(
            PROFILE,
            runner=fixture_runner(host_value=host_bytes(os="linux/arm64"))[0],
            hasher=lambda _path: H64,
        ),
        "os",
    )
    expect_error(
        lambda: native_image.inspect(
            PROFILE,
            runner=fixture_runner(host_value=host_bytes(repo_digests=[REPOSITORY]))[0],
            hasher=lambda _path: H64,
        ),
        "repository separator",
    )
    expect_error(
        lambda: native_image.qualify(
            PROFILE,
            runner=fixture_runner(self_value=self_bytes(config_digest=REGISTRY))[0],
            hasher=lambda _path: H64,
        ),
        "config_digest",
    )
    malformed = self_bytes()[:-1]
    expect_error(
        lambda: native_image.inspect(
            PROFILE,
            runner=fixture_runner(self_value=malformed)[0],
            hasher=lambda _path: H64,
        ),
        "not canonical",
    )
    reordered = cj.encode(
        O(
            ("toolchain", O(*((key, executable(key)) for key in TOOLCHAIN_KEYS))),
            ("config_digest", DIGEST),
            ("cargo_cache_manifest_sha256", H64),
            ("cargo_config_sha256", H64),
        )
    )
    expect_error(
        lambda: native_image.inspect(
            PROFILE,
            runner=fixture_runner(self_value=reordered)[0],
            hasher=lambda _path: H64,
        ),
        "wrong member order",
    )
    bad_toolchain = O(*((key, executable("changed") if key == "rustc" else executable(key)) for key in TOOLCHAIN_KEYS))
    expect_error(
        lambda: native_image.qualify(
            PROFILE,
            runner=fixture_runner(self_value=self_bytes(toolchain=bad_toolchain))[0],
            hasher=lambda _path: H64,
        ),
        "toolchain identity",
    )
    expect_error(
        lambda: native_image.inspect(
            PROFILE,
            runner=fixture_runner(self_value=self_bytes(toolchain=O(*((key, executable(key)) for key in TOOLCHAIN_KEYS[:-1]))))[0],
            hasher=lambda _path: H64,
        ),
        "wrong member order",
    )
    expect_error(
        lambda: native_image.inspect(
            PROFILE,
            runner=fixture_runner(self_value=b"x" * (native_image._MAX_IDENTITY_BYTES + 1))[0],
            hasher=lambda _path: H64,
        ),
        "exceeds the fixed output limit",
    )

    mismatch_events = []
    mismatch_runner, _unused, _host, _run, _cleanup = fixture_runner(events=mismatch_events)
    expect_error(
        lambda: native_image.inspect(PROFILE, runner=mismatch_runner, hasher=lambda _path: ONE64),
        "does not match profile",
    )
    assert mismatch_events == []

    runner, _events, host_command, run_command, cleanup_command = fixture_runner(
        failures={}
    )
    calls = []

    def failing_run(argv):
        calls.append(argv)
        if argv == host_command:
            return host_bytes()
        if argv == run_command:
            raise native_host.NativeHostError("injected image timeout")
        if argv == cleanup_command:
            return b"removed\n"
        raise AssertionError(argv)

    expect_error(
        lambda: native_image.inspect(PROFILE, runner=failing_run, hasher=lambda _path: H64),
        "fixed container cleanup",
    )
    assert calls == [host_command, run_command, cleanup_command]

    production_failure_events = []
    original_run_docker_commands = native_host.run_docker_commands

    def production_failure_runner(commands, expected_client_hash=None, *, between=None):
        assert expected_client_hash == H64
        assert between is None
        command = tuple(commands)[0]
        production_failure_events.append(command)
        if command == host_command:
            return (host_bytes(),), H64
        if command == run_command:
            raise KeyboardInterrupt()
        if command == cleanup_command:
            return (b"removed\n",), H64
        raise AssertionError(command)

    native_host.run_docker_commands = production_failure_runner
    try:
        expect_error(lambda: native_image.inspect(PROFILE), "fixed container cleanup")
    finally:
        native_host.run_docker_commands = original_run_docker_commands
    assert production_failure_events == [host_command, run_command, cleanup_command]

    def cleanup_failure(argv):
        calls.append(argv)
        if argv == host_command:
            return host_bytes()
        if argv == run_command:
            raise native_host.NativeHostError("injected image timeout")
        if argv == cleanup_command:
            raise native_host.NativeHostError("injected cleanup failure")
        raise AssertionError(argv)

    calls.clear()
    expect_error(
        lambda: native_image.inspect(PROFILE, runner=cleanup_failure, hasher=lambda _path: H64),
        "cleanup is uncertain",
    )
    assert calls == [host_command, run_command, cleanup_command]


main()
PY
echo "native image qualification checks passed"
