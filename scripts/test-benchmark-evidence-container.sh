#!/usr/bin/env bash
# Deterministic owner for the pinned evidence-container argv boundary.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
from scripts.benchmark_evidence import container


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
}


def launch(**overrides):
    values = {
        "child_id": H64,
        "source": "/evidence/source",
        "target": "/evidence/target",
        "work": "/evidence/work",
        "cargo_home": "/evidence/cargo",
        "toolchain": "/evidence/toolchain",
        "command": ("/work/bench", "native"),
    }
    values.update(overrides)
    return container.ContainerLaunch(**values)


def expect_error(call, fragment):
    try:
        call()
    except container.ContainerError as exc:
        assert fragment in str(exc), (fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid container request: {fragment}")


def main():
    argv = container.build_argv(PROFILE, launch())
    assert argv[0:3] == ("/usr/bin/docker", "run", "--rm")
    assert "--pull=never" in argv
    assert "--network=none" in argv
    assert "--read-only" in argv
    assert "--cap-drop=ALL" in argv
    assert "--security-opt=no-new-privileges" in argv
    assert "--security-opt=seccomp=/opt/align-evidence/seccomp.json" in argv
    assert "--security-opt=apparmor=align-evidence-v1" in argv
    assert f"--user={container.CONTAINER_UID}:{container.CONTAINER_GID}" in argv
    assert "--cpuset-cpus=0-7" in argv
    assert "--cpuset-mems=0" in argv
    assert "--cgroup-parent=/" in argv
    assert f"--memory={container.MEMORY_BYTES}" in argv
    assert f"--memory-swap={container.MEMORY_SWAP_BYTES}" in argv
    assert f"--pids-limit={container.PIDS_LIMIT}" in argv
    assert f"--ulimit=nofile={container.NOFILE_LIMIT}:{container.NOFILE_LIMIT}" in argv
    assert f"--tmpfs=/tmp:rw,noexec,nosuid,nodev,size={container.TMPFS_BYTES}" in argv
    assert {arg for arg in argv if arg.startswith("--ipc=")} == {"--ipc=private"}
    assert {arg for arg in argv if arg.startswith("--pid=")} == {"--pid=private"}
    assert {arg for arg in argv if arg.startswith("--uts=")} == {"--uts=private"}
    assert {arg for arg in argv if arg.startswith("--cgroupns=")} == {"--cgroupns=private"}
    assert f"--name=align-evidence-{H64}" in argv
    assert "--env=PATH=/toolchain/bin:/usr/bin:/bin" in argv
    assert "--env=LC_ALL=C" in argv
    assert "--env=TZ=UTC" in argv
    assert "--env=HOME=/nonexistent" in argv
    assert "--env=CARGO_NET_OFFLINE=true" in argv
    assert "--env=CARGO_HOME=/cargo" in argv
    assert "--env=CARGO_TARGET_DIR=/target" in argv
    assert "--env=TMPDIR=/tmp" in argv
    assert "--env=ALIGN_BENCH_WORK_DIR=/work" in argv
    assert "--mount=type=bind,src=/evidence/source,dst=/src,readonly" in argv
    assert "--mount=type=bind,src=/evidence/target,dst=/target" in argv
    assert "--mount=type=bind,src=/evidence/work,dst=/work" in argv
    assert "--mount=type=bind,src=/evidence/cargo,dst=/cargo,readonly" in argv
    assert "--mount=type=bind,src=/evidence/toolchain,dst=/toolchain,readonly" in argv
    assert "--workdir=/work" in argv
    assert argv[-3:] == ("sha256:" + H64, "/work/bench", "native")
    assert not any("privileged" in arg or "network=host" in arg for arg in argv)
    assert not any(arg.startswith("--cpu-quota") or arg.startswith("--cpu-period") for arg in argv)

    systemd_profile = {
        **PROFILE,
        "docker": {"cgroup_driver": "systemd", "cgroup_parent": "-.slice"},
    }
    systemd_argv = container.build_argv(systemd_profile, launch())
    assert "--cgroup-parent=-.slice" in systemd_argv

    expect_error(lambda: container.build_argv({"image": {}, "machine": {}}, launch()), "registry_digest")
    expect_error(
        lambda: container.build_argv(
            {"image": {"registry_digest": "ubuntu:24.04", "local_image_id": H64, "platform": "linux/amd64"}, "machine": PROFILE["machine"]},
            launch(),
        ),
        "registry_digest",
    )
    expect_error(
        lambda: container.build_argv(
            {"image": {"registry_digest": "sha256:" + H64, "local_image_id": H64, "platform": "linux/arm64"}, "machine": PROFILE["machine"]},
            launch(),
        ),
        "linux/amd64",
    )
    expect_error(
        lambda: container.build_argv(
            {"image": PROFILE["image"], "machine": {**PROFILE["machine"], "architecture": "aarch64"}},
            launch(),
        ),
        "x86_64",
    )
    expect_error(lambda: container.build_argv(PROFILE, launch(child_id="f")), "child_id")
    expect_error(lambda: container.build_argv(PROFILE, launch(source="relative")), "source")
    expect_error(lambda: container.build_argv(PROFILE, launch(target="/")), "target")
    expect_error(lambda: container.build_argv(PROFILE, launch(work="/evidence/../work")), "work")
    expect_error(lambda: container.build_argv(PROFILE, launch(toolchain="/evidence/source")), "distinct")
    expect_error(lambda: container.build_argv(PROFILE, launch(target="/evidence/source/target")), "nest")
    expect_error(lambda: container.build_argv(PROFILE, launch(command=("/bin/sh", "-c", "echo"))), "executable path")
    expect_error(lambda: container.build_argv(PROFILE, launch(command=("/work/../src/bench",))), "path aliases")
    expect_error(lambda: container.build_argv(PROFILE, launch(command=("/work/bench\x00",))), "NUL-free")
    expect_error(lambda: container.build_argv(PROFILE, launch(command=())), "non-empty argv")
    expect_error(
        lambda: container.build_argv(
            {"image": PROFILE["image"], "machine": {**PROFILE["machine"], "benchmark_cpu_set": "0;--network=host"}},
            launch(),
        ),
        "benchmark_cpu_set",
    )
    expect_error(
        lambda: container.build_argv(
            {**PROFILE, "docker": {"cgroup_driver": "systemd", "cgroup_parent": "/"}},
            launch(),
        ),
        "must be -.slice",
    )


main()
PY
