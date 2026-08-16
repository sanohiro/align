"""Build the fixed Docker argv for one evidence child.

This module is an argv boundary, not a Docker or host verifier.  The future
controller must verify the profile, opened host paths, daemon, image, and
child lifecycle before calling it.  Once called, this layer makes the
container's security-relevant selectors explicit and leaves no ambient
environment or caller-selected image/namespace option to Docker.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Any, Mapping, Sequence


class ContainerError(ValueError):
    """A child launch request cannot satisfy the fixed container boundary."""


DOCKER = "/usr/bin/docker"
CONTAINER_PREFIX = "align-evidence-"
IMAGE_CONTAINER_PREFIX = "align-evidence-image-"
IMAGE_INSPECTION_EXECUTABLE = "/opt/align-evidence/image-self-inspect"
CONTAINER_UID = 65532
CONTAINER_GID = 65532
MEMORY_BYTES = 4 * 1024 * 1024 * 1024
MEMORY_SWAP_BYTES = MEMORY_BYTES
PIDS_LIMIT = 256
NOFILE_LIMIT = 1024
TMPFS_BYTES = 64 * 1024 * 1024
PATH = "/src"
TARGET = "/target"
WORK = "/work"
CARGO_HOME = "/cargo"
TOOLCHAIN = "/toolchain"

_HEX64 = re.compile(r"[0-9a-f]{64}\Z")
_DIGEST = re.compile(r"sha256:[0-9a-f]{64}\Z")
_NAME = re.compile(r"[A-Za-z0-9._/:+=@-]{1,255}\Z")
_HOST_PATH = re.compile(r"/[A-Za-z0-9._/-]{1,4095}\Z")
_GUEST_PATH = re.compile(r"/(?:src|work)/[A-Za-z0-9._/-]{1,4095}\Z")


@dataclass(frozen=True)
class ContainerLaunch:
    """Host paths and a trusted guest command for one isolated child."""

    child_id: str
    source: str
    target: str
    work: str
    cargo_home: str
    toolchain: str
    command: tuple[str, ...]


def _error(label: str, message: str) -> None:
    raise ContainerError(f"{label}: {message}")


def _string(value: Any, pattern: re.Pattern[str], label: str) -> str:
    if not isinstance(value, str) or pattern.fullmatch(value) is None:
        _error(label, "has an invalid grammar")
    return value


def _host_path(value: Any, label: str) -> str:
    path = _string(value, _HOST_PATH, label)
    parts = path.split("/")
    if path == "/" or path.endswith("/") or any(part in ("", ".", "..") for part in parts[1:]):
        _error(label, "must be an absolute non-root path without aliases")
    return path


def _profile_field(profile: Any, path: Sequence[str]) -> Any:
    value: Any = profile
    for component in path:
        if not isinstance(value, Mapping) or component not in value:
            _error("profile", f"is missing {'.'.join(path)}")
        value = value[component]
    return value


def _profile_identity(profile: Any) -> tuple[str, str, str, str]:
    image = _profile_field(profile, ("image",))
    machine = _profile_field(profile, ("machine",))
    if not isinstance(image, Mapping) or not isinstance(machine, Mapping):
        _error("profile", "image and machine must be objects")
    _string(image.get("registry_digest"), _DIGEST, "profile.image.registry_digest")
    local_image_id = _string(image.get("local_image_id"), _HEX64, "profile.image.local_image_id")
    if _string(image.get("platform"), _NAME, "profile.image.platform") != "linux/amd64":
        _error("profile.image.platform", "must be linux/amd64")
    if _string(machine.get("architecture"), _NAME, "profile.machine.architecture") != "x86_64":
        _error("profile.machine.architecture", "must be x86_64")
    cpu_set = _string(machine.get("benchmark_cpu_set"), _NAME, "profile.machine.benchmark_cpu_set")
    numa_set = _string(machine.get("numa_set"), _NAME, "profile.machine.numa_set")
    docker = _profile_field(profile, ("docker",))
    if not isinstance(docker, Mapping):
        _error("profile", "docker must be an object")
    cgroup_driver = _string(docker.get("cgroup_driver"), _NAME, "profile.docker.cgroup_driver")
    cgroup_parent = _string(docker.get("cgroup_parent"), _NAME, "profile.docker.cgroup_parent")
    expected_parent = {"cgroupfs": "/", "systemd": "-.slice"}.get(cgroup_driver)
    if expected_parent is None:
        _error("profile.docker.cgroup_driver", "must be cgroupfs or systemd")
    if cgroup_parent != expected_parent:
        _error("profile.docker.cgroup_parent", f"must be {expected_parent} for {cgroup_driver}")
    return f"sha256:{local_image_id}", cpu_set, numa_set, cgroup_parent


def _command(command: Sequence[str]) -> tuple[str, ...]:
    if not isinstance(command, (tuple, list)) or not command:
        _error("command", "must be a non-empty argv sequence")
    result = tuple(command)
    for index, argument in enumerate(result):
        if not isinstance(argument, str) or not argument or "\x00" in argument:
            _error(f"command[{index}]", "must be a non-empty NUL-free string")
        if any(ord(char) < 0x20 or ord(char) > 0x7E for char in argument):
            _error(f"command[{index}]", "must contain printable ASCII only")
    if _GUEST_PATH.fullmatch(result[0]) is None:
        _error("command[0]", "must be a source or work executable path")
    parts = result[0].split("/")
    if any(part in ("", ".", "..") for part in parts[1:]):
        _error("command[0]", "must not contain path aliases")
    return result


def _launch(value: Any) -> ContainerLaunch:
    if not isinstance(value, ContainerLaunch):
        _error("launch", "must be a ContainerLaunch")
    child_id = _string(value.child_id, _HEX64, "launch.child_id")
    paths = (
        ("launch.source", _host_path(value.source, "launch.source")),
        ("launch.target", _host_path(value.target, "launch.target")),
        ("launch.work", _host_path(value.work, "launch.work")),
        ("launch.cargo_home", _host_path(value.cargo_home, "launch.cargo_home")),
        ("launch.toolchain", _host_path(value.toolchain, "launch.toolchain")),
    )
    host_paths = [path for _, path in paths]
    if len(host_paths) != len(set(host_paths)):
        _error("launch", "source, target, work, Cargo home, and toolchain must be distinct")
    for index, left in enumerate(host_paths):
        for right in host_paths[index + 1 :]:
            if right.startswith(left + "/") or left.startswith(right + "/"):
                _error("launch", "source, target, work, Cargo home, and toolchain must not nest")
    return ContainerLaunch(
        child_id=child_id,
        source=paths[0][1],
        target=paths[1][1],
        work=paths[2][1],
        cargo_home=paths[3][1],
        toolchain=paths[4][1],
        command=_command(value.command),
    )


def _mount(host: str, guest: str, read_only: bool) -> str:
    suffix = ",readonly" if read_only else ""
    return f"type=bind,src={host},dst={guest}{suffix}"


def _common_argv(profile: Any, name: str) -> tuple[str, ...]:
    """Return the security and resource selectors shared by every image child."""

    _digest, cpu_set, numa_set, cgroup_parent = _profile_identity(profile)
    return (
        DOCKER,
        "run",
        "--rm",
        "--pull=never",
        "--network=none",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges",
        "--security-opt=seccomp=/opt/align-evidence/seccomp.json",
        "--security-opt=apparmor=align-evidence-v1",
        f"--user={CONTAINER_UID}:{CONTAINER_GID}",
        f"--cpuset-cpus={cpu_set}",
        f"--cpuset-mems={numa_set}",
        f"--cgroup-parent={cgroup_parent}",
        f"--memory={MEMORY_BYTES}",
        f"--memory-swap={MEMORY_SWAP_BYTES}",
        f"--pids-limit={PIDS_LIMIT}",
        f"--ulimit=nofile={NOFILE_LIMIT}:{NOFILE_LIMIT}",
        f"--tmpfs=/tmp:rw,noexec,nosuid,nodev,size={TMPFS_BYTES}",
        "--ipc=private",
        "--pid=private",
        "--uts=private",
        "--cgroupns=private",
        f"--name={name}",
    )


def build_argv(profile: Any, launch: ContainerLaunch) -> tuple[str, ...]:
    """Build one complete, fixed Docker invocation without reading ambient state."""

    digest, _cpu_set, _numa_set, _cgroup_parent = _profile_identity(profile)
    launch = _launch(launch)
    name = f"{CONTAINER_PREFIX}{launch.child_id}"
    return _common_argv(profile, name) + (
        "--env=PATH=/toolchain/bin:/usr/bin:/bin",
        "--env=LC_ALL=C",
        "--env=TZ=UTC",
        "--env=HOME=/nonexistent",
        "--env=CARGO_NET_OFFLINE=true",
        "--env=CARGO_HOME=/cargo",
        "--env=CARGO_TARGET_DIR=/target",
        "--env=TMPDIR=/tmp",
        "--env=ALIGN_BENCH_WORK_DIR=/work",
        f"--mount={_mount(launch.source, PATH, True)}",
        f"--mount={_mount(launch.target, TARGET, False)}",
        f"--mount={_mount(launch.work, WORK, False)}",
        f"--mount={_mount(launch.cargo_home, CARGO_HOME, True)}",
        f"--mount={_mount(launch.toolchain, TOOLCHAIN, True)}",
        "--workdir=/work",
        digest,
        *launch.command,
    )


def build_image_inspection_argv(profile: Any, child_id: str) -> tuple[str, ...]:
    """Build the fixed no-mount command that self-inspects the pinned image."""

    digest, _cpu_set, _numa_set, _cgroup_parent = _profile_identity(profile)
    child_id = _string(child_id, _HEX64, "image inspection child_id")
    name = f"{IMAGE_CONTAINER_PREFIX}{child_id}"
    return _common_argv(profile, name) + (
        "--env=PATH=/toolchain/bin:/usr/bin:/bin",
        "--env=LC_ALL=C",
        "--env=TZ=UTC",
        "--env=HOME=/nonexistent",
        "--env=PYTHONDONTWRITEBYTECODE=1",
        "--env=CARGO_NET_OFFLINE=true",
        "--env=CARGO_HOME=/cargo",
        "--env=TMPDIR=/tmp",
        "--workdir=/",
        f"--entrypoint={IMAGE_INSPECTION_EXECUTABLE}",
        digest,
    )


def build_image_inspection_cleanup_argv(profile: Any, child_id: str) -> tuple[str, ...]:
    """Build the one fixed best-effort removal command for an uncertain image run."""

    _profile_identity(profile)
    child_id = _string(child_id, _HEX64, "image inspection child_id")
    name = f"{IMAGE_CONTAINER_PREFIX}{child_id}"
    return (DOCKER, "container", "rm", "--force", "--volumes", name)
